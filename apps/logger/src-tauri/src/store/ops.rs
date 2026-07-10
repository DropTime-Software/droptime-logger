//! SQL operations. Everything here runs on the single writer thread and takes
//! a plain `&Connection`/`&mut Connection` — no locking, no async.
//!
//! Marker model: `events` is the raw tap history; the canonical markers live
//! as columns on `roasts` and are RECOMPUTED from the latest event of each
//! kind (`canonicalize_markers`). That makes re-taps and undo self-healing:
//! re-marking charge shifts every seconds-from-charge marker, undoing a marker
//! falls back to the previous tap of that kind if one exists.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::LoggerError;
use crate::math;
use crate::model::{
    EventDto, GetRoastResult, RecoveryDto, RoastEventKind, RoastMarkersDto, RoastStatus,
    RoastSummaryDto, SampleDto, SessionMetaDto,
};

use super::SampleRow;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn speed_setting_key(roast_uuid: &str) -> String {
    format!("session_speed:{roast_uuid}")
}

// ---------------------------------------------------------------------------
// settings / device id
// ---------------------------------------------------------------------------

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, LoggerError> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), LoggerError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete_setting(conn: &Connection, key: &str) -> Result<(), LoggerError> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

/// Mint the per-install device id (UUIDv4) into settings on first run.
pub fn ensure_device_id(conn: &Connection) -> Result<String, LoggerError> {
    if let Some(id) = get_setting(conn, "device_id")? {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    // INSERT OR IGNORE: harmless if two threads raced (they cannot — single writer).
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('device_id', ?1)",
        params![id],
    )?;
    get_setting(conn, "device_id")?.ok_or_else(|| LoggerError::db("device_id vanished after mint"))
}

// ---------------------------------------------------------------------------
// roasts / samples
// ---------------------------------------------------------------------------

pub fn create_roast(
    conn: &Connection,
    roast_uuid: &str,
    source_id: &str,
    started_wall_ms: i64,
    meta: &SessionMetaDto,
) -> Result<(), LoggerError> {
    let device_id = ensure_device_id(conn)?;
    conn.execute(
        "INSERT INTO roasts (uuid, device_id, source_id, status, started_wall_ms,
                             machine_local_id, coffee_name, charge_weight_lb,
                             coffee_local_id, reference_roast_uuid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            roast_uuid,
            device_id,
            source_id,
            RoastStatus::Recording.as_str(),
            started_wall_ms,
            meta.machine_local_id,
            meta.coffee_name,
            meta.charge_weight_lb,
            meta.coffee_local_id,
            meta.reference_roast_uuid,
        ],
    )?;
    Ok(())
}

pub fn insert_sample(conn: &Connection, row: &SampleRow) -> Result<(), LoggerError> {
    conn.execute(
        "INSERT INTO samples (roast_uuid, seq, session_sec, bt_f, et_f, ambient_f, heater, fan, drum)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.roast_uuid,
            row.sample.seq as i64,
            row.sample.session_sec,
            row.sample.bt_f,
            row.sample.et_f,
            row.sample.ambient_f,
            row.sample.heater,
            row.sample.fan,
            row.sample.drum,
        ],
    )?;
    Ok(())
}

/// `(status, source_id)` for a roast, or None.
pub fn roast_status(
    conn: &Connection,
    roast_uuid: &str,
) -> Result<Option<(RoastStatus, String)>, LoggerError> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT status, source_id FROM roasts WHERE uuid = ?1",
            params![roast_uuid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some((status, source_id)) => {
            let status = RoastStatus::parse(&status)
                .ok_or_else(|| LoggerError::db(format!("unknown roast status in db: {status}")))?;
            Ok(Some((status, source_id)))
        }
    }
}

