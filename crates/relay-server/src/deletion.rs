//! Permanent branch removal under the workspace writer and SQLite transaction.
use crate::*;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(super) fn redacted_receipt(request: &str) -> String {
    format!("deleted:{:x}", Sha256::digest(request.as_bytes()))
}

pub(super) fn delete(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &mut Snapshot,
    session_id: &str,
    request_id: &str,
) -> Result<(), Error> {
    let before = snapshot.clone();
    let ids = before.session_branch_ids(session_id);
    snapshot
        .apply(
            Command::DeleteSessionBranch {
                session_id: session_id.into(),
            },
            request_id,
            now(),
        )
        .map_err(Error::invalid)?;
    let drafts = conversation::read_drafts(transaction)?;
    // The branch's transcript, from the messages table (schema 31).
    let mut deleted = Vec::new();
    for id in &ids {
        deleted.extend(messages::session(transaction, id).map_err(Error::internal)?);
    }
    let mut references = ids.clone();
    references.extend(deleted.iter().map(|m| m.id.clone()));
    references.extend(
        before
            .submissions
            .iter()
            .filter(|s| ids.contains(&s.session_id))
            .map(|s| s.id.clone()),
    );
    references.extend(
        before
            .tool_permissions
            .iter()
            .filter(|p| ids.contains(&p.session_id))
            .map(|p| p.id.clone()),
    );
    references.extend(
        before
            .agent_configs
            .iter()
            .filter(|c| !snapshot.agent_configs.iter().any(|kept| kept.id == c.id))
            .map(|c| c.id.clone()),
    );
    let mut removed_assets = HashSet::new();
    let mut retained_assets = HashSet::new();
    for message in &deleted {
        removed_assets.extend(assets(&message.parts).into_iter().map(|a| a.id.clone()));
    }
    // An asset stays while any other session's message mentions its id. A
    // substring match can only over-retain, never delete a referenced asset.
    for asset in &removed_assets {
        let kept: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE instr(data,?1)>0 AND session_id NOT IN (SELECT value FROM json_each(?2)))",
                params![asset, serde_json::to_string(&ids).map_err(Error::internal)?],
                |r| r.get(0),
            )
            .map_err(Error::internal)?;
        if kept {
            retained_assets.insert(asset.clone());
        }
    }
    for turn in &before.submissions {
        let target = if ids.contains(&turn.session_id) {
            &mut removed_assets
        } else {
            &mut retained_assets
        };
        target.extend(assets(&turn.parts).into_iter().map(|a| a.id.clone()));
    }
    for draft in &drafts {
        let target = if ids.contains(&draft.session_id) {
            &mut removed_assets
        } else {
            &mut retained_assets
        };
        target.extend(assets(&draft.parts).into_iter().map(|a| a.id.clone()));
    }
    // Keep retry identities without retaining deleted prompt/attachment contents.
    let receipts = {
        let mut query = transaction
            .prepare("SELECT request_id,request FROM receipts")
            .map_err(Error::internal)?;
        query
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(Error::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::internal)?
    };
    for (id, request) in receipts {
        if ids
            .iter()
            .any(|session| session.strip_prefix("session-") == Some(id.as_str()))
            || serde_json::from_str::<serde_json::Value>(&request)
                .ok()
                .is_some_and(|value| references_branch(&value, &references))
        {
            transaction
                .execute(
                    "UPDATE receipts SET request=?1 WHERE request_id=?2",
                    params![redacted_receipt(&request), id],
                )
                .map_err(Error::internal)?;
        }
    }
    for id in &ids {
        transaction
            .execute("DELETE FROM spawn_native_calls WHERE caller=?1", [id])
            .map_err(Error::internal)?;
        transaction
            .execute("DELETE FROM spawn_notice_consumption WHERE caller=?1", [id])
            .map_err(Error::internal)?;
        transaction.execute("DELETE FROM spawn_requests WHERE attempt IN (SELECT id FROM spawn_attempts WHERE caller=?1)", [id]).map_err(Error::internal)?;
        transaction
            .execute("DELETE FROM spawn_attempts WHERE caller=?1", [id])
            .map_err(Error::internal)?;
        // Keep parent-side retry identities, but never return a deleted child
        // as a live session or allow the same request to recreate it.
        transaction.execute("UPDATE spawn_attempts SET child=NULL,phase='child_deleted',response=json_object('spawn_attempt',id,'phase','child_deleted','deleted',json('true')) WHERE child=?1", [id]).map_err(Error::internal)?;
        for table in [
            "drafts",
            "runs",
            "processes",
            "workspace_waits",
            "lifecycle_holds",
        ] {
            transaction
                .execute(&format!("DELETE FROM {table} WHERE session_id=?1"), [id])
                .map_err(Error::internal)?;
        }
        transaction
            .execute(
                "DELETE FROM draft_receipts WHERE json_extract(response,'$.session_id')=?1",
                [id],
            )
            .map_err(Error::internal)?;
    }
    messages::delete_sessions(transaction, ids.iter().map(String::as_str))
        .map_err(Error::internal)?;
    for asset in removed_assets.difference(&retained_assets) {
        transaction
            .execute("DELETE FROM assets WHERE id=?1", [asset])
            .map_err(Error::internal)?;
    }
    // Validate every owned path before removing any files. If cleanup fails, the
    // database transaction rolls back and the archived transcript remains readable.
    commands::delete_branch_workspaces(&before, snapshot, &ids)?;
    Ok(())
}

fn references_branch(value: &serde_json::Value, ids: &HashSet<String>) -> bool {
    match value {
        serde_json::Value::String(value) => ids.contains(value),
        serde_json::Value::Array(values) => values.iter().any(|v| references_branch(v, ids)),
        serde_json::Value::Object(values) => values.values().any(|v| references_branch(v, ids)),
        _ => false,
    }
}
