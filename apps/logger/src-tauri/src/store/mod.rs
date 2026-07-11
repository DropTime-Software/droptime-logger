//! store/ — the rusqlite persistence layer (CONTRACTS.md §1).
//!
//! One writer thread owns the only `Connection`; every operation crosses an
//! mpsc command queue. Sample appends are executed IMMEDIATELY on arrival
//! (inside an open batch transaction, so the row is in SQLite before the
//! caller's ack returns and the sample is emitted to the UI) and the batch
//! commits roughly once per second. Any non-append operation commits the open
//! batch first, so reads and marker updates always observe every prior sample.

mod imports;
mod library;
mod ops;
mod schema;
mod sync;

use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::error::LoggerError;
use crate::model::{
    GetRoastResult, RecoveryDto, RoastEventKind, RoastMarkersDto, RoastStatus, RoastSummaryDto,
    SampleDto, SessionMetaDto,
};

pub use ops::{now_ms, speed_setting_key, ResumeSeed};
pub use sync::{MarkSyncedArg, SyncRoastDto};

/// How long a sample batch transaction stays open before committing.
const BATCH_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// A sample destined for the `samples` table.
#[derive(Debug, Clone)]
pub struct SampleRow {
    pub roast_uuid: String,
    pub sample: SampleDto,
}

enum Cmd {
    Append {
        row: SampleRow,
        ack: mpsc::SyncSender<Result<(), LoggerError>>,
    },
    Task(Box<dyn FnOnce(&mut Connection) + Send>),
}

/// Cheap-to-clone handle to the writer thread.
#[derive(Clone)]
pub struct Store {
    tx: mpsc::Sender<Cmd>,
}

