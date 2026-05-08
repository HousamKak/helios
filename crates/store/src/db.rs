//! SQLite open + migration runner.
//!
//! The store owns a single `Connection` shared via `Arc<Mutex<...>>`.
//! All DB calls happen via `tokio::task::spawn_blocking` so they don't
//! starve the runtime. SQLite is in WAL mode, so concurrent readers
//! against this writer are safe at the file level — the Mutex is just
//! the simpler way to serialise within one process.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OpenFlags, params};

/// Shared, blocking handle to the connection.
pub type SharedDb = Arc<Mutex<Connection>>;

/// Open (or create) the database at `path`, run pending migrations,
/// return a shared handle plus the count of migrations newly applied.
pub fn open(path: &Path) -> anyhow::Result<(SharedDb, usize)> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).ok();
    }

    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

    let applied = run_migrations(&conn)?;
    Ok((Arc::new(Mutex::new(conn)), applied))
}

/// Apply any migrations from `helios_schema::migrations::MIGRATIONS`
/// that aren't already recorded. Idempotent — re-running is safe.
fn run_migrations(conn: &Connection) -> anyhow::Result<usize> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )?;

    let already: HashSet<String> = {
        let mut stmt = conn.prepare("SELECT name FROM _migrations")?;
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter().collect()
    };

    let mut count = 0usize;
    for migration in helios_schema::migrations::MIGRATIONS {
        if already.contains(migration.name) {
            continue;
        }
        conn.execute_batch(migration.sql)?;
        conn.execute(
            "INSERT INTO _migrations (name) VALUES (?1)",
            params![migration.name],
        )?;
        count += 1;
        tracing::info!(migration = migration.name, "applied migration");
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_db_and_applies_all_migrations() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let (_db, applied) = open(tmp.path()).unwrap();
        assert_eq!(applied, helios_schema::migrations::MIGRATIONS.len());
    }

    #[test]
    fn second_open_applies_zero_migrations() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let _ = open(tmp.path()).unwrap();
        let (_db, applied) = open(tmp.path()).unwrap();
        assert_eq!(applied, 0);
    }

    #[test]
    fn schema_tables_exist_after_open() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let (db, _) = open(tmp.path()).unwrap();
        let conn = db.lock().unwrap();
        for table in [
            "projects",
            "agent_instances",
            "tasks",
            "events",
            "memory_records",
            "processes",
            "files",
            "applets",
            "desktops",
            "canvas_entities",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "table {table} should exist");
        }
    }
}
