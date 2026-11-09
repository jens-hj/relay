//! An ancestor agent's controls over a descendant's queued turns: the same
//! view, edit, cancel, send-now and resume a human has, through the same
//! queue commands, sent as that agent.
use super::*;

pub(super) const TOOLS: [&str; 5] = [
    "view_agent_queue",
    "edit_queued_message",
    "cancel_queued_message",
    "send_queued_message_now",
    "resume_agent_queue",
];
/// Characters of a queued turn's text shown by `view_agent_queue`.
const PREVIEW_CHARS: usize = 4000;
const PAUSED_HINT: &str = "The queue is paused and nothing in it starts by itself, and a new message only queues behind it. Call resume_agent_queue to start it, or send_queued_message_now to start one turn first.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    agent: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Queued {
    agent: String,
    submission: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    agent: String,
    submission: String,
    text: String,
}

enum Op {
    Resume,
    Cancel(String),
    Promote(String),
    Edit(String, String),
}
impl Op {
    fn submission(&self) -> Option<&str> {
        match self {
            Op::Resume => None,
            Op::Cancel(id) | Op::Promote(id) | Op::Edit(id, _) => Some(id),
        }
    }
    fn description(&self, agent: &str) -> String {
        match self {
            Op::Resume => format!("Resume the queue of subagent {agent}"),
            Op::Cancel(id) => format!("Cancel queued message {id} of subagent {agent}"),
            Op::Promote(id) => format!("Send queued message {id} to subagent {agent} now"),
            Op::Edit(id, _) => format!("Edit queued message {id} of subagent {agent}"),
        }
    }
    fn summary(&self, agent: &str) -> String {
        match self {
            Op::Resume => format!("Resumed the queue of {agent}"),
            Op::Cancel(id) => format!("Cancelled queued message {id} of {agent}"),
            Op::Promote(id) => format!("Sent queued message {id} to {agent} now"),
            Op::Edit(id, _) => format!("Edited queued message {id} of {agent}"),
        }
    }
}

fn waiting(state: &SubmissionState) -> bool {
    matches!(state, SubmissionState::Queued | SubmissionState::Paused)
}
/// How many turns wait in a session's queue, and whether it is paused.
pub(super) fn summary(snapshot: &Snapshot, id: &str) -> Value {
    let turns = snapshot
        .submissions
        .iter()
        .filter(|s| s.session_id == id && waiting(&s.state));
    let (mut queued, mut paused) = (0, false);
    for turn in turns {
        queued += 1;
        paused |= turn.state == SubmissionState::Paused;
    }
    json!({"queued":queued,"paused":paused})
}
/// A session's unfinished turns, in queue order, as its ancestor sees them.
fn view(snapshot: &Snapshot, id: &str) -> Value {
    let session = snapshot.sessions.iter().find(|s| s.id == id);
    let turns = snapshot
        .submissions
        .iter()
        .filter(|s| {
            s.session_id == id
                && (waiting(&s.state)
                    || matches!(
                        s.state,
                        SubmissionState::Launching | SubmissionState::Running
                    ))
        })
        .map(|s| {
            let text = plain_text(&s.parts);
            let truncated = text.chars().count() > PREVIEW_CHARS;
            let sender = match &s.sender {
                Sender::User => "user",
                Sender::Agent { session_id } => session_id.as_str(),
            };
            json!({"submission":s.id,"state":s.state,"sender":sender,"text":text.chars().take(PREVIEW_CHARS).collect::<String>(),"truncated":truncated,"error":s.error,"approve_implementation":s.approve_implementation,"editable":waiting(&s.state)})
        })
        .collect::<Vec<_>>();
    let mut value = summary(snapshot, id);
    value["agent"] = json!(id);
    value["status"] = json!(session.and_then(|s| s.worker.as_ref()).map(|w| &w.status));
    value["archived"] = json!(session.is_some_and(|s| s.archived));
    value["turns"] = json!(turns);
    if value["paused"] == true {
        value["hint"] = json!(PAUSED_HINT);
    }
    value
}
/// The hint `message_agent` adds when its turn queued behind a paused queue.
pub(super) fn message_hint(snapshot: &Snapshot, id: &str) -> Option<&'static str> {
    snapshot
        .submissions
        .iter()
        .any(|s| s.session_id == id && s.state == SubmissionState::Paused)
        .then_some(PAUSED_HINT)
}

fn descendant(snapshot: &Snapshot, caller: &str, agent: &str) -> Result<(), Error> {
    if !snapshot.descendants(caller).iter().any(|s| s.id == agent) {
        return Err(Error::invalid(
            "Only your own descendants' queues can be managed",
        ));
    }
    Ok(())
}

