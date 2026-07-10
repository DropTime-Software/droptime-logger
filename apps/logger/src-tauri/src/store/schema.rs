//! Schema + migrations ladder. `PRAGMA user_version` tracks the version; each
//! entry in `MIGRATIONS` moves the DB up one version, applied strictly in
//! order inside a transaction. Before applying any migration to an EXISTING
//! database (user_version >= 1), the DB file is backup-copied next to itself.

use std::path::Path;

use rusqlite::Connection;

use crate::error::LoggerError;

/// v1 — verbatim from CONTRACTS.md §1 (frozen).
const SCHEMA_V1: &str = r#"
CREATE TABLE roasts (
  uuid TEXT PRIMARY KEY,              -- UUIDv7 minted at session start
  device_id TEXT NOT NULL,            -- per-install UUID (settings)
  source_id TEXT NOT NULL,            -- SourceInfo.id that captured it
  status TEXT NOT NULL,               -- recording | finished | abandoned
  started_wall_ms INTEGER NOT NULL,
  machine_local_id INTEGER,
  coffee_name TEXT,
  charge_weight_lb REAL,
  drop_weight_lb REAL,
  charge_session_sec REAL,            -- set by the charge event
  charge_temp_f REAL,
  turning_point_sec REAL, turning_point_temp_f REAL,
  dry_end_sec REAL, fc_start_sec REAL, fc_end_sec REAL,
  drop_sec REAL, drop_temp_f REAL,    -- all *_sec are seconds-from-charge
  notes TEXT,
  synced_batch_id TEXT                -- Convex roastBatches id after sync (Phase 2b)
);
CREATE TABLE samples (
  roast_uuid TEXT NOT NULL REFERENCES roasts(uuid),
  seq INTEGER NOT NULL,
  session_sec REAL NOT NULL,
  bt_f REAL NOT NULL,
  et_f REAL, ambient_f REAL, heater REAL, fan REAL, drum REAL,
  PRIMARY KEY (roast_uuid, seq)
) WITHOUT ROWID;                      -- append-only, never rewritten
CREATE TABLE events (
  roast_uuid TEXT NOT NULL REFERENCES roasts(uuid),
  kind TEXT NOT NULL,                 -- RoastEventKind (types.ts)
  session_sec REAL NOT NULL,
  note TEXT,
  created_ms INTEGER NOT NULL
);                                    -- raw tap history; roasts.* markers are canonical
CREATE TABLE machines_local (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  make TEXT,
  source_pin TEXT                     -- JSON: preferred source/port/channel-roles
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE outbox (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  roast_uuid TEXT,
  op TEXT NOT NULL,                   -- start_live | chunk | live_patch | finalize | import
  chunk_index INTEGER,
  payload_version INTEGER NOT NULL DEFAULT 1,
  payload BLOB NOT NULL,
  created_ms INTEGER NOT NULL,
  synced_ms INTEGER
);                                    -- written in Phase 1, flushed in Phase 2b
CREATE INDEX idx_events_roast ON events(roast_uuid);
CREATE INDEX idx_outbox_pending ON outbox(synced_ms) WHERE synced_ms IS NULL;
"#;

/// v2 — verbatim from CONTRACTS.md §5 (frozen).
const SCHEMA_V2: &str = r#"
ALTER TABLE roasts ADD COLUMN coffee_local_id INTEGER;      -- optional link to coffees_local
ALTER TABLE roasts ADD COLUMN reference_roast_uuid TEXT;    -- background-replay reference used
ALTER TABLE roasts ADD COLUMN imported_from TEXT;           -- 'alog' | 'csv' when imported
ALTER TABLE machines_local ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
CREATE TABLE coffees_local (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  origin TEXT,
  process TEXT,
  notes TEXT,
  archived INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_roasts_started ON roasts(started_wall_ms DESC);
"#;

/// Strictly-ordered ladder: `MIGRATIONS[n]` migrates user_version n → n+1.
const MIGRATIONS: &[&str] = &[SCHEMA_V1, SCHEMA_V2];

pub const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;

fn user_version(conn: &Connection) -> Result<i64, LoggerError> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// Bring `conn` up to `LATEST_VERSION`, backup-copying the DB file first when
/// migrating an existing database (never on fresh create).
pub fn migrate(conn: &mut Connection, db_path: &Path) -> Result<(), LoggerError> {
    let current = user_version(conn)?;
    if current == LATEST_VERSION {
        return Ok(());
    }
    if current > LATEST_VERSION {
        return Err(LoggerError::db(format!(
            "database is version {current} but this build only knows {LATEST_VERSION}; \
             refusing to open (newer app wrote it?)"
        )));
    }

    if current >= 1 {
        backup_before_migration(conn, db_path, current)?;
    }

    for (idx, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let step = idx as i64 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", step)?;
        tx.commit()?;
        tracing::info!(version = step, "applied schema migration");
    }
    Ok(())
}

/// Copy the DB file aside before touching an existing database. WAL is
/// checkpoint-truncated first so the single-file copy is complete.
fn backup_before_migration(
    conn: &Connection,
    db_path: &Path,
    from: i64,
) -> Result<(), LoggerError> {
    checkpoint_truncate(conn);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let backup = db_path.with_extension(format!("v{from}.{stamp}.backup.db"));
    std::fs::copy(db_path, &backup)?;
    tracing::info!(backup = %backup.display(), "backed up database before migration");
    Ok(())
}

/// `PRAGMA wal_checkpoint(TRUNCATE)` — best-effort; failure is logged, not fatal.
pub fn checkpoint_truncate(conn: &Connection) {
    let res: Result<(i64, i64, i64), rusqlite::Error> =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        });
    match res {
        Ok((busy, _log, _ckpt)) if busy != 0 => {
            tracing::warn!("wal_checkpoint(TRUNCATE) reported busy; WAL not fully truncated");
        }
        Ok(_) => {}
        Err(err) => tracing::warn!(error = %err, "wal_checkpoint(TRUNCATE) failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db_path;

    fn open(path: &std::path::Path) -> Connection {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        Connection::open(path).unwrap()
    }

    fn version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_db_lands_at_v2_with_new_columns_usable() {
        let path = test_db_path("schema-fresh");
        let mut conn = open(&path);
        migrate(&mut conn, &path).unwrap();
        assert_eq!(LATEST_VERSION, 2);
        assert_eq!(version(&conn), 2);

        // v2 tables/columns are live: coffees_local + the three roasts columns
        // + machines_local.archived + the started_wall_ms index.
        conn.execute(
            "INSERT INTO coffees_local (name, origin, process) VALUES ('Guji', 'Ethiopia', 'washed')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO roasts (uuid, device_id, source_id, status, started_wall_ms,
                                 coffee_local_id, reference_roast_uuid, imported_from)
             VALUES ('u1', 'd', 'import:alog', 'finished', 1, 1, 'ref-uuid', 'alog')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO machines_local (name, archived) VALUES ('SF-25', 1)",
            [],
        )
        .unwrap();
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_roasts_started'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn v1_db_migrates_cleanly_to_v2() {
        let path = test_db_path("schema-v1-up");
        {
            // Build a genuine v1 database with existing rows.
            let conn = open(&path);
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO roasts (uuid, device_id, source_id, status, started_wall_ms)
                 VALUES ('u1', 'd', 'replay:x', 'finished', 42)",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO machines_local (name) VALUES ('Aillio')", [])
                .unwrap();
        }

        let mut conn = open(&path);
        migrate(&mut conn, &path).unwrap();
        assert_eq!(version(&conn), 2);

        // Pre-existing rows picked up the new defaults; new columns writable.
        let archived: i64 = conn
            .query_row(
                "SELECT archived FROM machines_local WHERE name = 'Aillio'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(archived, 0);
        conn.execute(
            "UPDATE roasts SET imported_from = 'csv', coffee_local_id = 7,
                               reference_roast_uuid = 'ref-2' WHERE uuid = 'u1'",
            [],
        )
        .unwrap();
        let (from, cid, reference): (Option<String>, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT imported_from, coffee_local_id, reference_roast_uuid FROM roasts WHERE uuid = 'u1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(from.as_deref(), Some("csv"));
        assert_eq!(cid, Some(7));
        assert_eq!(reference.as_deref(), Some("ref-2"));

        // Migrating an EXISTING db must have taken a backup copy first.
        let has_backup = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("backup"));
        assert!(has_backup, "v1→v2 migration must back up the DB file first");

        // Migration is idempotent at the latest version.
        migrate(&mut conn, &path).unwrap();
        assert_eq!(version(&conn), 2);
    }
}
