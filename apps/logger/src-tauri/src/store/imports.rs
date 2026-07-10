//! store/imports.rs — persistence for imported roasts (CONTRACTS.md §5 + §7.1).
//!
//! Owner: importer. `alog::import` parses files into `ImportedRoast` values (the
//! struct is defined in `crate::alog`, since this store submodule is private and
//! the parser must be able to name it) and commits each one through
//! `Store::insert_imported_roast`.
//!
//! Rebase invariant (CONTRACTS.md §5): the parser produces `ImportedRoast`
//! samples already REBASED so `session_sec = t + |min(t, 0)|` — charge pins the
//! rebase exactly like live capture; `events` carry session_sec on the same axis
//! (a `charge` event pins `charge_session_sec`). This insert writes
//! `status='finished'`, the `source_id`/`imported_from` pair, this install's
//! `device_id`, canonical markers recomputed from the events, an auto-detected
//! turning point, and an `import` outbox row.

use rusqlite::{params, Connection};

use crate::alog::ImportedRoast;
use crate::error::LoggerError;
use crate::model::GetRoastResult;

use super::{ops, Store};

impl Store {
    /// Persist one imported roast; returns the minted roast uuid (UUIDv7).
    ///
    /// Runs on the writer thread (like every other store op): all of the roast +
    /// samples + events go in through one transaction, canonical markers are
    /// recomputed from the events exactly as a live finish would, the turning
    /// point is auto-detected from the curve, an `import` outbox row is enqueued,
    /// and the WAL is checkpoint-truncated.
    pub fn insert_imported_roast(&self, roast: &ImportedRoast) -> Result<String, LoggerError> {
        let roast = roast.clone();
        self.with_conn(move |conn| insert_imported_roast(conn, &roast))
    }
}

fn insert_imported_roast(
    conn: &mut Connection,
    roast: &ImportedRoast,
) -> Result<String, LoggerError> {
    let uuid = uuid::Uuid::now_v7().to_string();
    let device_id = if roast.device_id.trim().is_empty() {
        ops::ensure_device_id(conn)?
    } else {
        roast.device_id.clone()
    };

    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO roasts (uuid, device_id, source_id, status, started_wall_ms,
                             coffee_name, charge_weight_lb, drop_weight_lb, notes, imported_from)
         VALUES (?1, ?2, ?3, 'finished', ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            uuid,
            device_id,
            roast.source_id,
            roast.started_wall_ms,
            roast.coffee_name,
            roast.charge_weight_lb,
            roast.drop_weight_lb,
            roast.notes,
            roast.imported_from,
        ],
    )?;

    {
        let mut stmt = tx.prepare(
            "INSERT INTO samples (roast_uuid, seq, session_sec, bt_f, et_f, ambient_f, heater, fan, drum)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for s in &roast.samples {
            stmt.execute(params![
                uuid,
                s.seq as i64,
                s.session_sec,
                s.bt_f,
                s.et_f,
                s.ambient_f,
                s.heater,
                s.fan,
                s.drum,
            ])?;
        }
    }

    // Marker taps. created_ms strictly increases so canonicalize's "latest tap of
    // each kind" ordering is deterministic even within the same millisecond.
    let base_ms = ops::now_ms();
    for (i, e) in roast.events.iter().enumerate() {
        tx.execute(
            "INSERT INTO events (roast_uuid, kind, session_sec, note, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                uuid,
                e.kind.as_str(),
                e.session_sec,
                e.note,
                base_ms + i as i64
            ],
        )?;
    }

    // Canonical markers + auto turning point — parity with a live finish.
    ops::canonicalize_markers(&tx, &uuid)?;
    auto_turning_point(&tx, &uuid)?;

    // `import` outbox row (flushed in Phase 2b), payload identical to `finalize`.
    let payload = GetRoastResult {
        summary: ops::load_summary(&tx, &uuid)?.ok_or_else(|| LoggerError::no_such_roast(&uuid))?,
        samples: ops::load_samples(&tx, &uuid)?,
        events: ops::load_events(&tx, &uuid)?,
    };
    let bytes = serde_json::to_vec(&payload)
        .map_err(|e| LoggerError::db(format!("outbox payload: {e}")))?;
    tx.execute(
        "INSERT INTO outbox (roast_uuid, op, chunk_index, payload_version, payload, created_ms)
         VALUES (?1, 'import', NULL, 1, ?2, ?3)",
        params![uuid, bytes, ops::now_ms()],
    )?;

    tx.commit()?;
    super::schema::checkpoint_truncate(conn);
    Ok(uuid)
}

/// Detect the turning point from the persisted curve and pin it when unmarked —
/// same rule the live finish path uses (parity with roast-console/math). Kept
/// local to this module because `ops::auto_turning_point` is private there.
fn auto_turning_point(conn: &Connection, roast_uuid: &str) -> Result<(), LoggerError> {
    let existing: Option<f64> = conn.query_row(
        "SELECT turning_point_sec FROM roasts WHERE uuid = ?1",
        params![roast_uuid],
        |row| row.get(0),
    )?;
    if existing.is_some() {
        return Ok(());
    }
    let charge: Option<f64> = conn.query_row(
        "SELECT charge_session_sec FROM roasts WHERE uuid = ?1",
        params![roast_uuid],
        |row| row.get(0),
    )?;
    let origin = charge.unwrap_or(0.0);

    let mut stmt =
        conn.prepare("SELECT session_sec, bt_f FROM samples WHERE roast_uuid = ?1 ORDER BY seq")?;
    let points: Vec<(f64, f64)> = stmt
        .query_map(params![roast_uuid], |row| {
            Ok((row.get::<_, f64>(0)? - origin, row.get::<_, f64>(1)?))
        })?
        .collect::<Result<_, _>>()?;

    if let Some((t, bt)) = crate::math::detect_turning_point(&points) {
        conn.execute(
            "UPDATE roasts SET turning_point_sec = ?2, turning_point_temp_f = ?3 WHERE uuid = ?1",
            params![roast_uuid, t, bt],
        )?;
    }
    Ok(())
}