impl Store {
    /// Open (creating/migrating as needed) the DB at `db_path` and spawn the
    /// writer thread. Applies the §1 pragmas and mints `device_id` on first run.
    pub fn open(db_path: &Path) -> Result<Self, LoggerError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(Duration::from_millis(5000))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate(&mut conn, db_path)?;
        ops::ensure_device_id(&conn)?;

        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("store-writer".into())
            .spawn(move || writer_loop(conn, rx))
            .map_err(|e| LoggerError::io(format!("failed to spawn store writer: {e}")))?;
        Ok(Store { tx })
    }

    /// Append one sample. Blocks until the row has been executed against
    /// SQLite (inside the open batch transaction) — callers emit to the UI
    /// only after this returns Ok. (§2: SQLite before Channel.)
    pub fn append_sample(&self, row: SampleRow) -> Result<(), LoggerError> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.tx
            .send(Cmd::Append { row, ack: ack_tx })
            .map_err(|_| LoggerError::db("store writer thread is gone"))?;
        ack_rx
            .recv()
            .map_err(|_| LoggerError::db("store writer dropped the append"))?
    }

    /// Run `f` on the writer thread; the open sample batch is committed first.
    fn with_conn<T, F>(&self, f: F) -> Result<T, LoggerError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, LoggerError> + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Cmd::Task(Box::new(move |conn| {
                let _ = tx.send(f(conn));
            })))
            .map_err(|_| LoggerError::db("store writer thread is gone"))?;
        rx.recv()
            .map_err(|_| LoggerError::db("store writer dropped the request"))?
    }

    /// Force-commit any open sample batch (used before handing the file to
    /// another process/connection, and by tests).
    pub fn flush(&self) -> Result<(), LoggerError> {
        self.with_conn(|_| Ok(()))
    }

    // -- roasts -------------------------------------------------------------

    pub fn create_roast(
        &self,
        roast_uuid: &str,
        source_id: &str,
        started_wall_ms: i64,
        meta: &SessionMetaDto,
    ) -> Result<(), LoggerError> {
        let (roast_uuid, source_id, meta) =
            (roast_uuid.to_owned(), source_id.to_owned(), meta.clone());
        self.with_conn(move |conn| {
            ops::create_roast(conn, &roast_uuid, &source_id, started_wall_ms, &meta)
        })
    }

    pub fn delete_roast_shell(&self, roast_uuid: &str) -> Result<(), LoggerError> {
        // Only used to clean up a roast row whose source failed to start —
        // it can have no samples/events yet.
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| {
            conn.execute(
                "DELETE FROM roasts WHERE uuid = ?1",
                rusqlite::params![roast_uuid],
            )?;
            Ok(())
        })
    }

    pub fn roast_status(
        &self,
        roast_uuid: &str,
    ) -> Result<Option<(RoastStatus, String)>, LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| ops::roast_status(conn, &roast_uuid))
    }

    pub fn last_sample(&self, roast_uuid: &str) -> Result<Option<(u64, f64)>, LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| ops::last_sample(conn, &roast_uuid))
    }

    // -- events / markers ---------------------------------------------------

    pub fn mark_event(
        &self,
        roast_uuid: &str,
        kind: RoastEventKind,
        session_sec: Option<f64>,
        note: Option<String>,
    ) -> Result<RoastMarkersDto, LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| {
            ops::mark_event(conn, &roast_uuid, kind, session_sec, note.as_deref())
        })
    }

    pub fn undo_event(
        &self,
        roast_uuid: &str,
        kind: RoastEventKind,
    ) -> Result<RoastMarkersDto, LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| ops::undo_event(conn, &roast_uuid, kind))
    }

    /// Whether ANY tap of `kind` exists for the roast (auto or manual).
    pub fn has_event(&self, roast_uuid: &str, kind: RoastEventKind) -> Result<bool, LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| ops::has_event(conn, &roast_uuid, kind))
    }

    /// Append a bare `note` event row (no marker recanonicalization).
    pub fn append_note_event(
        &self,
        roast_uuid: &str,
        session_sec: f64,
        note: &str,
    ) -> Result<(), LoggerError> {
        let (roast_uuid, note) = (roast_uuid.to_owned(), note.to_owned());
        self.with_conn(move |conn| ops::append_note_event(conn, &roast_uuid, session_sec, &note))
    }

    /// Persisted state `resume_session` needs (wall start + marked charge/drop).
    pub fn resume_seed(&self, roast_uuid: &str) -> Result<ResumeSeed, LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| ops::resume_seed(conn, &roast_uuid))
    }

    // -- lifecycle ------------------------------------------------------------

    pub fn finish_roast(
        &self,
        roast_uuid: &str,
        drop_weight_lb: Option<f64>,
        notes: Option<String>,
    ) -> Result<RoastSummaryDto, LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| {
            ops::finish_roast(conn, &roast_uuid, drop_weight_lb, notes.as_deref())
        })
    }

    pub fn abandon_roast(&self, roast_uuid: &str) -> Result<(), LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| ops::abandon_roast(conn, &roast_uuid))
    }

    pub fn pending_recovery(
        &self,
        exclude_uuid: Option<String>,
    ) -> Result<Option<RecoveryDto>, LoggerError> {
        self.with_conn(move |conn| ops::pending_recovery(conn, exclude_uuid.as_deref()))
    }

    pub fn discard_recovery(&self, roast_uuid: &str) -> Result<(), LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| ops::discard_recovery(conn, &roast_uuid))
    }

    // -- reads ----------------------------------------------------------------

    pub fn list_roasts(&self) -> Result<Vec<RoastSummaryDto>, LoggerError> {
        self.with_conn(|conn| ops::list_roasts(conn))
    }

    pub fn get_roast(&self, roast_uuid: &str) -> Result<GetRoastResult, LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| ops::get_roast(conn, &roast_uuid))
    }

    // -- settings ---------------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, LoggerError> {
        let key = key.to_owned();
        self.with_conn(move |conn| ops::get_setting(conn, &key))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), LoggerError> {
        let (key, value) = (key.to_owned(), value.to_owned());
        self.with_conn(move |conn| ops::set_setting(conn, &key, &value))
    }

    pub fn delete_setting(&self, key: &str) -> Result<(), LoggerError> {
        let key = key.to_owned();
        self.with_conn(move |conn| ops::delete_setting(conn, &key))
    }
}

// ---------------------------------------------------------------------------
// writer thread
// ---------------------------------------------------------------------------