pub(super) async fn call(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    tool: &str,
    input: Value,
    stop: &mut watch::Receiver<bool>,
) -> Result<Value, Error> {
    let (agent, op) = match tool {
        "view_agent_queue" => {
            let args: Target = parse(input)?;
            let snapshot = workspace.snapshots.borrow().clone();
            descendant(&snapshot, caller, &args.agent)?;
            return Ok(view(&snapshot, &args.agent));
        }
        "resume_agent_queue" => {
            let args: Target = parse(input)?;
            (args.agent, Op::Resume)
        }
        "cancel_queued_message" => {
            let args: Queued = parse(input)?;
            (args.agent, Op::Cancel(args.submission))
        }
        "send_queued_message_now" => {
            let args: Queued = parse(input)?;
            (args.agent, Op::Promote(args.submission))
        }
        "edit_queued_message" => {
            let args: Edit = parse(input)?;
            validate_prompt(&args.text)?;
            (args.agent, Op::Edit(args.submission, args.text))
        }
        _ => return Err(Error::invalid("Unknown Relay tool")),
    };
    control(workspace, caller, run, tool, agent, op, stop).await
}

/// The permissions a control is decided, and any approval asked, under.
#[derive(PartialEq)]
struct Policy {
    delegate: Permission,
    implement: Permission,
}
fn policy(snapshot: &Snapshot, caller: &str, agent: &str) -> Result<Policy, Error> {
    let find = |id: &str| {
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::invalid("Agent missing"))
    };
    Ok(Policy {
        delegate: profile(snapshot, find(caller)?)?.permissions[&Task::Delegate],
        implement: profile(snapshot, find(agent)?)?.permissions[&Task::Implement],
    })
}

async fn control(
    workspace: &Workspace,
    caller: &str,
    run: &str,
    tool: &str,
    agent: String,
    op: Op,
    stop: &mut watch::Receiver<bool>,
) -> Result<Value, Error> {
    let snapshot = workspace.snapshots.borrow().clone();
    descendant(&snapshot, caller, &agent)?;
    let decided = policy(&snapshot, caller, &agent)?;
    let edit = matches!(op, Op::Edit(..));
    if decided.delegate == Permission::Deny {
        return Err(Error::invalid("Profile denies managing subagents"));
    }
    // Edited text is the agent's own turn: like message_agent, it never
    // inherits an earlier approval and needs the target's implementation policy.
    if edit && decided.implement == Permission::Deny {
        return Err(Error::invalid("Target denies implementation"));
    }
    if decided.delegate == Permission::Ask || (edit && decided.implement == Permission::Ask) {
        let mut description = op.description(&agent);
        if edit && decided.implement == Permission::Ask {
            description.push_str(" and approve implementation for it");
        }
        approval(
            workspace,
            caller,
            run,
            &format!("mcp__relay__{tool}"),
            &description,
            stop,
        )
        .await?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (action, launch, current) = {
        let mut store = workspace.store.lock().map_err(Error::internal)?;
        if *stop.borrow() {
            return Err(unauthorized());
        }
        let mut current = store.snapshot()?;
        active_caller(&store, &current, caller, run)?;
        descendant(&current, caller, &agent)?;
        if policy(&current, caller, &agent)? != decided {
            return Err(Error::invalid("Agent policy changed; retry"));
        }
        if current.sessions.iter().any(|s| s.id == agent && s.archived) {
            return Err(Error::invalid("Unarchive this agent before continuing"));
        }
        // Queue commands name only a submission; keep them within this agent.
        if let Some(submission) = op.submission()
            && !current
                .submissions
                .iter()
                .any(|s| s.id == submission && s.session_id == agent)
        {
            return Err(Error::invalid("Message is not in this agent's queue"));
        }
        let active = current
            .sessions
            .iter()
            .find(|s| s.id == agent)
            .and_then(|s| s.worker.as_ref())
            .is_some_and(|w| runtime::active(&w.status));
        // Read under the lock: the turn interrupted is the one running now.
        let active_run_id = if active { store.run_id(&agent)? } else { None };
        let summary = op.summary(&agent);
        let store = &mut *store;
        let transaction = store
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::internal)?;
        let mut saved = None;
        let command = match op {
            Op::Resume => Command::ResumeQueue {
                session_id: agent.clone(),
                queue: None,
            },
            Op::Cancel(submission_id) => Command::CancelTurn { submission_id },
            Op::Promote(submission_id) => Command::PromoteTurn {
                submission_id,
                active_run_id,
            },
            Op::Edit(submission_id, text) => {
                // The edit validates a saved draft. Stage the text only within
                // this transaction, then restore the human's draft unchanged.
                saved = Some(
                    conversation::read_drafts(&transaction)?
                        .into_iter()
                        .find(|d| d.session_id == agent),
                );
                let revision = saved
                    .as_ref()
                    .and_then(Option::as_ref)
                    .map_or(0, |d| d.revision);
                let parts = vec![Part::text(&text)];
                stage_draft(
                    &transaction,
                    &Draft {
                        session_id: agent.clone(),
                        revision,
                        parts: parts.clone(),
                    },
                )?;
                Command::EditQueuedTurn {
                    submission_id,
                    draft_revision: revision,
                    parts,
                    approve_implementation: decided.implement == Permission::Ask,
                }
            }
        };
        crate::notices::command(&transaction, &current, &command)?;
        let action = conversation::apply(
            &transaction,
            &mut current,
            &id,
            command.clone(),
            &workspace.config,
            &store.controls,
            Sender::Agent {
                session_id: caller.into(),
            },
        )?;
        match saved {
            Some(Some(draft)) => stage_draft(&transaction, &draft)?,
            Some(None) => {
                transaction
                    .execute("DELETE FROM drafts WHERE session_id=?1", [&agent])
                    .map_err(Error::internal)?;
            }
            None => {}
        }
        if let Some(Action::Run { session_id, .. }) = &action {
            transaction
                .execute(
                    "DELETE FROM lifecycle_holds WHERE session_id=?1",
                    [session_id],
                )
                .map_err(Error::internal)?;
        }
        current.messages.push(Message {
            id: format!("agent-queue-{id}"),
            session_id: caller.into(),
            author: "Relay".into(),
            sender: Sender::User,
            kind: "agent_control".into(),
            body: summary,
            parts: vec![],
            tool: None,
        });
        current.revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        let envelope = CommandEnvelope {
            request_id: id.clone(),
            expected_revision: current.revision,
            command,
        };
        transaction
            .execute(
                "INSERT INTO receipts(request_id,request) VALUES(?1,?2)",
                params![
                    id,
                    serde_json::to_string(&envelope).map_err(Error::internal)?
                ],
            )
            .map_err(Error::internal)?;
        crate::notices::superseded(&transaction)?;
        let current = crate::persist(
            &store.snapshot_cache,
            &store.transcripts,
            &transaction,
            current,
        )?;
        transaction.commit().map_err(Error::internal)?;
        let launch = match &action {
            Some(Action::Run { session_id, .. }) => {
                let run_id = store
                    .run_id(session_id)?
                    .ok_or_else(|| Error::invalid("Run identity missing"))?;
                let (sender, receiver) = watch::channel(false);
                store.controls.insert(session_id.clone(), sender);
                Some((run_id, receiver))
            }
            Some(Action::Promote(session_id)) => {
                if let Some(control) = store.controls.get(session_id) {
                    control.send_replace(true);
                }
                None
            }
            _ => None,
        };
        workspace.snapshots.send_replace(current.clone());
        (action, launch, current)
    };
    let started = matches!(action, Some(Action::Run { .. }));
    let interrupting = matches!(action, Some(Action::Promote(_)));
    if let (Some(Action::Run { session_id, prompt }), Some((run_id, receiver))) = (action, launch) {
        tokio::spawn(runtime::run(
            workspace.clone(),
            session_id,
            run_id,
            prompt,
            receiver,
        ));
    }
    let mut result = view(&current, &agent);
    result["started"] = json!(started);
    result["interrupting_current_turn"] = json!(interrupting);
    Ok(result)
}