/// Highest-seq persisted sample: `(seq, session_sec)`.
pub fn last_sample(conn: &Connection, roast_uuid: &str) -> Result<Option<(u64, f64)>, LoggerError> {
    let row: Option<(i64, f64)> = conn
        .query_row(
            "SELECT seq, session_sec FROM samples WHERE roast_uuid = ?1 ORDER BY seq DESC LIMIT 1",
            params![roast_uuid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row.map(|(seq, sec)| (seq as u64, sec)))
}

/// BT of the sample nearest (in sessionSec) to `session_sec`; earlier wins ties.
fn nearest_bt(
    conn: &Connection,
    roast_uuid: &str,
    session_sec: f64,
) -> Result<Option<f64>, LoggerError> {
    Ok(conn
        .query_row(
            "SELECT bt_f FROM samples WHERE roast_uuid = ?1
             ORDER BY ABS(session_sec - ?2), seq LIMIT 1",
            params![roast_uuid, session_sec],
            |row| row.get(0),
        )
        .optional()?)
}

// ---------------------------------------------------------------------------
// events + canonical markers
// ---------------------------------------------------------------------------

fn require_roast(conn: &Connection, roast_uuid: &str) -> Result<(), LoggerError> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM roasts WHERE uuid = ?1",
            params![roast_uuid],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(LoggerError::no_such_roast(roast_uuid));
    }
    Ok(())
}

/// Latest event of each kind (tap order = created_ms, then insert order).
fn latest_events(
    conn: &Connection,
    roast_uuid: &str,
) -> Result<HashMap<RoastEventKind, f64>, LoggerError> {
    let mut stmt = conn.prepare(
        "SELECT kind, session_sec FROM events WHERE roast_uuid = ?1 ORDER BY created_ms, rowid",
    )?;
    let rows = stmt.query_map(params![roast_uuid], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;
    let mut latest = HashMap::new();
    for row in rows {
        let (kind, sec) = row?;
        if let Some(kind) = RoastEventKind::parse(&kind) {
            latest.insert(kind, sec);
        } else {
            tracing::warn!(kind, "ignoring unknown event kind in db");
        }
    }
    Ok(latest)
}

/// Recompute the canonical marker columns on `roasts` from the event history.
///
/// charge → `charge_session_sec` + `charge_temp_f` (nearest sample BT); every
/// other timed marker → seconds-from-charge (charge defaults to 0 until it is
/// marked, then everything re-derives). turning_point/drop also pin their temp
/// from the nearest sample.
pub fn canonicalize_markers(
    conn: &Connection,
    roast_uuid: &str,
) -> Result<RoastMarkersDto, LoggerError> {
    let latest = latest_events(conn, roast_uuid)?;

    let charge_session_sec = latest.get(&RoastEventKind::Charge).copied();
    let charge_origin = charge_session_sec.unwrap_or(0.0);
    let charge_temp_f = match charge_session_sec {
        Some(sec) => nearest_bt(conn, roast_uuid, sec)?,
        None => None,
    };

    let rel = |kind: RoastEventKind| latest.get(&kind).map(|sec| sec - charge_origin);
    let temp_at = |kind: RoastEventKind| -> Result<Option<f64>, LoggerError> {
        match latest.get(&kind) {
            Some(sec) => nearest_bt(conn, roast_uuid, *sec),
            None => Ok(None),
        }
    };

    let markers = RoastMarkersDto {
        turning_point_sec: rel(RoastEventKind::TurningPoint),
        turning_point_temp_f: temp_at(RoastEventKind::TurningPoint)?,
        dry_end_sec: rel(RoastEventKind::DryEnd),
        fc_start_sec: rel(RoastEventKind::FcStart),
        fc_end_sec: rel(RoastEventKind::FcEnd),
        drop_sec: rel(RoastEventKind::Drop),
        drop_temp_f: temp_at(RoastEventKind::Drop)?,
        charge_temp_f,
    };

    conn.execute(
        "UPDATE roasts SET
           charge_session_sec = ?2, charge_temp_f = ?3,
           turning_point_sec = ?4, turning_point_temp_f = ?5,
           dry_end_sec = ?6, fc_start_sec = ?7, fc_end_sec = ?8,
           drop_sec = ?9, drop_temp_f = ?10
         WHERE uuid = ?1",
        params![
            roast_uuid,
            charge_session_sec,
            markers.charge_temp_f,
            markers.turning_point_sec,
            markers.turning_point_temp_f,
            markers.dry_end_sec,
            markers.fc_start_sec,
            markers.fc_end_sec,
            markers.drop_sec,
            markers.drop_temp_f,
        ],
    )?;
    Ok(markers)
}

/// Marker taps are live-session operations: on `finished` roasts markers are
/// edited via `update_roast_markers` (library.rs), which deliberately never
/// re-canonicalizes — a re-canonicalize would NULL an auto-detected turning
/// point that has no event row. Enforcing `recording`-only here (inside the
/// tap transaction, so a concurrent finish cannot race past it) keeps that
/// invariant unreachable from any tap path.
fn require_recording(tx: &Connection, roast_uuid: &str) -> Result<(), LoggerError> {
    let (status, _) =
        roast_status(tx, roast_uuid)?.ok_or_else(|| LoggerError::no_such_roast(roast_uuid))?;
    if status != RoastStatus::Recording {
        return Err(LoggerError::invalid_args(format!(
            "roast {roast_uuid} is {}; markers on non-recording roasts are edited via update_roast_markers",
            status.as_str()
        )));
    }
    Ok(())
}

/// Append a tap to `events` and recompute canonical markers. Only valid on
/// `recording` roasts (see `require_recording`).
/// `session_sec` defaults to "now" = the latest persisted sample's sessionSec
/// (replay-speed-aware, unlike the wall clock).
pub fn mark_event(
    conn: &mut Connection,
    roast_uuid: &str,
    kind: RoastEventKind,
    session_sec: Option<f64>,
    note: Option<&str>,
) -> Result<RoastMarkersDto, LoggerError> {
    let tx = conn.transaction()?;
    require_recording(&tx, roast_uuid)?;
    let session_sec = match session_sec {
        Some(sec) => sec,
        None => last_sample(&tx, roast_uuid)?
            .map(|(_, sec)| sec)
            .unwrap_or(0.0),
    };
    tx.execute(
        "INSERT INTO events (roast_uuid, kind, session_sec, note, created_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![roast_uuid, kind.as_str(), session_sec, note, now_ms()],
    )?;
    let markers = canonicalize_markers(&tx, roast_uuid)?;
    tx.commit()?;
    Ok(markers)
}

/// Remove the most recent tap of `kind` and recompute canonical markers.
/// Only valid on `recording` roasts (see `require_recording`).
pub fn undo_event(
    conn: &mut Connection,
    roast_uuid: &str,
    kind: RoastEventKind,
) -> Result<RoastMarkersDto, LoggerError> {
    let tx = conn.transaction()?;
    require_recording(&tx, roast_uuid)?;
    tx.execute(
        "DELETE FROM events WHERE rowid = (
           SELECT rowid FROM events WHERE roast_uuid = ?1 AND kind = ?2
           ORDER BY created_ms DESC, rowid DESC LIMIT 1)",
        params![roast_uuid, kind.as_str()],
    )?;
    let markers = canonicalize_markers(&tx, roast_uuid)?;
    tx.commit()?;
    Ok(markers)
}

