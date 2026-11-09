//! Transcript storage.
//!
//! Messages live in their own table, one row per message, instead of inside the
//! workspace snapshot. The stored and published `Snapshot` always has an empty
//! `messages` list; readers query the rows they need and writers upsert only
//! the rows they changed.
//!
//! `seq` preserves the original transcript order: an upsert of an existing id
//! keeps its position and a new id is appended, matching `Vec::push` semantics.
//! `revision` is the workspace revision at which the row last changed, so a
//! revision bump that only touched messages survives a restart.
use relay_core::{Message, ToolState};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS messages(
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    run_id TEXT,
    running INTEGER NOT NULL DEFAULT 0,
    revision INTEGER NOT NULL,
    data TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_session ON messages(session_id, seq);
CREATE INDEX IF NOT EXISTS messages_run ON messages(session_id, run_id);
CREATE INDEX IF NOT EXISTS messages_running ON messages(running) WHERE running = 1;
CREATE INDEX IF NOT EXISTS messages_revision ON messages(revision);
";

type Result<T> = std::result::Result<T, rusqlite::Error>;

fn decode(json: String) -> Result<Message> {
    serde_json::from_str(&json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn encode(message: &Message) -> Result<String> {
    serde_json::to_string(message)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

/// The run a message belongs to: its tool call's run, else the run that wrote it.
fn run_of<'a>(message: &'a Message, run: Option<&'a str>) -> Option<&'a str> {
    message
        .tool
        .as_ref()
        .map(|tool| tool.run_id.as_str())
        .filter(|id| !id.is_empty())
        .or(run)
}

fn running(message: &Message) -> bool {
    message
        .tool
        .as_ref()
        .is_some_and(|tool| tool.state == ToolState::Running)
}

/// Inserts new messages at the end of the transcript and updates existing ones
/// in place. `run` attributes rows written while a run was active.
pub(crate) fn upsert<'a>(
    connection: &Connection,
    messages: impl IntoIterator<Item = &'a Message>,
    run: Option<&str>,
    revision: u64,
) -> Result<()> {
    let mut statement = connection.prepare_cached(
        "INSERT INTO messages(id,session_id,kind,run_id,running,revision,data)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(id) DO UPDATE SET
            session_id=excluded.session_id,
            kind=excluded.kind,
            run_id=COALESCE(messages.run_id,excluded.run_id),
            running=excluded.running,
            revision=excluded.revision,
            data=excluded.data",
    )?;
    let revision = i64::try_from(revision).unwrap_or(i64::MAX);
    for message in messages {
        statement.execute(params![
            message.id,
            message.session_id,
            message.kind,
            run_of(message, run),
            running(message),
            revision,
            encode(message)?,
        ])?;
    }
    Ok(())
}

pub(crate) fn get(connection: &Connection, id: &str) -> Result<Option<Message>> {
    connection
        .prepare_cached("SELECT data FROM messages WHERE id=?1")?
        .query_row([id], |r| r.get(0))
        .optional()?
        .map(decode)
        .transpose()
}

fn collect(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Message>> {
    let mut statement = connection.prepare_cached(sql)?;
    let rows = statement.query_map(params, |r| r.get::<_, String>(0))?;
    rows.map(|row| row.and_then(decode)).collect()
}

/// Every message of `session`, in transcript order. Linear in the session's
/// history; use only for rare operations (fork, search, archive export).
pub(crate) fn session(connection: &Connection, session: &str) -> Result<Vec<Message>> {
    collect(
        connection,
        "SELECT data FROM messages WHERE session_id=?1 ORDER BY seq",
        [session],
    )
}

/// The messages a run can address by id: those attributed to it, the prompt
/// and issue context that started it, and the agent message that requested it
/// (stored in the sender's session, marked read when the run starts).
pub(crate) fn run(connection: &Connection, session: &str, run: &str) -> Result<Vec<Message>> {
    collect(
        connection,
        "SELECT data FROM messages WHERE (session_id=?1 AND (run_id=?2 OR id IN ('prompt-'||?2,'issue-context-'||?2))) OR id='agent-message-'||?2 ORDER BY seq",
        params![session, run],
    )
}

/// Messages with a running tool call, optionally limited to one session/run.
pub(crate) fn running_tools(
    connection: &Connection,
    session: Option<&str>,
    run: Option<&str>,
) -> Result<Vec<Message>> {
    collect(
        connection,
        "SELECT data FROM messages WHERE running=1 AND (?1 IS NULL OR session_id=?1) AND (?2 IS NULL OR run_id=?2) ORDER BY seq",
        params![session, run],
    )
}

/// The newest message of `kind` in `session` (optionally for one run) that
/// satisfies `matches`, scanning only rows of that kind.
pub(crate) fn newest_matching(
    connection: &Connection,
    session: &str,
    kind: &str,
    run: Option<&str>,
    matches: impl Fn(&Message) -> bool,
) -> Result<Option<Message>> {
    let mut statement = connection.prepare_cached(
        "SELECT data FROM messages WHERE session_id=?1 AND kind=?2 AND (?3 IS NULL OR run_id=?3) ORDER BY seq DESC",
    )?;
    let mut rows = statement.query(params![session, kind, run])?;
    while let Some(row) = rows.next()? {
        let message = decode(row.get(0)?)?;
        if matches(&message) {
            return Ok(Some(message));
        }
    }
    Ok(None)
}

/// The first message of `session` whose kind is one of `kinds`.
pub(crate) fn first_of(
    connection: &Connection,
    session: &str,
    kinds: &[&str],
) -> Result<Option<Message>> {
    let kinds = serde_json::to_string(kinds).unwrap_or_default();
    connection
        .prepare_cached(
            "SELECT data FROM messages WHERE session_id=?1 AND kind IN (SELECT value FROM json_each(?2)) ORDER BY seq LIMIT 1",
        )?
        .query_row(params![session, kinds], |r| r.get(0))
        .optional()?
        .map(decode)
        .transpose()
}

/// A compact, ordered index of one session's transcript: `(seq, id)` pairs.
pub(crate) fn index(connection: &Connection, session: &str) -> Result<Vec<(i64, String)>> {
    let mut statement = connection
        .prepare_cached("SELECT seq,id FROM messages WHERE session_id=?1 ORDER BY seq")?;
    let rows = statement.query_map([session], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// Loads the messages with the given ids (any order); missing ids are skipped.
pub(crate) fn by_ids<'a>(
    connection: &Connection,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<Message>> {
    let ids = serde_json::to_string(&ids.into_iter().collect::<Vec<_>>()).unwrap_or_default();
    collect(
        connection,
        "SELECT data FROM messages WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY seq",
        [ids],
    )
}

/// The id of the session's newest `prompt-…` message.
pub(crate) fn newest_prompt(connection: &Connection, session: &str) -> Result<Option<String>> {
    connection
        .prepare_cached(
            "SELECT id FROM messages WHERE session_id=?1 AND kind='prompt' AND id >= 'prompt-' AND id < 'prompt-' || char(1114111) ORDER BY seq DESC LIMIT 1",
        )?
        .query_row([session], |r| r.get(0))
        .optional()
}

/// Reasoning rows of a session whose id starts with `prefix`, in order.
pub(crate) fn with_prefix(
    connection: &Connection,
    session: &str,
    kind: &str,
    prefix: &str,
) -> Result<Vec<Message>> {
    // `>= prefix AND < prefix + U+10FFFF` is an index-friendly prefix match
    // that, unlike LIKE, treats `%` and `_` literally.
    collect(
        connection,
        "SELECT data FROM messages WHERE session_id=?1 AND kind=?2 AND id >= ?3 AND id < ?3 || char(1114111) ORDER BY seq",
        params![session, kind, prefix],
    )
}

pub(crate) fn delete_sessions<'a>(
    connection: &Connection,
    sessions: impl IntoIterator<Item = &'a str>,
) -> Result<usize> {
    let sessions =
        serde_json::to_string(&sessions.into_iter().collect::<Vec<_>>()).unwrap_or_default();
    connection.execute(
        "DELETE FROM messages WHERE session_id IN (SELECT value FROM json_each(?1))",
        [sessions],
    )
}

pub(crate) fn max_revision(connection: &Connection) -> Result<u64> {
    let revision: i64 =
        connection.query_row("SELECT COALESCE(MAX(revision),0) FROM messages", [], |r| {
            r.get(0)
        })?;
    Ok(u64::try_from(revision).unwrap_or(0))
}

/// One active run's addressable messages, kept between harness events.
pub(crate) struct RunSet {
    pub(crate) run: String,
    /// Transcript order, as the run's update closures see them.
    pub(crate) messages: Vec<Message>,
    /// The committed value of each message, to detect which ones changed.
    committed: std::collections::HashMap<String, Message>,
}

impl RunSet {
    /// Messages that differ from their committed value, in order.
    pub(crate) fn dirty(&self) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| self.committed.get(&m.id) != Some(*m))
            .collect()
    }
    /// Records the dirty messages as committed.
    pub(crate) fn committed(&mut self) {
        for message in &self.messages {
            if self.committed.get(&message.id) != Some(message) {
                self.committed.insert(message.id.clone(), message.clone());
            }
        }
    }
}

/// In-memory transcripts of active runs, keyed by session, so a harness event
/// neither reloads nor rewrites the session's history. Any write to a session's
/// messages outside its run update drops the session's entry.
#[derive(Default)]
pub(crate) struct Runs(std::cell::RefCell<std::collections::HashMap<String, RunSet>>);

impl Runs {
    /// Removes and returns `session`'s set for `run`, loading it if absent.
    pub(crate) fn take(&self, connection: &Connection, session: &str, run: &str) -> Result<RunSet> {
        if let Some(set) = self.0.borrow_mut().remove(session)
            && set.run == run
        {
            return Ok(set);
        }
        let messages = self::run(connection, session, run)?;
        let committed = messages.iter().map(|m| (m.id.clone(), m.clone())).collect();
        Ok(RunSet {
            run: run.into(),
            messages,
            committed,
        })
    }
    pub(crate) fn put(&self, session: &str, set: RunSet) {
        self.0.borrow_mut().insert(session.into(), set);
    }
    #[cfg(test)]
    pub(crate) fn clear(&self) {
        self.0.borrow_mut().clear();
    }
    /// How many messages the cached set for `session` holds (tests).
    #[cfg(test)]
    pub(crate) fn loaded(&self, session: &str) -> Option<usize> {
        self.0.borrow().get(session).map(|set| set.messages.len())
    }
    pub(crate) fn forget<'a>(&self, sessions: impl IntoIterator<Item = &'a str>) {
        let mut runs = self.0.borrow_mut();
        if runs.is_empty() {
            return;
        }
        for session in sessions {
            runs.remove(session);
        }
    }
}

/// Changes whenever any of `session`'s rows is inserted, updated or deleted:
/// `(row count, newest seq, newest revision)`.
pub(crate) fn session_version(connection: &Connection, session: &str) -> Result<(i64, i64, i64)> {
    connection
        .prepare_cached(
            "SELECT COUNT(*),COALESCE(MAX(seq),0),COALESCE(MAX(revision),0) FROM messages WHERE session_id=?1",
        )?
        .query_row([session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
}

/// Every stored message in transcript order. Linear in all history;
/// diagnostics and tests only.
pub(crate) fn all(connection: &Connection) -> Result<Vec<Message>> {
    collect(connection, "SELECT data FROM messages ORDER BY seq", [])
}
