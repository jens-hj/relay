//! A read cache, never a second writer.
//!
//! The cache tracks the `workspace` row only. SQLite hooks on the owning
//! connection tell it exactly when that row changes, so writes to unrelated
//! tables (drafts, receipts, browser sessions, lifecycle rows, ...) no longer
//! force a full reload and re-parse of the snapshot. Commits by other
//! connections are still detected through `PRAGMA data_version`.
//!
//! Writes made through [`SnapshotCache::write`] are adopted on commit without
//! re-reading them. Any other write to the row (a raw `UPDATE workspace` on the
//! same connection) invalidates the cache, and reads inside that transaction
//! bypass it, so uncommitted state never outlives a rollback.
use relay_core::Snapshot;
use rusqlite::{Connection, hooks::Action};
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

type Phase<'a> = &'a mut dyn FnMut(&'static str, Instant, usize);

#[derive(Default)]
struct State {
    /// Committed workspace row and the `data_version` it was read or written at.
    entry: Option<(i64, Arc<Snapshot>)>,
    /// A [`SnapshotCache::write`] in the open transaction, adopted on commit.
    pending: Option<(i64, Arc<Snapshot>)>,
    /// Set while [`SnapshotCache::write`] runs its own `UPDATE`.
    expecting: bool,
    /// The open transaction changed the row outside [`SnapshotCache::write`].
    foreign: bool,
    /// Query for the minimum revision of a loaded row (see `revision_floor`).
    floor: Option<&'static str>,
}

#[derive(Default, Clone)]
pub(super) struct SnapshotCache(Arc<Mutex<State>>);

impl SnapshotCache {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Creates a cache bound to `connection`. Only one cache may be attached to
    /// a connection; reads through any other connection bypass nothing but are
    /// still validated with `data_version`.
    /// Loaded rows report at least the revision `sql` (a one-value query)
    /// returns, for revisions recorded outside the row by [`Self::stage`].
    pub(super) fn revision_floor(self, sql: &'static str) -> Self {
        self.state().floor = Some(sql);
        self
    }

    pub(super) fn attach(connection: &Connection) -> Self {
        let cache = Self::default();
        let state = cache.0.clone();
        connection.update_hook(Some(move |_: Action, _: &str, table: &str, _: i64| {
            if table == "workspace" {
                let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                if !state.expecting {
                    state.foreign = true;
                    state.pending = None;
                    state.entry = None;
                }
            }
        }));
        let state = cache.0.clone();
        connection.commit_hook(Some(move || {
            let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.foreign {
                state.entry = None;
            } else if let Some(pending) = state.pending.take() {
                state.entry = Some(pending);
            }
            state.pending = None;
            state.foreign = false;
            false
        }));
        let state = cache.0.clone();
        connection.rollback_hook(Some(move || {
            let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
            state.pending = None;
            state.foreign = false;
        }));
        cache
    }

    pub(super) fn read(
        &self,
        connection: &Connection,
        mut phase: impl FnMut(&'static str, Instant, usize),
    ) -> Result<Arc<Snapshot>, Box<dyn std::error::Error + Send + Sync>> {
        let data_version: i64 =
            connection.pragma_query_value(None, "data_version", |r| r.get(0))?;
        {
            let state = self.state();
            if !state.foreign {
                if let Some((_, snapshot)) = &state.pending {
                    return Ok(snapshot.clone());
                }
                if let Some((version, snapshot)) = &state.entry
                    && *version == data_version
                {
                    return Ok(snapshot.clone());
                }
            }
        }
        let mut snapshot = load(connection, &mut phase)?;
        let floor = self.state().floor;
        if let Some(sql) = floor {
            let floor: i64 = connection.query_row(sql, [], |r| r.get(0))?;
            snapshot.revision = snapshot.revision.max(u64::try_from(floor).unwrap_or(0));
        }
        let snapshot = Arc::new(snapshot);
        let mut state = self.state();
        // Never cache a row changed outside `write` in this transaction: it may
        // still roll back.
        if !state.foreign && state.pending.is_none() {
            state.entry = Some((data_version, snapshot.clone()));
        }
        Ok(snapshot)
    }

    /// Serializes and stores `snapshot` as the workspace row through
    /// `connection`. The cache adopts it when that write commits.
    pub(super) fn write(
        &self,
        connection: &Connection,
        snapshot: Arc<Snapshot>,
        mut phase: impl FnMut(&'static str, Instant, usize),
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let started = Instant::now();
        let json = serde_json::to_string(&*snapshot)?;
        let bytes = json.len();
        phase("serialize", started, bytes);
        let data_version: i64 =
            connection.pragma_query_value(None, "data_version", |r| r.get(0))?;
        {
            let mut state = self.state();
            state.expecting = true;
            if !state.foreign {
                state.pending = Some((data_version, snapshot));
            }
        }
        let started = Instant::now();
        let result = connection.execute("UPDATE workspace SET snapshot=?1 WHERE id=1", [json]);
        let mut state = self.state();
        state.expecting = false;
        match result {
            Ok(_) => {
                phase("write", started, bytes);
                Ok(())
            }
            Err(error) => {
                state.pending = None;
                Err(error.into())
            }
        }
    }

    /// Adopts `snapshot` on the next commit through `connection` without
    /// writing the row, for a change stored elsewhere (only its revision and
    /// other tables differ from the row; loading reconciles the revision).
    pub(super) fn stage(
        &self,
        connection: &Connection,
        snapshot: Arc<Snapshot>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data_version: i64 =
            connection.pragma_query_value(None, "data_version", |r| r.get(0))?;
        let mut state = self.state();
        if !state.foreign {
            state.pending = Some((data_version, snapshot));
        }
        Ok(())
    }
}

fn load(
    connection: &Connection,
    phase: Phase<'_>,
) -> Result<Snapshot, Box<dyn std::error::Error + Send + Sync>> {
    let started = Instant::now();
    let json: String =
        connection.query_row("SELECT snapshot FROM workspace WHERE id=1", [], |r| {
            r.get(0)
        })?;
    phase("load", started, json.len());
    let started = Instant::now();
    let snapshot = serde_json::from_str(&json)?;
    phase("deserialize", started, json.len());
    Ok(snapshot)
}