/// Whether ANY tap of `kind` exists for the roast (auto or manual). The
/// engine uses this so an auto-mark can never re-tap an already-marked kind.
pub fn has_event(
    conn: &Connection,
    roast_uuid: &str,
    kind: RoastEventKind,
) -> Result<bool, LoggerError> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM events WHERE roast_uuid = ?1 AND kind = ?2 LIMIT 1",
            params![roast_uuid, kind.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

/// Append a bare `note` event row WITHOUT re-canonicalizing markers — the
/// resume-gap path (precedent: `discard_recovery`). `note` rows never affect
/// canonical markers, so skipping canonicalization is safe and cheap.
pub fn append_note_event(
    conn: &Connection,
    roast_uuid: &str,
    session_sec: f64,
    note: &str,
) -> Result<(), LoggerError> {
    require_roast(conn, roast_uuid)?;
    conn.execute(
        "INSERT INTO events (roast_uuid, kind, session_sec, note, created_ms)
         VALUES (?1, 'note', ?2, ?3, ?4)",
        params![roast_uuid, session_sec, note, now_ms()],
    )?;
    Ok(())
}

/// Persisted state `resume_session` needs to rebuild a crashed session
/// honestly: the wall-clock start (to re-anchor the device session clock so
/// the outage stays on the time axis) and the already-marked charge/drop
/// (to seed the auto-mark detector so it can never re-fire a marked kind).
#[derive(Debug, Clone)]
pub struct ResumeSeed {
    pub started_wall_ms: i64,
    /// Latest canonical charge: (absolute session_sec, pinned BT °F if any).
    pub charge: Option<(f64, Option<f64>)>,
    /// A drop tap exists (`drop_sec` is seconds-from-charge; existence is all
    /// the detector needs).
    pub drop_marked: bool,
}

/// Read the `ResumeSeed` off the roasts row. The canonical marker columns are
/// kept in lockstep with the events table by `canonicalize_markers` on every
/// tap, so for a `recording` roast they are ground truth for "is charge/drop
/// already marked".
pub fn resume_seed(conn: &Connection, roast_uuid: &str) -> Result<ResumeSeed, LoggerError> {
    conn.query_row(
        "SELECT started_wall_ms, charge_session_sec, charge_temp_f, drop_sec
         FROM roasts WHERE uuid = ?1",
        params![roast_uuid],
        |row| {
            let started_wall_ms: i64 = row.get(0)?;
            let charge_sec: Option<f64> = row.get(1)?;
            let charge_temp: Option<f64> = row.get(2)?;
            let drop_sec: Option<f64> = row.get(3)?;
            Ok(ResumeSeed {
                started_wall_ms,
                charge: charge_sec.map(|sec| (sec, charge_temp)),
                drop_marked: drop_sec.is_some(),
            })
        },
    )
    .optional()?
    .ok_or_else(|| LoggerError::no_such_roast(roast_uuid))
}

/// If no turning point is marked, detect it from the persisted curve
/// (parity with roast-console/math `detectTurningPoint`) and pin it.
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

    if let Some((t, bt)) = math::detect_turning_point(&points) {
        conn.execute(
            "UPDATE roasts SET turning_point_sec = ?2, turning_point_temp_f = ?3 WHERE uuid = ?1",
            params![roast_uuid, t, bt],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// finish / abandon / recovery
// ---------------------------------------------------------------------------

pub fn finish_roast(
    conn: &mut Connection,
    roast_uuid: &str,
    drop_weight_lb: Option<f64>,
    notes: Option<&str>,
) -> Result<RoastSummaryDto, LoggerError> {
    let tx = conn.transaction()?;
    require_roast(&tx, roast_uuid)?;
    canonicalize_markers(&tx, roast_uuid)?;
    auto_turning_point(&tx, roast_uuid)?;
    tx.execute(
        "UPDATE roasts SET status = ?2,
           drop_weight_lb = COALESCE(?3, drop_weight_lb),
           notes = COALESCE(?4, notes)
         WHERE uuid = ?1",
        params![
            roast_uuid,
            RoastStatus::Finished.as_str(),
            drop_weight_lb,
            notes
        ],
    )?;
    delete_setting(&tx, &speed_setting_key(roast_uuid))?;
    delete_setting(&tx, &crate::capture::pin_setting_key(roast_uuid))?;

    let summary =
        load_summary(&tx, roast_uuid)?.ok_or_else(|| LoggerError::no_such_roast(roast_uuid))?;
    let payload = GetRoastResult {
        summary: summary.clone(),
        samples: load_samples(&tx, roast_uuid)?,
        events: load_events(&tx, roast_uuid)?,
    };
    enqueue_outbox(&tx, Some(roast_uuid), "finalize", None, &payload)?;
    tx.commit()?;

    super::schema::checkpoint_truncate(conn);
    Ok(summary)
}

pub fn abandon_roast(conn: &mut Connection, roast_uuid: &str) -> Result<(), LoggerError> {
    let tx = conn.transaction()?;
    require_roast(&tx, roast_uuid)?;
    tx.execute(
        "UPDATE roasts SET status = ?2 WHERE uuid = ?1",
        params![roast_uuid, RoastStatus::Abandoned.as_str()],
    )?;
    delete_setting(&tx, &speed_setting_key(roast_uuid))?;
    delete_setting(&tx, &crate::capture::pin_setting_key(roast_uuid))?;
    tx.commit()?;
    Ok(())
}

/// Newest `recording` roast that is NOT the currently-active session — an
/// orphan from a previous process.
pub fn pending_recovery(
    conn: &Connection,
    exclude_uuid: Option<&str>,
) -> Result<Option<RecoveryDto>, LoggerError> {
    struct OrphanRow {
        roast_uuid: String,
        started_wall_ms: i64,
        meta: SessionMetaDto,
    }
    let row: Option<OrphanRow> = conn
        .query_row(
            "SELECT uuid, started_wall_ms, machine_local_id, coffee_name, charge_weight_lb,
                    coffee_local_id, reference_roast_uuid
             FROM roasts WHERE status = 'recording' AND uuid <> COALESCE(?1, '')
             ORDER BY started_wall_ms DESC, uuid DESC LIMIT 1",
            params![exclude_uuid],
            |row| {
                Ok(OrphanRow {
                    roast_uuid: row.get(0)?,
                    started_wall_ms: row.get(1)?,
                    meta: SessionMetaDto {
                        machine_local_id: row.get(2)?,
                        coffee_name: row.get(3)?,
                        charge_weight_lb: row.get(4)?,
                        coffee_local_id: row.get(5)?,
                        reference_roast_uuid: row.get(6)?,
                    },
                })
            },
        )
        .optional()?;
    let Some(orphan) = row else {
        return Ok(None);
    };
    let (last_seq, last_session_sec) = last_sample(conn, &orphan.roast_uuid)?.unwrap_or((0, 0.0));
    Ok(Some(RecoveryDto {
        roast_uuid: orphan.roast_uuid,
        started_wall_ms: orphan.started_wall_ms,
        last_seq,
        last_session_sec,
        meta: orphan.meta,
    }))
}

/// Close an orphaned `recording` roast as `finished` at its last sample with a
/// gap note. Data is preserved; markers are finalized like a normal finish.
pub fn discard_recovery(conn: &mut Connection, roast_uuid: &str) -> Result<(), LoggerError> {
    let tx = conn.transaction()?;
    let Some((status, _)) = roast_status(&tx, roast_uuid)? else {
        return Err(LoggerError::no_such_roast(roast_uuid));
    };
    if status != RoastStatus::Recording {
        // Already closed — idempotent.
        tx.commit()?;
        return Ok(());
    }
    let (_, last_session_sec) = last_sample(&tx, roast_uuid)?.unwrap_or((0, 0.0));
    tx.execute(
        "INSERT INTO events (roast_uuid, kind, session_sec, note, created_ms)
         VALUES (?1, 'note', ?2, ?3, ?4)",
        params![
            roast_uuid,
            last_session_sec,
            "gap: session interrupted; closed at last recorded sample",
            now_ms()
        ],
    )?;
    canonicalize_markers(&tx, roast_uuid)?;
    auto_turning_point(&tx, roast_uuid)?;
    tx.execute(
        "UPDATE roasts SET status = 'finished',
           notes = COALESCE(notes, 'Session interrupted — closed at last recorded sample (gap).')
         WHERE uuid = ?1",
        params![roast_uuid],
    )?;
    delete_setting(&tx, &speed_setting_key(roast_uuid))?;
    delete_setting(&tx, &crate::capture::pin_setting_key(roast_uuid))?;
    tx.commit()?;
    super::schema::checkpoint_truncate(conn);
    Ok(())
}

// ---------------------------------------------------------------------------
// reads
// ---------------------------------------------------------------------------

const SUMMARY_SELECT: &str = "SELECT r.uuid, r.status, r.started_wall_ms, m.name,
       r.coffee_name, r.charge_weight_lb, r.drop_weight_lb, r.notes,
       r.turning_point_sec, r.turning_point_temp_f, r.dry_end_sec,
       r.fc_start_sec, r.fc_end_sec, r.drop_sec, r.drop_temp_f, r.charge_temp_f
  FROM roasts r LEFT JOIN machines_local m ON m.id = r.machine_local_id";

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RoastSummaryDto> {
    let status: String = row.get(1)?;
    let markers = RoastMarkersDto {
        turning_point_sec: row.get(8)?,
        turning_point_temp_f: row.get(9)?,
        dry_end_sec: row.get(10)?,
        fc_start_sec: row.get(11)?,
        fc_end_sec: row.get(12)?,
        drop_sec: row.get(13)?,
        drop_temp_f: row.get(14)?,
        charge_temp_f: row.get(15)?,
    };
    let charge_weight_lb: Option<f64> = row.get(5)?;
    let drop_weight_lb: Option<f64> = row.get(6)?;
    Ok(RoastSummaryDto {
        roast_uuid: row.get(0)?,
        status: RoastStatus::parse(&status).unwrap_or(RoastStatus::Abandoned),
        started_wall_ms: row.get(2)?,
        machine_name: row.get(3)?,
        coffee_name: row.get(4)?,
        charge_weight_lb,
        drop_weight_lb,
        weight_loss_pct: math::weight_loss_pct(charge_weight_lb, drop_weight_lb),
        dtr: math::compute_dtr(markers.fc_start_sec, markers.drop_sec),
        notes: row.get(7)?,
        markers,
    })
}

pub fn load_summary(
    conn: &Connection,
    roast_uuid: &str,
) -> Result<Option<RoastSummaryDto>, LoggerError> {
    Ok(conn
        .query_row(
            &format!("{SUMMARY_SELECT} WHERE r.uuid = ?1"),
            params![roast_uuid],
            summary_from_row,
        )
        .optional()?)
}

pub fn list_roasts(conn: &Connection) -> Result<Vec<RoastSummaryDto>, LoggerError> {
    let mut stmt = conn.prepare(&format!(
        "{SUMMARY_SELECT} ORDER BY r.started_wall_ms DESC, r.uuid DESC"
    ))?;
    let rows = stmt.query_map([], summary_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn load_samples(conn: &Connection, roast_uuid: &str) -> Result<Vec<SampleDto>, LoggerError> {
    let mut stmt = conn.prepare(
        "SELECT seq, session_sec, bt_f, et_f, ambient_f, heater, fan, drum
         FROM samples WHERE roast_uuid = ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![roast_uuid], |row| {
        Ok(SampleDto {
            seq: row.get::<_, i64>(0)? as u64,
            session_sec: row.get(1)?,
            bt_f: row.get(2)?,
            et_f: row.get(3)?,
            ambient_f: row.get(4)?,
            heater: row.get(5)?,
            fan: row.get(6)?,
            drum: row.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn load_events(conn: &Connection, roast_uuid: &str) -> Result<Vec<EventDto>, LoggerError> {
    let mut stmt = conn.prepare(
        "SELECT kind, session_sec, note FROM events WHERE roast_uuid = ?1
         ORDER BY created_ms, rowid",
    )?;
    let rows = stmt.query_map(params![roast_uuid], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut events = Vec::new();
    for row in rows {
        let (kind, session_sec, note) = row?;
        match RoastEventKind::parse(&kind) {
            Some(kind) => events.push(EventDto {
                kind,
                session_sec,
                note,
            }),
            None => tracing::warn!(kind, "ignoring unknown event kind in db"),
        }
    }
    Ok(events)
}

pub fn get_roast(conn: &Connection, roast_uuid: &str) -> Result<GetRoastResult, LoggerError> {
    let summary =
        load_summary(conn, roast_uuid)?.ok_or_else(|| LoggerError::no_such_roast(roast_uuid))?;
    Ok(GetRoastResult {
        summary,
        samples: load_samples(conn, roast_uuid)?,
        events: load_events(conn, roast_uuid)?,
    })
}

// ---------------------------------------------------------------------------
// outbox (written in Phase 1, flushed in Phase 2b)
// ---------------------------------------------------------------------------

fn enqueue_outbox<T: serde::Serialize>(
    conn: &Connection,
    roast_uuid: Option<&str>,
    op: &str,
    chunk_index: Option<i64>,
    payload: &T,
) -> Result<(), LoggerError> {
    let payload =
        serde_json::to_vec(payload).map_err(|e| LoggerError::db(format!("outbox payload: {e}")))?;
    conn.execute(
        "INSERT INTO outbox (roast_uuid, op, chunk_index, payload_version, payload, created_ms)
         VALUES (?1, ?2, ?3, 1, ?4, ?5)",
        params![roast_uuid, op, chunk_index, payload, now_ms()],
    )?;
    Ok(())
}
