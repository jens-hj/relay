//! Counts work, rather than asserting a machine-dependent latency threshold.
#[path = "../src/snapshot_cache.rs"]
mod snapshot_cache;
use relay_core::{AgentProfile, Snapshot, demo_snapshot};
use rusqlite::{Connection, params};
use std::{cell::Cell, sync::Arc};

fn write(connection: &Connection, snapshot: &Snapshot) {
    connection
        .execute(
            "UPDATE workspace SET snapshot=?1 WHERE id=1",
            [serde_json::to_string(snapshot).unwrap()],
        )
        .unwrap();
}

fn open(path: &std::path::Path, snapshot: &Snapshot) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE workspace(id INTEGER PRIMARY KEY,snapshot TEXT NOT NULL);
             CREATE TABLE other(id INTEGER PRIMARY KEY,value TEXT NOT NULL);",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO workspace VALUES(1,?1)",
            params![serde_json::to_string(snapshot).unwrap()],
        )
        .unwrap();
    connection
}

/// Reads through the cache and returns the value and whether it reloaded.
fn read(cache: &snapshot_cache::SnapshotCache, connection: &Connection) -> (Snapshot, bool) {
    let loaded = Cell::new(false);
    let snapshot = cache
        .read(connection, |phase, _, _| {
            if phase == "load" {
                loaded.set(true);
            }
        })
        .unwrap();
    (Snapshot::clone(&snapshot), loaded.get())
}

#[test]
fn repeated_reads_load_once_and_workspace_writers_invalidate() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite");
    let mut snapshot = demo_snapshot(AgentProfile::default());
    let mut connection = open(&path, &snapshot);
    let cache = snapshot_cache::SnapshotCache::attach(&connection);
    let mut loads = 0;
    for _ in 0..500 {
        let (actual, loaded) = read(&cache, &connection);
        loads += usize::from(loaded);
        assert_eq!(actual, snapshot);
    }
    assert_eq!(
        loads, 1,
        "500 unchanged reads must not perform 500 JSON loads/deserializations"
    );
    snapshot.revision += 1;
    write(&connection, &snapshot);
    assert_eq!(read(&cache, &connection), (snapshot.clone(), true));
    let external = Connection::open(&path).unwrap();
    snapshot.revision += 1;
    write(&external, &snapshot);
    assert_eq!(read(&cache, &connection), (snapshot.clone(), true));
    {
        let transaction = connection.transaction().unwrap();
        let mut uncommitted = snapshot.clone();
        uncommitted.revision += 1;
        write(&transaction, &uncommitted);
        assert_eq!(read(&cache, &transaction).0, uncommitted);
        transaction.rollback().unwrap();
    }
    assert_eq!(
        read(&cache, &connection).0,
        snapshot,
        "a transactional read must never survive rollback in the cache"
    );
}

#[test]
fn writes_to_other_tables_and_transactions_keep_the_cache() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite");
    let snapshot = demo_snapshot(AgentProfile::default());
    let mut connection = open(&path, &snapshot);
    let cache = snapshot_cache::SnapshotCache::attach(&connection);
    assert!(read(&cache, &connection).1);
    for value in 0..100 {
        connection
            .execute("INSERT INTO other(value) VALUES(?1)", [value.to_string()])
            .unwrap();
        assert_eq!(read(&cache, &connection), (snapshot.clone(), false));
    }
    let transaction = connection.transaction().unwrap();
    transaction
        .execute("INSERT INTO other(value) VALUES('t')", [])
        .unwrap();
    assert_eq!(
        read(&cache, &transaction),
        (snapshot.clone(), false),
        "a transaction that has not written the workspace reuses the cache"
    );
    transaction.commit().unwrap();
    assert_eq!(read(&cache, &connection), (snapshot, false));
}

#[test]
fn own_writes_are_adopted_on_commit_and_dropped_on_rollback() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite");
    let mut snapshot = demo_snapshot(AgentProfile::default());
    let mut connection = open(&path, &snapshot);
    let cache = snapshot_cache::SnapshotCache::attach(&connection);
    assert!(read(&cache, &connection).1);

    // Autocommit write.
    snapshot.revision += 1;
    cache
        .write(&connection, Arc::new(snapshot.clone()), |_, _, _| {})
        .unwrap();
    assert_eq!(read(&cache, &connection), (snapshot.clone(), false));

    // Committed transactional write; visible inside the transaction too.
    snapshot.revision += 1;
    let transaction = connection.transaction().unwrap();
    cache
        .write(&transaction, Arc::new(snapshot.clone()), |_, _, _| {})
        .unwrap();
    assert_eq!(read(&cache, &transaction), (snapshot.clone(), false));
    transaction.commit().unwrap();
    assert_eq!(read(&cache, &connection), (snapshot.clone(), false));

    // Rolled-back write is never cached.
    let transaction = connection.transaction().unwrap();
    let mut uncommitted = snapshot.clone();
    uncommitted.revision += 1;
    cache
        .write(&transaction, Arc::new(uncommitted.clone()), |_, _, _| {})
        .unwrap();
    assert_eq!(read(&cache, &transaction).0, uncommitted);
    drop(transaction);
    assert_eq!(read(&cache, &connection), (snapshot.clone(), false));

    // The stored row matches what the cache serves.
    let fresh = snapshot_cache::SnapshotCache::default();
    assert_eq!(read(&fresh, &connection), (snapshot.clone(), true));

    // A commit by another connection after an own write still invalidates.
    let external = Connection::open(&path).unwrap();
    snapshot.revision += 1;
    write(&external, &snapshot);
    assert_eq!(read(&cache, &connection), (snapshot, true));
}

#[test]
fn staged_values_are_adopted_on_commit_and_loads_respect_the_revision_floor() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite");
    let mut snapshot = demo_snapshot(AgentProfile::default());
    let mut connection = open(&path, &snapshot);
    connection
        .execute_batch(
            "CREATE TABLE floor(revision INTEGER NOT NULL); INSERT INTO floor VALUES(0);",
        )
        .unwrap();
    let cache = snapshot_cache::SnapshotCache::attach(&connection)
        .revision_floor("SELECT revision FROM floor");
    assert!(read(&cache, &connection).1);

    // A change stored elsewhere (here: the floor) is staged, not written.
    snapshot.revision += 5;
    let transaction = connection.transaction().unwrap();
    transaction
        .execute("UPDATE floor SET revision=?1", [snapshot.revision])
        .unwrap();
    cache
        .stage(&transaction, Arc::new(snapshot.clone()))
        .unwrap();
    transaction.commit().unwrap();
    assert_eq!(read(&cache, &connection), (snapshot.clone(), false));

    // A reload of the older row still reports the newer revision.
    let fresh = snapshot_cache::SnapshotCache::attach(&Connection::open(&path).unwrap())
        .revision_floor("SELECT revision FROM floor");
    let (loaded, reloaded) = read(&fresh, &connection);
    assert!(reloaded);
    assert_eq!(loaded.revision, snapshot.revision);

    // A rolled-back stage is discarded.
    let transaction = connection.transaction().unwrap();
    let mut staged = snapshot.clone();
    staged.revision += 1;
    cache.stage(&transaction, Arc::new(staged)).unwrap();
    drop(transaction);
    assert_eq!(read(&cache, &connection), (snapshot, false));
}