fn writer_loop(mut conn: Connection, rx: mpsc::Receiver<Cmd>) {
    let mut batch_started: Option<Instant> = None;

    loop {
        let timeout = match batch_started {
            Some(started) => BATCH_FLUSH_INTERVAL.saturating_sub(started.elapsed()),
            None => Duration::from_secs(60),
        };
        match rx.recv_timeout(timeout) {
            Ok(Cmd::Append { row, ack }) => {
                let mut result = Ok(());
                if batch_started.is_none() {
                    match conn.execute_batch("BEGIN IMMEDIATE;") {
                        Ok(()) => batch_started = Some(Instant::now()),
                        Err(err) => result = Err(err.into()),
                    }
                }
                if result.is_ok() {
                    result = ops::insert_sample(&conn, &row);
                }
                let _ = ack.send(result);
                if batch_started.is_some_and(|s| s.elapsed() >= BATCH_FLUSH_INTERVAL) {
                    commit_batch(&conn, &mut batch_started);
                }
            }
            Ok(Cmd::Task(task)) => {
                commit_batch(&conn, &mut batch_started);
                task(&mut conn);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => commit_batch(&conn, &mut batch_started),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                commit_batch(&conn, &mut batch_started);
                break;
            }
        }
    }
}

fn commit_batch(conn: &Connection, batch_started: &mut Option<Instant>) {
    if batch_started.take().is_none() {
        return;
    }
    if let Err(err) = conn.execute_batch("COMMIT;") {
        tracing::error!(error = %err, "sample batch commit failed; rolling back");
        if let Err(rb) = conn.execute_batch("ROLLBACK;") {
            tracing::error!(error = %rb, "rollback also failed");
        }
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) fn test_db_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!(
            "droptime-logger-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ))
        .join("droptime-logger.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RoastEventKind as K;

    fn sample(uuid: &str, seq: u64, session_sec: f64, bt: f64) -> SampleRow {
        SampleRow {
            roast_uuid: uuid.into(),
            sample: SampleDto {
                seq,
                session_sec,
                bt_f: bt,
                et_f: Some(bt + 30.0),
                ambient_f: None,
                heater: None,
                fan: None,
                drum: None,
            },
        }
    }

    /// A plausible BT curve: preheat fall to a TP at t=60, then a climb.
    fn bt_at(session_sec: f64) -> f64 {
        if session_sec <= 60.0 {
            380.0 - 3.5 * session_sec // 380 → 170
        } else {
            170.0 + 0.55 * (session_sec - 60.0)
        }
    }

    #[test]
    fn roundtrip_session_to_reopened_db() {
        let path = test_db_path("roundtrip");
        let store = Store::open(&path).unwrap();
        let uuid = "roast-1";
        let meta = SessionMetaDto {
            coffee_name: Some("Ethiopia Guji".into()),
            charge_weight_lb: Some(24.0),
            ..Default::default()
        };
        store
            .create_roast(uuid, "replay:ethiopia-guji", 1_700_000_000_000, &meta)
            .unwrap();

        for seq in 1..=300u64 {
            let t = (seq - 1) as f64;
            store.append_sample(sample(uuid, seq, t, bt_at(t))).unwrap();
        }

        store.mark_event(uuid, K::Charge, Some(0.0), None).unwrap();
        store
            .mark_event(uuid, K::DryEnd, Some(180.0), None)
            .unwrap();
        store
            .mark_event(uuid, K::FcStart, Some(250.0), None)
            .unwrap();
        let markers = store.mark_event(uuid, K::Drop, Some(299.0), None).unwrap();
        assert_eq!(markers.drop_sec, Some(299.0));
        assert!(markers.drop_temp_f.is_some());

        let summary = store
            .finish_roast(uuid, Some(20.4), Some("clean".into()))
            .unwrap();
        assert_eq!(summary.status, RoastStatus::Finished);
        // dtr parity: (299 - 250) / 299 = 0.16388… → 0.164
        assert_eq!(summary.dtr, Some(0.164));
        // weight loss parity: (24 - 20.4) / 24 = 15.0
        assert_eq!(summary.weight_loss_pct, Some(15.0));
        // auto turning point: min BT at t=60 (bt 170), later rise > +1 → detected
        assert_eq!(summary.markers.turning_point_sec, Some(60.0));
        assert_eq!(summary.markers.turning_point_temp_f, Some(170.0));

        // Reopen the file as a fresh "process" and verify everything survived.
        store.flush().unwrap();
        drop(store);
        let store2 = Store::open(&path).unwrap();
        let roast = store2.get_roast(uuid).unwrap();
        assert_eq!(roast.samples.len(), 300);
        assert_eq!(roast.samples[0].seq, 1);
        assert_eq!(roast.samples[299].session_sec, 299.0);
        assert_eq!(roast.events.len(), 4);
        assert_eq!(roast.summary.status, RoastStatus::Finished);
        assert_eq!(roast.summary.markers.drop_sec, Some(299.0));
        assert_eq!(roast.summary.notes.as_deref(), Some("clean"));

        let listed = store2.list_roasts().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].roast_uuid, uuid);
    }

    #[test]
    fn create_roast_persists_v2_meta_fields() {
        let store = Store::open(&test_db_path("v2-meta")).unwrap();
        let meta = SessionMetaDto {
            coffee_local_id: Some(3),
            reference_roast_uuid: Some("ref-1".into()),
            ..Default::default()
        };
        store
            .create_roast("roast-v2", "replay:fast-decaf", 1, &meta)
            .unwrap();
        let (coffee_local_id, reference): (Option<i64>, Option<String>) = store
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT coffee_local_id, reference_roast_uuid FROM roasts WHERE uuid = 'roast-v2'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(coffee_local_id, Some(3));
        assert_eq!(reference.as_deref(), Some("ref-1"));

        // ... and pending_recovery hands them back in the meta.
        let rec = store
            .pending_recovery(None)
            .unwrap()
            .expect("recording roast is an orphan");
        assert_eq!(rec.meta.coffee_local_id, Some(3));
        assert_eq!(rec.meta.reference_roast_uuid.as_deref(), Some("ref-1"));
    }

    #[test]
    fn device_id_minted_once_and_stable() {
        let path = test_db_path("device-id");
        let store = Store::open(&path).unwrap();
        let id = store
            .get_setting("device_id")
            .unwrap()
            .expect("device_id minted on first run");
        uuid::Uuid::parse_str(&id).expect("device_id is a UUID");
        store.flush().unwrap();
        drop(store);
        let store2 = Store::open(&path).unwrap();
        assert_eq!(
            store2.get_setting("device_id").unwrap().as_deref(),
            Some(id.as_str())
        );
    }

    #[test]
    fn marker_canonicalization_re_tap_and_undo() {
        let path = test_db_path("markers");
        let store = Store::open(&path).unwrap();
        let uuid = "roast-m";
        store
            .create_roast(uuid, "replay:fast-decaf", 1, &SessionMetaDto::default())
            .unwrap();
        for seq in 1..=240u64 {
            let t = (seq - 1) as f64;
            store.append_sample(sample(uuid, seq, t, bt_at(t))).unwrap();
        }

        // Marks are sessionSec; canonical markers are seconds-from-charge.
        store.mark_event(uuid, K::Charge, Some(30.0), None).unwrap();
        let m = store
            .mark_event(uuid, K::DryEnd, Some(210.0), None)
            .unwrap();
        assert_eq!(m.dry_end_sec, Some(180.0));
        // charge_temp_f pinned from the nearest sample to sessionSec 30 → bt_at(30)
        assert_eq!(m.charge_temp_f, Some(bt_at(30.0)));

        // Re-tapping charge shifts every relative marker (latest tap wins).
        let m = store.mark_event(uuid, K::Charge, Some(40.0), None).unwrap();
        assert_eq!(m.dry_end_sec, Some(170.0));
        assert_eq!(m.charge_temp_f, Some(bt_at(40.0)));

        // Undo the re-tap → falls back to the previous charge tap.
        let m = store.undo_event(uuid, K::Charge).unwrap();
        assert_eq!(m.dry_end_sec, Some(180.0));
        assert_eq!(m.charge_temp_f, Some(bt_at(30.0)));

        // Undo dry_end entirely.
        let m = store.undo_event(uuid, K::DryEnd).unwrap();
        assert_eq!(m.dry_end_sec, None);

        // Marker default "now": no sessionSec → latest persisted sample (239.0).
        let m = store.mark_event(uuid, K::Drop, None, None).unwrap();
        assert_eq!(m.drop_sec, Some(239.0 - 30.0));
        assert_eq!(m.drop_temp_f, Some(bt_at(239.0)));

        // Unknown roast → no_such_roast
        let err = store.mark_event("nope", K::Charge, None, None).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::NoSuchRoast);
    }

    #[test]
    fn mark_and_undo_are_recording_only() {
        // Taps are live-session operations; on finished roasts markers are
        // edited via update_roast_markers. A tap against a finished roast
        // would re-canonicalize and NULL an auto-detected turning point
        // (which has no event row) — refuse it with invalid_args.
        let store = Store::open(&test_db_path("tap-guard")).unwrap();
        let uuid = "roast-g";
        store
            .create_roast(uuid, "replay:fast-decaf", 1, &SessionMetaDto::default())
            .unwrap();
        for seq in 1..=120u64 {
            let t = (seq - 1) as f64;
            store.append_sample(sample(uuid, seq, t, bt_at(t))).unwrap();
        }
        store.mark_event(uuid, K::Charge, Some(0.0), None).unwrap();
        let summary = store.finish_roast(uuid, None, None).unwrap();
        let auto_tp = summary
            .markers
            .turning_point_sec
            .expect("auto TP pinned at finish");

        let err = store
            .mark_event(uuid, K::Drop, Some(100.0), None)
            .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgs);
        let err = store.undo_event(uuid, K::Charge).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgs);

        // The auto-detected turning point survived both refused taps.
        let roast = store.get_roast(uuid).unwrap();
        assert_eq!(roast.summary.markers.turning_point_sec, Some(auto_tp));
        assert!(
            roast.events.iter().all(|e| e.kind != K::Drop),
            "no drop row appended"
        );
    }

    #[test]
    fn recovery_pending_and_discard() {
        let path = test_db_path("recovery");
        let uuid = "roast-r";
        {
            let store = Store::open(&path).unwrap();
            store
                .create_roast(
                    uuid,
                    "replay:ethiopia-guji",
                    42,
                    &SessionMetaDto {
                        coffee_name: Some("Guji".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
            for seq in 1..=90u64 {
                let t = (seq - 1) as f64;
                store.append_sample(sample(uuid, seq, t, bt_at(t))).unwrap();
            }
            store.flush().unwrap();
            // Dropped without finish → simulated crash: status stays 'recording'.
        }

        let store = Store::open(&path).unwrap();
        let rec = store
            .pending_recovery(None)
            .unwrap()
            .expect("orphan detected");
        assert_eq!(rec.roast_uuid, uuid);
        assert_eq!(rec.started_wall_ms, 42);
        assert_eq!(rec.last_seq, 90);
        assert_eq!(rec.last_session_sec, 89.0);
        assert_eq!(rec.meta.coffee_name.as_deref(), Some("Guji"));

        // The active session must be excluded from recovery.
        assert!(store.pending_recovery(Some(uuid.into())).unwrap().is_none());

        store.discard_recovery(uuid).unwrap();
        assert!(store.pending_recovery(None).unwrap().is_none());
        let roast = store.get_roast(uuid).unwrap();
        assert_eq!(roast.summary.status, RoastStatus::Finished);
        assert!(roast.summary.notes.as_deref().unwrap_or("").contains("gap"));
        // gap note event appended at the last sample
        let note = roast
            .events
            .iter()
            .find(|e| e.kind == K::Note)
            .expect("gap note event");
        assert_eq!(note.session_sec, 89.0);
        assert!(note.note.as_deref().unwrap_or("").contains("gap"));
        // markers were finalized: TP auto-detected (charge defaulted to 0)
        assert_eq!(roast.summary.markers.turning_point_sec, Some(60.0));

        // Discard is idempotent.
        store.discard_recovery(uuid).unwrap();
    }

    #[test]
    fn batched_samples_survive_without_explicit_flush() {
        // The writer commits open batches when the queue disconnects, so even a
        // handle drop without flush() must not lose executed appends.
        let path = test_db_path("batch");
        let uuid = "roast-b";
        {
            let store = Store::open(&path).unwrap();
            store
                .create_roast(uuid, "replay:fast-decaf", 7, &SessionMetaDto::default())
                .unwrap();
            for seq in 1..=25u64 {
                store
                    .append_sample(sample(uuid, seq, (seq - 1) as f64, 300.0))
                    .unwrap();
            }
            // no flush() — rely on the disconnect commit
        }
        // Poll: the writer thread commits asynchronously after disconnect.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let store = Store::open(&path).unwrap();
            let n = store.get_roast(uuid).unwrap().samples.len();
            if n == 25 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "samples not committed after disconnect (saw {n})"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
