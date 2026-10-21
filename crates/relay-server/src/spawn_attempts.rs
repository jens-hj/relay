//! Durable pre-child outcomes. No prompts, credentials or raw errors are stored.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) const REQUEST: &str = "x-relay-spawn-request";

fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// A request identity is scoped to a caller/run, never an authorization grant.
pub(super) fn begin(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    request: &str,
    input: &Value,
) -> Result<(String, Option<Value>), Error> {
    if request.is_empty() || request.len() > 256 {
        return Err(Error::invalid("Invalid spawn request identity"));
    }
    let id = hash(&serde_json::to_string(&(caller, run, request)).map_err(Error::internal)?);
    let input_hash = hash(&input.to_string());
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    let snapshot = store.snapshot()?;
    agent_tools::active_caller(&store, &snapshot, caller, run)?;
    let project = &snapshot
        .sessions
        .iter()
        .find(|s| s.id == caller)
        .unwrap()
        .project_id;
    let previous: Option<(String, String, Option<String>)> = store
        .connection
        .query_row(
            "SELECT input_hash,phase,response FROM spawn_attempts WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(Error::internal)?;
    if let Some((saved, phase, response)) = previous {
        if saved != input_hash {
            return Err(Error::invalid(
                "Spawn request identity already used for different arguments",
            ));
        }
        let value = match response {
            Some(response) => serde_json::from_str(&response).map_err(Error::internal)?,
            None => json!({"spawn_attempt":id,"phase":phase,"pending":true}),
        };
        return Ok((id, Some(value)));
    }
    let transaction = store.connection.transaction().map_err(Error::internal)?;
    transaction.execute("INSERT INTO spawn_attempts(id,caller,run,project,input_hash,phase) VALUES(?1,?2,?3,?4,?5,'validating')", params![id,caller,run,project,input_hash]).map_err(Error::internal)?;
    transaction.commit().map_err(Error::internal)?;
    Ok((id, None))
}

pub(super) fn phase(workspace: &Workspace, id: Option<&str>, phase: &str) -> Result<(), Error> {
    if let Some(id) = id {
        workspace
            .store
            .lock()
            .map_err(Error::internal)?
            .connection
            .execute(
                "UPDATE spawn_attempts SET phase=?1 WHERE id=?2 AND response IS NULL",
                params![phase, id],
            )
            .map_err(Error::internal)?;
    }
    Ok(())
}

/// Child identity and the created outcome commit with child storage, so restart
/// never labels a committed child as a pre-child cancellation or forks it twice.
pub(super) fn created(
    transaction: &rusqlite::Transaction<'_>,
    id: Option<&str>,
    child: &str,
) -> Result<(), Error> {
    if let Some(id) = id {
        let response = json!({"spawn_attempt":id,"phase":"created_queued","agent":child});
        transaction
            .execute(
                "UPDATE spawn_attempts SET phase='created_queued',child=?1,response=?2 WHERE id=?3",
                params![child, response.to_string(), id],
            )
            .map_err(Error::internal)?;
    }
    Ok(())
}

/// Only fixed, actionable descriptions cross the durable recovery boundary.
/// Raw filesystem, model-discovery and harness errors can contain secrets.
pub(super) fn finish(
    workspace: &Workspace,
    id: &str,
    cancelled: bool,
    denied: bool,
) -> Result<Value, Error> {
    let store = workspace.store.lock().map_err(Error::internal)?;
    let (phase, response): (String, Option<String>) = store
        .connection
        .query_row(
            "SELECT phase,response FROM spawn_attempts WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(Error::internal)?;
    if let Some(response) = response {
        return serde_json::from_str(&response).map_err(Error::internal);
    }
    let (outcome, message) = if cancelled {
        (
            "cancelled",
            "Spawn cancelled before a child was created; inspect the attempt before retrying",
        )
    } else if denied {
        (
            "denied",
            "Human denied spawn approval; no child was created",
        )
    } else if phase == "preparing_workspace" {
        (
            "workspace_failed",
            "Spawn failed while preparing workspaces; no child was created",
        )
    } else {
        (
            "rejected",
            "Spawn rejected before a child was created; check model, scope and approval policy",
        )
    };
    let value = json!({"spawn_attempt":id,"phase":outcome,"error":message,"isError":true});
    store
        .connection
        .execute(
            "UPDATE spawn_attempts SET phase=?1,response=?2 WHERE id=?3 AND response IS NULL",
            params![outcome, value.to_string(), id],
        )
        .map_err(Error::internal)?;
    Ok(value)
}

fn identity(caller: &str, run: &str, request: &str) -> Result<String, Error> {
    Ok(hash(
        &serde_json::to_string(&(caller, run, request)).map_err(Error::internal)?,
    ))
}

/// Bind only a native Relay MCP start event. A later unrelated result, even
/// with a copied public receipt, cannot create a binding or acknowledge it.
fn bind_native_calls(
    connection: &Connection,
    snapshot: &Snapshot,
    caller: &str,
    run: &str,
) -> Result<(), Error> {
    for message in snapshot.messages.iter().filter(|m| {
        m.session_id == caller
            && m.kind == "tool_call"
            && matches!(m.author.as_str(), "Codex" | "Claude Code")
    }) {
        let Some(tool) = message.tool.as_ref().filter(|t| {
            t.run_id == run && t.name == "mcp__relay__spawn_agent" && t.state == ToolState::Running
        }) else {
            continue;
        };
        let Ok(input) = serde_json::from_str::<Value>(&tool.input) else {
            continue;
        };
        let Some(call) = input["call_id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256)
        else {
            continue;
        };
        let attempt = identity(caller, run, call)?;
        connection.execute("INSERT OR IGNORE INTO spawn_native_calls(caller,run,native_id,attempt,input_hash) VALUES(?1,?2,?3,?4,?5)", params![caller,run,message.id,attempt,hash(&input.to_string())]).map_err(Error::internal)?;
    }
    Ok(())
}

fn observed(
    connection: &Connection,
    snapshot: &Snapshot,
    caller: &str,
    run: &str,
    attempt: &str,
    response: &Value,
) -> Result<bool, Error> {
    for message in snapshot
        .messages
        .iter()
        .filter(|m| m.session_id == caller && m.kind == "tool_call")
    {
        let Some(tool) = message.tool.as_ref().filter(|t| {
            t.run_id == run
                && t.name == "mcp__relay__spawn_agent"
                && matches!(t.state, ToolState::Completed | ToolState::Failed)
        }) else {
            continue;
        };
        let bound: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM spawn_native_calls n JOIN spawn_attempts a ON a.id=n.attempt WHERE n.caller=?1 AND n.run=?2 AND n.native_id=?3 AND n.attempt=?4 AND n.input_hash=a.input_hash)", params![caller,run,message.id,attempt], |r|r.get(0)).map_err(Error::internal)?;
        if !bound {
            continue;
        }
        let Ok(body) = serde_json::from_str::<Value>(&message.body) else {
            continue;
        };
        // Codex 0.161's McpToolCallResult omits the bridge's isError;
        // failure is the enclosing native item's terminal status. Claude
        // exposes its single tool_result text directly, with is_error also
        // mapped to ToolState. Neither permits recursive receipt searches.
        let failed = response["isError"] == true;
        if (tool.state == ToolState::Failed) != failed {
            continue;
        }
        if (message.author == "Codex" && codex_result(&body, response))
            || (message.author == "Claude Code" && body == *response)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn codex_result(body: &Value, response: &Value) -> bool {
    let Some(result) = body.as_object() else {
        return false;
    };
    if result
        .keys()
        .any(|key| !matches!(key.as_str(), "content" | "structuredContent" | "_meta"))
    {
        return false;
    }
    let Some(content) = result.get("content").and_then(Value::as_array) else {
        return false;
    };
    let [block] = content.as_slice() else {
        return false;
    };
    let Some(text) = block.as_object() else {
        return false;
    };
    text.keys()
        .all(|key| matches!(key.as_str(), "type" | "text" | "annotations" | "_meta"))
        && block["type"] == "text"
        && block["text"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .is_some_and(|value| value == *response)
}
pub(super) fn recover(connection: &Connection) -> Result<(), Error> {
    // Never replay workspace preparation, approval or child creation on restart.
    connection.execute("UPDATE spawn_attempts SET phase='cancelled',response=json_object('spawn_attempt',id,'phase','cancelled','error','Server restarted before spawn completed; no child was created. Continuation requires a new authorized request.','isError',json('true')) WHERE response IS NULL", []).map_err(Error::internal)?;
    Ok(())
}

/// A native harness has accepted the notice turn. Reservation and process
/// launch alone never set this marker. It does not authorize any spawn replay.
pub(super) fn consumed(workspace: &Workspace, caller: &str, run: &str) -> Result<(), Error> {
    if !run.starts_with(notices::PREFIX) {
        return Ok(());
    }
    let store = workspace.store.lock().map_err(Error::internal)?;
    let snapshot = store.snapshot()?;
    agent_tools::active_caller(&store, &snapshot, caller, run)?;
    store.connection.execute("INSERT OR IGNORE INTO spawn_notice_consumption(notice_id,caller) SELECT notice_id,caller FROM spawn_attempts WHERE caller=?1 AND notice_id=?2 AND delivered=0", params![caller,run]).map_err(Error::internal)?;
    Ok(())
}

/// Withdraw a provisional recovery turn when the harness records its exact
/// correlated terminal result. Called in the same transaction as harness events.
pub(super) fn settle(
    connection: &Connection,
    snapshot: &mut Snapshot,
    caller: &str,
    run: &str,
) -> Result<(), Error> {
    bind_native_calls(connection, snapshot, caller, run)?;
    let rows = {
        let mut query = connection.prepare("SELECT id,run,response,notice_id FROM spawn_attempts WHERE caller=?1 AND run=?2 AND response IS NOT NULL AND delivered=0").map_err(Error::internal)?;
        query
            .query_map([caller, run], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(Error::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::internal)?
    };
    for (id, run, response, notice) in rows {
        let response: Value = serde_json::from_str(&response).map_err(Error::internal)?;
        if observed(connection, snapshot, caller, &run, &id, &response)? {
            connection
                .execute("UPDATE spawn_attempts SET delivered=1 WHERE id=?1", [&id])
                .map_err(Error::internal)?;
            if let Some(notice) = notice {
                snapshot.submissions.retain(|s| {
                    s.id != notice
                        || !matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
                });
            }
        }
    }
    Ok(())
}

/// Terminal outcomes enter the parent's durable queue while it is active;
/// synchronous harness delivery withdraws that provisional turn before queue
/// advancement. No active harness is interrupted, and no approval is replayed.
pub(super) fn flush(workspace: &Workspace, caller: &str, allow_launch: bool) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    if store.closing {
        return Ok(());
    }
    let rows = {
        let mut query = store.connection.prepare("SELECT id,run,project,response,notice_id,EXISTS(SELECT 1 FROM spawn_notice_consumption n WHERE n.notice_id=a.notice_id AND n.caller=a.caller) FROM spawn_attempts a WHERE caller=?1 AND response IS NOT NULL AND delivered=0 ORDER BY rowid").map_err(Error::internal)?;
        query
            .query_map([caller], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, bool>(5)?,
                ))
            })
            .map_err(Error::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::internal)?
    };
    for (id, _run, project, response, reserved, consumed) in rows {
        if consumed {
            continue;
        }
        let mut snapshot = store.snapshot()?;
        let Some(parent) = snapshot
            .sessions
            .iter()
            .find(|s| s.id == caller && s.project_id == project)
        else {
            continue;
        };
        if let Some(notice) = &reserved
            && let Some(turn) = snapshot
                .submissions
                .iter_mut()
                .find(|s| &s.id == notice && s.session_id == caller)
        {
            if matches!(
                turn.state,
                SubmissionState::Interrupted | SubmissionState::Failed | SubmissionState::Completed
            ) {
                turn.state = SubmissionState::Paused;
                turn.error =
                    Some("Spawn outcome was not consumed; resume explicitly to deliver it".into());
                turn.interrupts_run = None;
                snapshot.revision = snapshot
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| Error::invalid("Revision exhausted"))?;
                store.save(&snapshot)?;
                workspace.snapshots.send_replace(snapshot);
            }
            // An explicit cancellation stays cancelled. Live, queued and
            // paused reservations keep their identity; never auto-launch again.
            continue;
        }
        let text = format!(
            "Relay: a spawn attempt has an undelivered outcome.\n\n{response}\n\nThis is an outcome receipt, not authorization to retry. A created_queued outcome identifies an existing child; inspect it before sending follow-up work. A rejected, denied or cancelled outcome created no child. Retain useful evidence and resolve any remaining work before cleanup."
        );
        let notice = reserved
            .clone()
            .unwrap_or_else(|| format!("{}spawn-{id}", notices::PREFIX));
        let launch = reserved.is_none()
            && allow_launch
            && parent
                .worker
                .as_ref()
                .is_some_and(|w| runtime::active(&w.status) || w.status == WorkerStatus::Completed);
        notices::enqueue(
            workspace,
            &mut store,
            snapshot,
            caller,
            notices::Delivery {
                text,
                id: notice,
                attempt: Some(&id),
                allow_launch: launch,
            },
        )?;
    }
    Ok(())
}

/// Caller-scoped state is also included in next-turn context, so a paused
/// recovery notice never hides an outcome from an explicit user continuation.
pub(super) fn view(workspace: &Workspace, caller: &str) -> Result<Vec<Value>, Error> {
    let store = workspace.store.lock().map_err(Error::internal)?;
    let snapshot = store.snapshot()?;
    let mut query = store.connection.prepare("SELECT id,phase,child,delivered,notice_id,EXISTS(SELECT 1 FROM spawn_notice_consumption n WHERE n.notice_id=a.notice_id AND n.caller=a.caller) FROM spawn_attempts a WHERE caller=?1 ORDER BY rowid DESC LIMIT 64").map_err(Error::internal)?;
    query.query_map([caller], |r| {
        let child = r.get::<_,Option<String>>(2)?;
        let worker = child.as_ref().and_then(|id|snapshot.sessions.iter().find(|s| &s.id == id)).and_then(|s|s.worker.as_ref());
        let notice = r.get::<_,Option<String>>(4)?;
        let state = snapshot.submissions.iter().find(|s|Some(&s.id)==notice.as_ref()).map(|s|&s.state);
        Ok(json!({"spawn_attempt":r.get::<_,String>(0)?,"phase":r.get::<_,String>(1)?,"agent":child,"response_delivered":r.get::<_,bool>(3)?,"notice":notice,"notice_state":state,"notice_consumed":r.get::<_,bool>(5)?,"child_status":worker.map(|w|&w.status),"launch_failed":worker.is_some_and(|w|w.status==WorkerStatus::Failed && w.launched_network_policy.is_none())}))
    }).map_err(Error::internal)?.collect::<Result<Vec<_>,_>>().map_err(Error::internal)
}

pub(super) fn recover_notices(workspace: &Workspace) -> Result<(), Error> {
    let parents = {
        let store = workspace.store.lock().map_err(Error::internal)?;
        let mut query = store
            .connection
            .prepare("SELECT DISTINCT caller FROM spawn_attempts WHERE delivered=0")
            .map_err(Error::internal)?;
        query
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(Error::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::internal)?
    };
    for parent in parents {
        flush(workspace, &parent, false)?;
    }
    Ok(())
}
