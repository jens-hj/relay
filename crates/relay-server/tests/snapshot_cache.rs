//! Counts work, rather than asserting a machine-dependent latency threshold.
#[path = "../src/snapshot_cache.rs"]
mod snapshot_cache;
use relay_core::{AgentProfile, Snapshot, demo_snapshot};
use rusqlite::{Connection, params};

fn write(connection: &Connection, snapshot: &Snapshot) {
    connection
        .execute(
            "UPDATE workspace SET snapshot=?1 WHERE id=1",
            [serde_json::to_string(snapshot).unwrap()],
        )
        .unwrap();
}

#[test]
fn repeated_reads_load_once_and_all_sqlite_writers_invalidate() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite");
    let mut connection = Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE workspace(id INTEGER PRIMARY KEY,snapshot TEXT NOT NULL);").unwrap();
    let mut snapshot = demo_snapshot(AgentProfile::default());
    connection
        .execute(
            "INSERT INTO workspace VALUES(1,?1)",
            params![serde_json::to_string(&snapshot).unwrap()],
        )
        .unwrap();
    let cache = snapshot_cache::SnapshotCache::default();
    let mut loads = 0;
    for _ in 0..500 {
        let actual = cache
            .read(&connection, |phase, _, _| {
                if phase == "load" {
                    loads += 1;
                }
            })
            .unwrap();
        assert_eq!(actual, snapshot);
        assert!(cache.unchanged(&actual));
    }
    assert_eq!(
        loads, 1,
        "500 unchanged reads must not perform 500 JSON loads/deserializations"
    );
    snapshot.revision += 1;
    write(&connection, &snapshot);
    assert_eq!(cache.read(&connection, |_, _, _| {}).unwrap(), snapshot);
    let external = Connection::open(&path).unwrap();
    snapshot.revision += 1;
    write(&external, &snapshot);
    assert_eq!(cache.read(&connection, |_, _, _| {}).unwrap(), snapshot);
    {
        let transaction = connection.transaction().unwrap();
        let mut uncommitted = snapshot.clone();
        uncommitted.revision += 1;
        write(&transaction, &uncommitted);
        assert_eq!(cache.read(&transaction, |_, _, _| {}).unwrap(), uncommitted);
        transaction.rollback().unwrap();
    }
    assert_eq!(
        cache.read(&connection, |_, _, _| {}).unwrap(),
        snapshot,
        "a transactional read must never survive rollback in the cache"
    );
}
