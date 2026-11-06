//! A read cache, never a second writer. SQLite changes invalidate it, including
//! writes on this connection outside Store::save and commits by other connections.
use relay_core::Snapshot;
use rusqlite::Connection;
use std::{cell::RefCell, time::Instant};

#[derive(Default)]
pub(super) struct SnapshotCache(RefCell<Option<Entry>>);

struct Entry {
    changes: u64,
    data_version: i64,
    snapshot: Snapshot,
}

impl SnapshotCache {
    pub(super) fn read(
        &self,
        connection: &Connection,
        mut phase: impl FnMut(&'static str, Instant, usize),
    ) -> Result<Snapshot, Box<dyn std::error::Error + Send + Sync>> {
        let changes = connection.total_changes();
        let data_version = connection.pragma_query_value(None, "data_version", |r| r.get(0))?;
        let transactional = !connection.is_autocommit();
        let mut cached = self.0.borrow_mut();
        // A rolled-back write still increments total_changes. Never install
        // uncommitted state in a cache that might outlive that transaction.
        if transactional
            || cached
                .as_ref()
                .is_none_or(|c| c.changes != changes || c.data_version != data_version)
        {
            let started = Instant::now();
            let json: String =
                connection.query_row("SELECT snapshot FROM workspace WHERE id=1", [], |r| {
                    r.get(0)
                })?;
            phase("load", started, json.len());
            let started = Instant::now();
            let snapshot = serde_json::from_str(&json)?;
            phase("deserialize", started, json.len());
            if transactional {
                return Ok(snapshot);
            }
            *cached = Some(Entry {
                changes,
                data_version,
                snapshot,
            });
        }
        let started = Instant::now();
        let result = cached.as_ref().unwrap().snapshot.clone();
        phase("clone", started, 0);
        Ok(result)
    }

    /// update_run holds the Store mutex from read through commit. Compare with
    /// that read's cached original without another whole-history deep clone.
    pub(super) fn unchanged(&self, snapshot: &Snapshot) -> bool {
        self.0
            .borrow()
            .as_ref()
            .is_some_and(|c| c.snapshot == *snapshot)
    }
}
