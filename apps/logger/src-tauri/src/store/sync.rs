//! store/sync.rs — Cloud-sync outbox reader (Droptime Cloud, build plan §10).
//!
//! Phase 1 (capture) enqueues durable `finalize` / `import` outbox rows
//! (ops.rs:finish_roast, imports.rs:insert_imported_roast). This module is the
//! read side the webview drainer pulls from: it assembles, for each pending
//! row, everything the `convex/logger.ts` mutation family needs — a stable
//! `clientMachineId`, the coffee name, weights, canonical markers, and the full
//! curve mapped to the Cloud's `CurvePoint` shape ({t, bt, et?, burner?,
//! airflow?, drum?} with `t` = seconds-from-charge so it aligns with the
//! seconds-from-charge markers). The webview owns the authed Convex calls; this
//! keeps the DB access (and the mapping) on the single writer thread.
//!
//! Idempotency is server-side (by_org_client on clientRoastId), so a row that
//! is read, POSTed, but not yet acked is simply replayed — harmless. `mark_synced`
//! stamps `outbox.synced_ms` AND `roasts.synced_batch_id` in one transaction so
//! an ack is atomic; the partial index `idx_outbox_pending` keeps the queue scan
//! O(pending).

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::LoggerError;
use crate::model::RoastMarkersDto;

use super::{ops, Store};

/// One Cloud `CurvePoint` (mirrors `convex/roasts.ts` `curvePoint`): `t` is
/// seconds-from-charge, temps °F. `ror` is omitted — the server derives it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurvePointDto {
    pub t: f64,
    pub bt: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub et: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burner: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub airflow: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drum: Option<f64>,
}

/// Everything the drainer needs to sync ONE pending roast to the Cloud. Built
/// server-side from the DB so the webview never re-reads SQLite; the drainer
/// maps this directly onto `logger.upsertMachine` + (`appendSampleChunk`×N →
/// `finalizeRoast`) for `op:"finalize"`, or `importLocalHistory` for `op:"import"`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRoastDto {
    /// outbox.id — the ack key passed back to `mark_synced`.
    pub outbox_id: i64,
    /// "finalize" | "import" (unknown ops are skipped by `pending_sync`).
    pub op: String,
    /// roast_uuid — the Cloud's clientRoastId (roastBatches.clientId).
    pub client_roast_id: String,
    /// per-install device id (settings.device_id).
    pub device_id: String,
    /// Stable machine identity for `upsertMachine`: `{deviceId}:m{localId}` or
    /// `{deviceId}:default` when the roast had no local machine assigned.
    pub client_machine_id: String,
    pub machine_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_make: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coffee_name: Option<String>,
    /// Roast wall-clock start; the drainer derives the ISO roastDate from it.
    pub started_wall_ms: i64,
    /// finalizeRoast/importLocalHistory require a charge weight; 0 when unknown.
    pub charge_weight_lb: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_weight_lb: Option<f64>,
    /// Canonical markers, flattened (seconds-from-charge; temps °F).
    #[serde(flatten)]
    pub markers: RoastMarkersDto,
    /// Full-resolution curve, `t` = seconds-from-charge.
    pub curve: Vec<CurvePointDto>,
}

/// Ack payload from the drainer once the Cloud accepts a row.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkSyncedArg {
    pub outbox_id: i64,
    pub client_roast_id: String,
    pub batch_id: String,
}

impl Store {
    /// Oldest-first pending sync rows (synced_ms IS NULL), assembled for the
    /// Cloud. Unknown/reserved ops (start_live/chunk/live_patch) are skipped so
    /// the drainer only ever sees replayable finalize/import work.
    pub fn pending_sync(&self, limit: i64) -> Result<Vec<SyncRoastDto>, LoggerError> {
        self.with_conn(move |conn| pending_sync(conn, limit))
    }