fn stage_draft(transaction: &rusqlite::Transaction, draft: &Draft) -> Result<(), Error> {
    transaction
        .execute(
            "INSERT OR REPLACE INTO drafts(session_id,draft) VALUES(?1,?2)",
            params![
                draft.session_id,
                serde_json::to_string(draft).map_err(Error::internal)?
            ],
        )
        .map_err(Error::internal)?;
    Ok(())
}

/// Tool specifications, in the shape of `tools()`.
pub(super) fn specs() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>)> {
    let string = json!({"type":"string"});
    vec![
        (
            "view_agent_queue",
            "Show one of your descendants' queued turns in order: submission id, state (queued, paused, launching, running), sender, text, pause reason, and whether the queue is paused. A paused queue (for example after a server restart or a stop) never starts by itself; message_agent only queues behind it.",
            json!({"agent":string}),
            vec!["agent"],
        ),
        (
            "resume_agent_queue",
            "Resume a descendant's paused queue, like the human Resume queue control: paused turns become queued and, when the agent is idle, the oldest starts now. Requires delegation permission.",
            json!({"agent":string}),
            vec!["agent"],
        ),
        (
            "send_queued_message_now",
            "Start one queued or paused turn of a descendant now, ahead of the rest. If the agent is running, its current turn is interrupted and this turn follows it. Requires delegation permission.",
            json!({"agent":string,"submission":string}),
            vec!["agent", "submission"],
        ),
        (
            "edit_queued_message",
            "Replace the text of a descendant's queued or paused turn; it keeps its place and state. Requires delegation permission and the target's implementation policy, as message_agent does.",
            json!({"agent":string,"submission":string,"text":string}),
            vec!["agent", "submission", "text"],
        ),
        (
            "cancel_queued_message",
            "Cancel a descendant's queued or paused turn so it never runs. Requires delegation permission.",
            json!({"agent":string,"submission":string}),
            vec!["agent", "submission"],
        ),
    ]
}