    /// Count of pending outbox rows — cheap, for the sync-status indicator.
    pub fn pending_sync_count(&self) -> Result<i64, LoggerError> {
        self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM outbox WHERE synced_ms IS NULL",
                [],
                |r| r.get(0),
            )?)
        })
    }

    /// Atomically ack a batch of synced rows: stamp outbox.synced_ms and
    /// roasts.synced_batch_id per row in ONE transaction. Rows whose roast was
    /// deleted between read and ack are tolerated (the UPDATE simply matches 0).
    pub fn mark_synced(&self, acks: Vec<MarkSyncedArg>) -> Result<(), LoggerError> {
        self.with_conn(move |conn| {
            let tx = conn.transaction()?;
            let now = ops::now_ms();
            for ack in &acks {
                tx.execute(
                    "UPDATE outbox SET synced_ms = ?2 WHERE id = ?1",
                    params![ack.outbox_id, now],
                )?;
                tx.execute(
                    "UPDATE roasts SET synced_batch_id = ?2 WHERE uuid = ?1",
                    params![ack.client_roast_id, ack.batch_id],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }
}

fn pending_sync(conn: &Connection, limit: i64) -> Result<Vec<SyncRoastDto>, LoggerError> {
    let device_id =
        ops::get_setting(conn, "device_id")?.ok_or_else(|| LoggerError::db("device_id missing"))?;

    let mut stmt = conn.prepare(
        "SELECT id, roast_uuid, op FROM outbox
         WHERE synced_ms IS NULL AND op IN ('finalize', 'import') AND roast_uuid IS NOT NULL
         ORDER BY id ASC LIMIT ?1",
    )?;
    let rows: Vec<(i64, String, String)> = stmt
        .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;

    let mut out = Vec::with_capacity(rows.len());
    for (outbox_id, roast_uuid, op) in rows {
        // A roast can vanish (delete_roast also nukes its outbox rows), but read
        // order isn't transactional with delete — skip a row whose roast is gone.
        let Some(dto) = build_sync_roast(conn, outbox_id, &op, &roast_uuid, &device_id)? else {
            continue;
        };
        out.push(dto);
    }
    Ok(out)
}

#[allow(clippy::type_complexity)]
fn build_sync_roast(
    conn: &Connection,
    outbox_id: i64,
    op: &str,
    roast_uuid: &str,
    device_id: &str,
) -> Result<Option<SyncRoastDto>, LoggerError> {
    let row: Option<RoastSyncRow> = conn
        .query_row(
            "SELECT r.started_wall_ms, r.charge_weight_lb, r.drop_weight_lb,
                    r.charge_session_sec, r.charge_temp_f,
                    r.turning_point_sec, r.turning_point_temp_f, r.dry_end_sec,
                    r.fc_start_sec, r.fc_end_sec, r.drop_sec, r.drop_temp_f,
                    r.machine_local_id, r.coffee_name, m.name, m.make
             FROM roasts r LEFT JOIN machines_local m ON m.id = r.machine_local_id
             WHERE r.uuid = ?1",
            params![roast_uuid],
            |r| {
                Ok(RoastSyncRow {
                    started_wall_ms: r.get(0)?,
                    charge_weight_lb: r.get(1)?,
                    drop_weight_lb: r.get(2)?,
                    charge_session_sec: r.get(3)?,
                    charge_temp_f: r.get(4)?,
                    turning_point_sec: r.get(5)?,
                    turning_point_temp_f: r.get(6)?,
                    dry_end_sec: r.get(7)?,
                    fc_start_sec: r.get(8)?,
                    fc_end_sec: r.get(9)?,
                    drop_sec: r.get(10)?,
                    drop_temp_f: r.get(11)?,
                    machine_local_id: r.get(12)?,
                    coffee_name: r.get(13)?,
                    machine_name: r.get(14)?,
                    machine_make: r.get(15)?,
                })
            },
        )
        .optional()?;
    let Some(row) = row else { return Ok(None) };

    let client_machine_id = match row.machine_local_id {
        Some(id) => format!("{device_id}:m{id}"),
        None => format!("{device_id}:default"),
    };
    let machine_name = row
        .machine_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Roaster")
        .to_string();

    // t = seconds-from-charge so the curve aligns with the seconds-from-charge
    // markers; charge defaults to session origin 0 until it is marked.
    let charge_origin = row.charge_session_sec.unwrap_or(0.0);
    let mut curve = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT session_sec, bt_f, et_f, heater, fan, drum
         FROM samples WHERE roast_uuid = ?1 ORDER BY seq",
    )?;
    let sample_iter = stmt.query_map(params![roast_uuid], |r| {
        Ok(CurvePointDto {
            t: r.get::<_, f64>(0)? - charge_origin,
            bt: r.get(1)?,
            et: r.get(2)?,
            burner: r.get(3)?,
            airflow: r.get(4)?,
            drum: r.get(5)?,
        })
    })?;
    for cp in sample_iter {
        curve.push(cp?);
    }

    Ok(Some(SyncRoastDto {
        outbox_id,
        op: op.to_string(),
        client_roast_id: roast_uuid.to_string(),
        device_id: device_id.to_string(),
        client_machine_id,
        machine_name,
        machine_make: row.machine_make,
        coffee_name: row.coffee_name,
        started_wall_ms: row.started_wall_ms,
        charge_weight_lb: row.charge_weight_lb.unwrap_or(0.0),
        drop_weight_lb: row.drop_weight_lb,
        markers: RoastMarkersDto {
            turning_point_sec: row.turning_point_sec,
            turning_point_temp_f: row.turning_point_temp_f,
            dry_end_sec: row.dry_end_sec,
            fc_start_sec: row.fc_start_sec,
            fc_end_sec: row.fc_end_sec,
            drop_sec: row.drop_sec,
            drop_temp_f: row.drop_temp_f,
            charge_temp_f: row.charge_temp_f,
        },
        curve,
    }))
}

struct RoastSyncRow {
    started_wall_ms: i64,
    charge_weight_lb: Option<f64>,
    drop_weight_lb: Option<f64>,
    charge_session_sec: Option<f64>,
    charge_temp_f: Option<f64>,
    turning_point_sec: Option<f64>,
    turning_point_temp_f: Option<f64>,
    dry_end_sec: Option<f64>,
    fc_start_sec: Option<f64>,
    fc_end_sec: Option<f64>,
    drop_sec: Option<f64>,
    drop_temp_f: Option<f64>,
    machine_local_id: Option<i64>,
    coffee_name: Option<String>,
    machine_name: Option<String>,
    machine_make: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RoastEventKind as K, SampleDto, SaveMachineArgs, SessionMetaDto};
    use crate::store::{test_db_path, SampleRow, Store};

    fn sample(uuid: &str, seq: u64, session_sec: f64, bt: f64) -> SampleRow {
        SampleRow {
            roast_uuid: uuid.into(),
            sample: SampleDto {
                seq,
                session_sec,
                bt_f: bt,
                et_f: Some(bt + 30.0),
                ambient_f: None,
                heater: Some(60.0),
                fan: Some(40.0),
                drum: None,
            },
        }
    }

    #[test]
    fn finalize_row_maps_to_sync_dto() {
        let store = Store::open(&test_db_path("sync-finalize")).unwrap();
        let device_id = store.get_setting("device_id").unwrap().unwrap();
        let machine = store
            .save_machine(&SaveMachineArgs {
                id: None,
                name: "Loring S15".into(),
                make: Some("Loring".into()),
                source_pin_json: None,
            })
            .unwrap();

        let uuid = "roast-sync-1";
        store
            .create_roast(
                uuid,
                "replay:x",
                1_700_000_000_000,
                &SessionMetaDto {
                    machine_local_id: Some(machine.id),
                    coffee_name: Some("Ethiopia Guji".into()),
                    charge_weight_lb: Some(24.0),
                    ..Default::default()
                },
            )
            .unwrap();
        for seq in 1..=120u64 {
            let t = (seq - 1) as f64;
            store
                .append_sample(sample(uuid, seq, t, 380.0 - t))
                .unwrap();
        }
        // charge at sessionSec 10 → curve t rebased to seconds-from-charge.
        store.mark_event(uuid, K::Charge, Some(10.0), None).unwrap();
        store.mark_event(uuid, K::Drop, Some(110.0), None).unwrap();
        store.finish_roast(uuid, Some(20.4), None).unwrap();

        let pending = store.pending_sync(100).unwrap();
        assert_eq!(pending.len(), 1);
        let d = &pending[0];
        assert_eq!(d.op, "finalize");
        assert_eq!(d.client_roast_id, uuid);
        assert_eq!(d.client_machine_id, format!("{device_id}:m{}", machine.id));
        assert_eq!(d.machine_name, "Loring S15");
        assert_eq!(d.machine_make.as_deref(), Some("Loring"));
        assert_eq!(d.coffee_name.as_deref(), Some("Ethiopia Guji"));
        assert_eq!(d.charge_weight_lb, 24.0);
        assert_eq!(d.drop_weight_lb, Some(20.4));
        assert_eq!(d.markers.drop_sec, Some(100.0)); // 110 - 10
                                                     // curve t rebased: first sample sessionSec 0 → t = -10 (pre-charge)
        assert_eq!(d.curve.len(), 120);
        assert_eq!(d.curve[0].t, -10.0);
        assert_eq!(d.curve[10].t, 0.0); // sessionSec 10 == charge
        assert_eq!(d.curve[0].burner, Some(60.0));
        assert_eq!(d.curve[0].airflow, Some(40.0));

        // ack: marks synced + stamps batch id, and it leaves the queue.
        store
            .mark_synced(vec![MarkSyncedArg {
                outbox_id: d.outbox_id,
                client_roast_id: uuid.into(),
                batch_id: "batch_abc".into(),
            }])
            .unwrap();
        assert_eq!(store.pending_sync_count().unwrap(), 0);
        assert!(store.pending_sync(100).unwrap().is_empty());
        let synced: Option<String> = store
            .with_conn(|c| {
                Ok(c.query_row(
                    "SELECT synced_batch_id FROM roasts WHERE uuid = 'roast-sync-1'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(synced.as_deref(), Some("batch_abc"));
    }

    #[test]
    fn roast_without_machine_gets_default_client_machine_id() {
        let store = Store::open(&test_db_path("sync-nomachine")).unwrap();
        let device_id = store.get_setting("device_id").unwrap().unwrap();
        let uuid = "roast-nomachine";
        store
            .create_roast(uuid, "replay:x", 42, &SessionMetaDto::default())
            .unwrap();
        store.append_sample(sample(uuid, 1, 0.0, 300.0)).unwrap();
        store.finish_roast(uuid, None, None).unwrap();

        let pending = store.pending_sync(100).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].client_machine_id, format!("{device_id}:default"));
        assert_eq!(pending[0].machine_name, "Roaster");
        assert_eq!(pending[0].charge_weight_lb, 0.0);
    }
}
