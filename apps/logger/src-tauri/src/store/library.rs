//! store/library.rs — machines/coffees library + roast editing (CONTRACTS.md §7.3).
//!
//! Owner: librarian. All SQL lands in ops-style helpers here (running through
//! `with_conn` on the writer thread); the `Store` methods below are the surface
//! the IPC commands call.
//!
//! Semantics (frozen):
//! - machines/coffees are never hard-deleted — `archived = 1` only; list_* return
//!   non-archived rows, name-ordered (case-insensitive).
//! - `save_*` upsert by optional id; a non-empty name is required (`invalid_args`).
//! - `update_roast` uses the double-Option patch DTO (absent = unchanged, explicit
//!   null = clear). Setting `coffeeLocalId` also denormalizes `coffee_name` from
//!   `coffees_local` so summaries stay fast; `machine_name` is resolved by the
//!   SUMMARY_SELECT join, so only `machine_local_id` is written.
//! - `update_roast_markers`: only on `finished` roasts (else `invalid_args`);
//!   charge NOT editable; each editable marker is rewritten directly on `roasts`
//!   (never re-canonicalized — that would clobber an auto-detected turning point
//!   which has no event row). A set appends an `events` row (kind = the marker's
//!   event kind, note `"edited"`) at `charge_origin + sec`; drop/turning-point
//!   temps are recomputed from the nearest sample; a clear drops the kind's events
//!   and nulls the column(s). Derived fields (dtr, weight_loss_pct) fall out of the
//!   column read in `load_summary`.
//! - `delete_roast` hard-deletes roast + samples + events + its outbox rows in one
//!   transaction (the active-session guard lives in ipc.rs).

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use crate::error::LoggerError;
use crate::model::{
    CoffeeDto, MachineDto, MarkerPatchDto, RoastEventKind, RoastMarkersDto, RoastPatchDto,
    RoastStatus, RoastSummaryDto, SaveCoffeeArgs, SaveMachineArgs,
};

use super::{ops, Store};

// ---------------------------------------------------------------------------
// Store surface (thin wrappers → writer thread; see store/mod.rs)
// ---------------------------------------------------------------------------

impl Store {
    pub fn list_machines(&self) -> Result<Vec<MachineDto>, LoggerError> {
        self.with_conn(list_machines)
    }

    pub fn save_machine(&self, args: &SaveMachineArgs) -> Result<MachineDto, LoggerError> {
        let args = args.clone();
        self.with_conn(move |conn| save_machine(conn, &args))
    }

    pub fn archive_machine(&self, id: i64) -> Result<(), LoggerError> {
        self.with_conn(move |conn| archive_machine(conn, id))
    }

    pub fn list_coffees(&self) -> Result<Vec<CoffeeDto>, LoggerError> {
        self.with_conn(list_coffees)
    }

    pub fn save_coffee(&self, args: &SaveCoffeeArgs) -> Result<CoffeeDto, LoggerError> {
        let args = args.clone();
        self.with_conn(move |conn| save_coffee(conn, &args))
    }

    pub fn archive_coffee(&self, id: i64) -> Result<(), LoggerError> {
        self.with_conn(move |conn| archive_coffee(conn, id))
    }

    pub fn update_roast(
        &self,
        roast_uuid: &str,
        patch: &RoastPatchDto,
    ) -> Result<RoastSummaryDto, LoggerError> {
        let (roast_uuid, patch) = (roast_uuid.to_owned(), patch.clone());
        self.with_conn(move |conn| update_roast(conn, &roast_uuid, &patch))
    }

    pub fn update_roast_markers(
        &self,
        roast_uuid: &str,
        markers: &MarkerPatchDto,
    ) -> Result<RoastMarkersDto, LoggerError> {
        let (roast_uuid, markers) = (roast_uuid.to_owned(), markers.clone());
        self.with_conn(move |conn| update_roast_markers(conn, &roast_uuid, &markers))
    }

    pub fn delete_roast(&self, roast_uuid: &str) -> Result<(), LoggerError> {
        let roast_uuid = roast_uuid.to_owned();
        self.with_conn(move |conn| delete_roast(conn, &roast_uuid))
    }
}

// ---------------------------------------------------------------------------
// small value helpers
// ---------------------------------------------------------------------------

fn opt_text(v: Option<String>) -> Value {
    v.map(Value::Text).unwrap_or(Value::Null)
}
fn opt_int(v: Option<i64>) -> Value {
    v.map(Value::Integer).unwrap_or(Value::Null)
}
fn opt_real(v: Option<f64>) -> Value {
    v.map(Value::Real).unwrap_or(Value::Null)
}

fn roast_exists(conn: &Connection, roast_uuid: &str) -> Result<bool, LoggerError> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM roasts WHERE uuid = ?1",
            params![roast_uuid],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// BT of the sample nearest (in sessionSec) to `session_sec`; earlier wins ties.
/// (Mirrors ops::nearest_bt, which is private to that module.)
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
// machines
// ---------------------------------------------------------------------------

fn machine_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MachineDto> {
    Ok(MachineDto {
        id: row.get(0)?,
        name: row.get(1)?,
        make: row.get(2)?,
        source_pin_json: row.get(3)?,
        archived: row.get::<_, i64>(4)? != 0,
    })
}

fn list_machines(conn: &mut Connection) -> Result<Vec<MachineDto>, LoggerError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, make, source_pin, archived FROM machines_local
         WHERE archived = 0 ORDER BY name COLLATE NOCASE, id",
    )?;
    let rows = stmt.query_map([], machine_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn load_machine(conn: &Connection, id: i64) -> Result<MachineDto, LoggerError> {
    conn.query_row(
        "SELECT id, name, make, source_pin, archived FROM machines_local WHERE id = ?1",
        params![id],
        machine_from_row,
    )
    .optional()?
    .ok_or_else(|| LoggerError::invalid_args(format!("no such machine: {id}")))
}

fn save_machine(conn: &mut Connection, args: &SaveMachineArgs) -> Result<MachineDto, LoggerError> {
    let name = args.name.trim();
    if name.is_empty() {
        return Err(LoggerError::invalid_args("machine name must not be empty"));
    }
    let id = match args.id {
        Some(id) => {
            let changed = conn.execute(
                "UPDATE machines_local SET name = ?2, make = ?3, source_pin = ?4 WHERE id = ?1",
                params![id, name, args.make, args.source_pin_json],
            )?;
            if changed == 0 {
                return Err(LoggerError::invalid_args(format!("no such machine: {id}")));
            }
            id
        }
        None => {
            conn.execute(
                "INSERT INTO machines_local (name, make, source_pin) VALUES (?1, ?2, ?3)",
                params![name, args.make, args.source_pin_json],
            )?;
            conn.last_insert_rowid()
        }
    };
    load_machine(conn, id)
}

fn archive_machine(conn: &mut Connection, id: i64) -> Result<(), LoggerError> {
    let changed = conn.execute(
        "UPDATE machines_local SET archived = 1 WHERE id = ?1",
        params![id],
    )?;
    if changed == 0 {
        return Err(LoggerError::invalid_args(format!("no such machine: {id}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// coffees
// ---------------------------------------------------------------------------

fn coffee_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CoffeeDto> {
    Ok(CoffeeDto {
        id: row.get(0)?,
        name: row.get(1)?,
        origin: row.get(2)?,
        process: row.get(3)?,
        notes: row.get(4)?,
        archived: row.get::<_, i64>(5)? != 0,
    })
}

fn list_coffees(conn: &mut Connection) -> Result<Vec<CoffeeDto>, LoggerError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, origin, process, notes, archived FROM coffees_local
         WHERE archived = 0 ORDER BY name COLLATE NOCASE, id",
    )?;
    let rows = stmt.query_map([], coffee_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn load_coffee(conn: &Connection, id: i64) -> Result<CoffeeDto, LoggerError> {
    conn.query_row(
        "SELECT id, name, origin, process, notes, archived FROM coffees_local WHERE id = ?1",
        params![id],
        coffee_from_row,
    )
    .optional()?
    .ok_or_else(|| LoggerError::invalid_args(format!("no such coffee: {id}")))
}

fn save_coffee(conn: &mut Connection, args: &SaveCoffeeArgs) -> Result<CoffeeDto, LoggerError> {
    let name = args.name.trim();
    if name.is_empty() {
        return Err(LoggerError::invalid_args("coffee name must not be empty"));
    }
    let id = match args.id {
        Some(id) => {
            let changed = conn.execute(
                "UPDATE coffees_local SET name = ?2, origin = ?3, process = ?4, notes = ?5
                 WHERE id = ?1",
                params![id, name, args.origin, args.process, args.notes],
            )?;
            if changed == 0 {
                return Err(LoggerError::invalid_args(format!("no such coffee: {id}")));
            }
            id
        }
        None => {
            conn.execute(
                "INSERT INTO coffees_local (name, origin, process, notes) VALUES (?1, ?2, ?3, ?4)",
                params![name, args.origin, args.process, args.notes],
            )?;
            conn.last_insert_rowid()
        }
    };
    load_coffee(conn, id)
}

fn archive_coffee(conn: &mut Connection, id: i64) -> Result<(), LoggerError> {
    let changed = conn.execute(
        "UPDATE coffees_local SET archived = 1 WHERE id = ?1",
        params![id],
    )?;
    if changed == 0 {
        return Err(LoggerError::invalid_args(format!("no such coffee: {id}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// update_roast (meta patch)
// ---------------------------------------------------------------------------

fn update_roast(
    conn: &mut Connection,
    roast_uuid: &str,
    patch: &RoastPatchDto,
) -> Result<RoastSummaryDto, LoggerError> {
    if !roast_exists(conn, roast_uuid)? {
        return Err(LoggerError::no_such_roast(roast_uuid));
    }

    let mut sets: Vec<&str> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();

    // Coffee link + denormalized name. Setting coffeeLocalId also copies the
    // coffee's name into roasts.coffee_name (summaries read the column directly).
    let mut coffee_name_change = patch.coffee_name.clone();
    if let Some(link) = &patch.coffee_local_id {
        match link {
            Some(id) => {
                let coffee = load_coffee(conn, *id)?; // → invalid_args if unknown
                sets.push("coffee_local_id = ?");
                vals.push(Value::Integer(*id));
                coffee_name_change = Some(Some(coffee.name));
            }
            None => {
                sets.push("coffee_local_id = ?");
                vals.push(Value::Null);
            }
        }
    }
    if let Some(name) = coffee_name_change {
        sets.push("coffee_name = ?");
        vals.push(opt_text(name));
    }
    if let Some(machine) = patch.machine_local_id {
        sets.push("machine_local_id = ?");
        vals.push(opt_int(machine));
    }
    if let Some(w) = patch.charge_weight_lb {
        sets.push("charge_weight_lb = ?");
        vals.push(opt_real(w));
    }
    if let Some(w) = patch.drop_weight_lb {
        sets.push("drop_weight_lb = ?");
        vals.push(opt_real(w));
    }
    if let Some(notes) = &patch.notes {
        sets.push("notes = ?");
        vals.push(opt_text(notes.clone()));
    }

    if !sets.is_empty() {
        let sql = format!("UPDATE roasts SET {} WHERE uuid = ?", sets.join(", "));
        vals.push(Value::Text(roast_uuid.to_owned()));
        conn.execute(&sql, params_from_iter(vals))?;
    }

    ops::load_summary(conn, roast_uuid)?.ok_or_else(|| LoggerError::no_such_roast(roast_uuid))
}

// ---------------------------------------------------------------------------
// update_roast_markers (marker patch, finished roasts only)
// ---------------------------------------------------------------------------

/// One editable marker on `roasts`. `sec_col` is the seconds-from-charge column;
/// `temp_col` is its pinned-temperature column (only turning point + drop have one).
struct MarkerCol {
    kind: RoastEventKind,
    sec_col: &'static str,
    temp_col: Option<&'static str>,
}

fn apply_marker(
    tx: &Connection,
    roast_uuid: &str,
    charge_origin: f64,
    col: &MarkerCol,
    change: &Option<Option<f64>>,
) -> Result<(), LoggerError> {
    let Some(change) = change else {
        return Ok(()); // absent → unchanged
    };
    // Any prior tap of this kind is replaced by the edit (or removed on clear) so
    // the events history matches the marker column exactly.
    tx.execute(
        "DELETE FROM events WHERE roast_uuid = ?1 AND kind = ?2",
        params![roast_uuid, col.kind.as_str()],
    )?;
    match change {
        Some(rel_sec) => {
            let abs_sec = charge_origin + rel_sec;
            tx.execute(
                "INSERT INTO events (roast_uuid, kind, session_sec, note, created_ms)
                 VALUES (?1, ?2, ?3, 'edited', ?4)",
                params![roast_uuid, col.kind.as_str(), abs_sec, ops::now_ms()],
            )?;
            tx.execute(
                &format!("UPDATE roasts SET {} = ?2 WHERE uuid = ?1", col.sec_col),
                params![roast_uuid, rel_sec],
            )?;
            if let Some(temp_col) = col.temp_col {
                let temp = nearest_bt(tx, roast_uuid, abs_sec)?;
                tx.execute(
                    &format!("UPDATE roasts SET {temp_col} = ?2 WHERE uuid = ?1"),
                    params![roast_uuid, temp],
                )?;
            }
        }
        None => {
            tx.execute(
                &format!("UPDATE roasts SET {} = NULL WHERE uuid = ?1", col.sec_col),
                params![roast_uuid],
            )?;
            if let Some(temp_col) = col.temp_col {
                tx.execute(
                    &format!("UPDATE roasts SET {temp_col} = NULL WHERE uuid = ?1"),
                    params![roast_uuid],
                )?;
            }
        }
    }
    Ok(())
}

fn update_roast_markers(
    conn: &mut Connection,
    roast_uuid: &str,
    patch: &MarkerPatchDto,
) -> Result<RoastMarkersDto, LoggerError> {
    let status = ops::roast_status(conn, roast_uuid)?
        .ok_or_else(|| LoggerError::no_such_roast(roast_uuid))?
        .0;
    if status != RoastStatus::Finished {
        return Err(LoggerError::invalid_args(
            "markers can only be edited on a finished roast",
        ));
    }

    let tx = conn.transaction()?;
    let charge_origin: f64 = tx
        .query_row(
            "SELECT charge_session_sec FROM roasts WHERE uuid = ?1",
            params![roast_uuid],
            |r| r.get::<_, Option<f64>>(0),
        )?
        .unwrap_or(0.0);

    // `Option<Option<f64>>` is Copy, so the patch fields go into the array by value.
    let fields: [(MarkerCol, Option<Option<f64>>); 5] = [
        (
            MarkerCol {
                kind: RoastEventKind::TurningPoint,
                sec_col: "turning_point_sec",
                temp_col: Some("turning_point_temp_f"),
            },
            patch.turning_point_sec,
        ),
        (
            MarkerCol {
                kind: RoastEventKind::DryEnd,
                sec_col: "dry_end_sec",
                temp_col: None,
            },
            patch.dry_end_sec,
        ),
        (
            MarkerCol {
                kind: RoastEventKind::FcStart,
                sec_col: "fc_start_sec",
                temp_col: None,
            },
            patch.fc_start_sec,
        ),
        (
            MarkerCol {
                kind: RoastEventKind::FcEnd,
                sec_col: "fc_end_sec",
                temp_col: None,
            },
            patch.fc_end_sec,
        ),
        (
            MarkerCol {
                kind: RoastEventKind::Drop,
                sec_col: "drop_sec",
                temp_col: Some("drop_temp_f"),
            },
            patch.drop_sec,
        ),
    ];
    for (col, change) in &fields {
        apply_marker(&tx, roast_uuid, charge_origin, col, change)?;
    }

    let markers = ops::load_summary(&tx, roast_uuid)?
        .ok_or_else(|| LoggerError::no_such_roast(roast_uuid))?
        .markers;
    tx.commit()?;
    Ok(markers)
}

// ---------------------------------------------------------------------------
// delete_roast (cascade)
// ---------------------------------------------------------------------------

fn delete_roast(conn: &mut Connection, roast_uuid: &str) -> Result<(), LoggerError> {
    let tx = conn.transaction()?;
    if !roast_exists(&tx, roast_uuid)? {
        return Err(LoggerError::no_such_roast(roast_uuid));
    }
    // Children first (FK: samples/events reference roasts; outbox has no FK).
    tx.execute(
        "DELETE FROM samples WHERE roast_uuid = ?1",
        params![roast_uuid],
    )?;
    tx.execute(
        "DELETE FROM events WHERE roast_uuid = ?1",
        params![roast_uuid],
    )?;
    tx.execute(
        "DELETE FROM outbox WHERE roast_uuid = ?1",
        params![roast_uuid],
    )?;
    tx.execute("DELETE FROM roasts WHERE uuid = ?1", params![roast_uuid])?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::model::{RoastEventKind as K, SampleDto, SessionMetaDto};
    use crate::store::{test_db_path, SampleRow};

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

    /// Same synthetic BT curve the store tests use: preheat fall to TP at t=60,
    /// then a climb.
    fn bt_at(session_sec: f64) -> f64 {
        if session_sec <= 60.0 {
            380.0 - 3.5 * session_sec
        } else {
            170.0 + 0.55 * (session_sec - 60.0)
        }
    }

    /// A finished roast with charge pinned at sessionSec 0 (so seconds-from-charge
    /// == absolute sessionSec) and drop at 299.
    fn finished_roast(store: &Store, uuid: &str) {
        store
            .create_roast(
                uuid,
                "replay:ethiopia-guji",
                1_700_000_000_000,
                &SessionMetaDto::default(),
            )
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
        store.mark_event(uuid, K::Drop, Some(299.0), None).unwrap();
        store
            .finish_roast(uuid, Some(20.4), Some("clean".into()))
            .unwrap();
    }

    #[test]
    fn machines_crud_roundtrip_and_archive() {
        let store = Store::open(&test_db_path("lib-machines")).unwrap();

        let created = store
            .save_machine(&SaveMachineArgs {
                id: None,
                name: "  SF-25  ".into(),
                make: Some("San Franciscan".into()),
                source_pin_json: Some(r#"{"sourceId":"tc4:usbserial-1"}"#.into()),
            })
            .unwrap();
        assert_eq!(created.name, "SF-25", "name is trimmed on save");
        assert!(!created.archived);
        assert!(created.source_pin_json.is_some());

        // Update in place by id.
        let updated = store
            .save_machine(&SaveMachineArgs {
                id: Some(created.id),
                name: "SF-25 Roaster".into(),
                make: Some("San Franciscan".into()),
                source_pin_json: None,
            })
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, "SF-25 Roaster");
        assert_eq!(
            updated.source_pin_json, None,
            "save overwrites source_pin with the given value"
        );

        // A second machine, name-ordered ahead of the first.
        store
            .save_machine(&SaveMachineArgs {
                id: None,
                name: "Aillio Bullet".into(),
                make: None,
                source_pin_json: None,
            })
            .unwrap();

        let listed = store.list_machines().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed[0].name, "Aillio Bullet",
            "list is name-ordered (case-insensitive)"
        );
        assert_eq!(listed[1].name, "SF-25 Roaster");

        // Archive hides from the list but never hard-deletes.
        store.archive_machine(created.id).unwrap();
        let listed = store.list_machines().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Aillio Bullet");

        // Empty name and unknown id are rejected.
        let err = store
            .save_machine(&SaveMachineArgs {
                id: None,
                name: "   ".into(),
                make: None,
                source_pin_json: None,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgs);
        let err = store.archive_machine(9999).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgs);
        let err = store
            .save_machine(&SaveMachineArgs {
                id: Some(9999),
                name: "X".into(),
                make: None,
                source_pin_json: None,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgs);
    }

    #[test]
    fn coffees_crud_roundtrip_and_archive() {
        let store = Store::open(&test_db_path("lib-coffees")).unwrap();

        let guji = store
            .save_coffee(&SaveCoffeeArgs {
                id: None,
                name: "Ethiopia Guji".into(),
                origin: Some("Ethiopia".into()),
                process: Some("washed".into()),
                notes: Some("floral".into()),
            })
            .unwrap();
        assert_eq!(guji.origin.as_deref(), Some("Ethiopia"));
        assert!(!guji.archived);

        store
            .save_coffee(&SaveCoffeeArgs {
                id: None,
                name: "Colombia".into(),
                origin: None,
                process: None,
                notes: None,
            })
            .unwrap();

        let listed = store.list_coffees().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "Colombia");
        assert_eq!(listed[1].name, "Ethiopia Guji");

        // Update by id.
        let updated = store
            .save_coffee(&SaveCoffeeArgs {
                id: Some(guji.id),
                name: "Ethiopia Guji — Hambela".into(),
                origin: Some("Ethiopia".into()),
                process: Some("natural".into()),
                notes: None,
            })
            .unwrap();
        assert_eq!(updated.process.as_deref(), Some("natural"));
        assert_eq!(updated.notes, None);

        store.archive_coffee(guji.id).unwrap();
        let listed = store.list_coffees().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Colombia");

        let err = store
            .save_coffee(&SaveCoffeeArgs {
                id: None,
                name: "".into(),
                origin: None,
                process: None,
                notes: None,
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgs);
    }

    #[test]
    fn update_roast_patch_clear_vs_absent() {
        let store = Store::open(&test_db_path("lib-update-roast")).unwrap();
        let uuid = "roast-meta";
        store
            .create_roast(
                uuid,
                "replay:ethiopia-guji",
                1,
                &SessionMetaDto {
                    coffee_name: Some("Original".into()),
                    charge_weight_lb: Some(24.0),
                    ..Default::default()
                },
            )
            .unwrap();

        // Patch notes only; coffee_name is absent → unchanged.
        let patch = serde_json::from_str::<RoastPatchDto>(r#"{ "notes": "first edit" }"#).unwrap();
        let summary = store.update_roast(uuid, &patch).unwrap();
        assert_eq!(summary.notes.as_deref(), Some("first edit"));
        assert_eq!(summary.coffee_name.as_deref(), Some("Original"));
        assert_eq!(summary.charge_weight_lb, Some(24.0));

        // Explicit null clears coffee_name; notes absent → still "first edit".
        let patch = serde_json::from_str::<RoastPatchDto>(r#"{ "coffeeName": null }"#).unwrap();
        let summary = store.update_roast(uuid, &patch).unwrap();
        assert_eq!(summary.coffee_name, None);
        assert_eq!(summary.notes.as_deref(), Some("first edit"));

        // Unknown roast → no_such_roast.
        let err = store
            .update_roast("nope", &RoastPatchDto::default())
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NoSuchRoast);
    }

    #[test]
    fn update_roast_coffee_local_denormalizes_name() {
        let store = Store::open(&test_db_path("lib-coffee-denorm")).unwrap();
        let coffee = store
            .save_coffee(&SaveCoffeeArgs {
                id: None,
                name: "Guji".into(),
                origin: None,
                process: None,
                notes: None,
            })
            .unwrap();
        let uuid = "roast-link";
        store
            .create_roast(uuid, "replay:x", 1, &SessionMetaDto::default())
            .unwrap();

        // Linking a coffee copies its name into the denormalized column, even when
        // the patch also carries a stale coffeeName.
        let patch = RoastPatchDto {
            coffee_name: Some(Some("stale name".into())),
            coffee_local_id: Some(Some(coffee.id)),
            ..Default::default()
        };
        let summary = store.update_roast(uuid, &patch).unwrap();
        assert_eq!(summary.coffee_name.as_deref(), Some("Guji"));

        // Linking an unknown coffee id is rejected.
        let patch = RoastPatchDto {
            coffee_local_id: Some(Some(9999)),
            ..Default::default()
        };
        let err = store.update_roast(uuid, &patch).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgs);
    }

    #[test]
    fn update_roast_markers_recomputes_temps_appends_events_and_dtr() {
        let store = Store::open(&test_db_path("lib-markers")).unwrap();
        let uuid = "roast-edit";
        finished_roast(&store, uuid);

        // Edit drop to 280s: temp re-pins from the nearest sample and dtr follows.
        let patch = serde_json::from_str::<MarkerPatchDto>(r#"{ "dropSec": 280 }"#).unwrap();
        let markers = store.update_roast_markers(uuid, &patch).unwrap();
        assert_eq!(markers.drop_sec, Some(280.0));
        assert_eq!(
            markers.drop_temp_f,
            Some(bt_at(280.0)),
            "drop temp recomputed from samples"
        );

        let roast = store.get_roast(uuid).unwrap();
        // dtr = (280 - 250) / 280 → 0.107
        assert_eq!(roast.summary.dtr, Some(0.107));
        // An "edited" drop event was appended at charge_origin + 280 (= 280).
        let edited = roast
            .events
            .iter()
            .find(|e| e.kind == K::Drop && e.note.as_deref() == Some("edited"))
            .expect("edited drop event");
        assert_eq!(edited.session_sec, 280.0);
        // Exactly one drop event survives (the original tap was replaced).
        assert_eq!(roast.events.iter().filter(|e| e.kind == K::Drop).count(), 1);

        // Edit turning point: its temp recomputes too (auto-detected TP had no event
        // row, so this must not have been clobbered by a re-canonicalize).
        let patch = serde_json::from_str::<MarkerPatchDto>(r#"{ "turningPointSec": 55 }"#).unwrap();
        let markers = store.update_roast_markers(uuid, &patch).unwrap();
        assert_eq!(markers.turning_point_sec, Some(55.0));
        assert_eq!(markers.turning_point_temp_f, Some(bt_at(55.0)));

        // Clearing dry_end nulls the column.
        let patch = serde_json::from_str::<MarkerPatchDto>(r#"{ "dryEndSec": null }"#).unwrap();
        let markers = store.update_roast_markers(uuid, &patch).unwrap();
        assert_eq!(markers.dry_end_sec, None);
        // Other markers are untouched by the clear.
        assert_eq!(markers.drop_sec, Some(280.0));
        assert_eq!(markers.turning_point_sec, Some(55.0));
    }

    #[test]
    fn update_roast_markers_rejects_unfinished_and_missing() {
        let store = Store::open(&test_db_path("lib-markers-guard")).unwrap();
        let uuid = "roast-recording";
        store
            .create_roast(uuid, "replay:x", 1, &SessionMetaDto::default())
            .unwrap();
        store.append_sample(sample(uuid, 1, 0.0, 300.0)).unwrap();

        let patch = MarkerPatchDto {
            drop_sec: Some(Some(120.0)),
            ..Default::default()
        };
        let err = store.update_roast_markers(uuid, &patch).unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::InvalidArgs,
            "recording roasts are not editable"
        );

        let err = store.update_roast_markers("nope", &patch).unwrap_err();
        assert_eq!(err.code, ErrorCode::NoSuchRoast);
    }

    #[test]
    fn delete_roast_cascades_all_rows() {
        let store = Store::open(&test_db_path("lib-delete")).unwrap();
        let uuid = "roast-junk";
        finished_roast(&store, uuid); // finish enqueues a 'finalize' outbox row

        let counts = |store: &Store| -> (i64, i64, i64, i64) {
            store
                .with_conn(|conn| {
                    let roasts: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM roasts WHERE uuid = 'roast-junk'",
                        [],
                        |r| r.get(0),
                    )?;
                    let samples: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM samples WHERE roast_uuid = 'roast-junk'",
                        [],
                        |r| r.get(0),
                    )?;
                    let events: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM events WHERE roast_uuid = 'roast-junk'",
                        [],
                        |r| r.get(0),
                    )?;
                    let outbox: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM outbox WHERE roast_uuid = 'roast-junk'",
                        [],
                        |r| r.get(0),
                    )?;
                    Ok((roasts, samples, events, outbox))
                })
                .unwrap()
        };

        let (r, s, e, o) = counts(&store);
        assert_eq!(r, 1);
        assert_eq!(s, 300);
        assert!(e >= 4, "charge/dry_end/fc_start/drop events present");
        assert_eq!(o, 1, "finalize outbox row present");

        store.delete_roast(uuid).unwrap();

        let (r, s, e, o) = counts(&store);
        assert_eq!(
            (r, s, e, o),
            (0, 0, 0, 0),
            "roast + samples + events + outbox all gone"
        );

        // Deleting a missing roast is a clean no_such_roast.
        let err = store.delete_roast(uuid).unwrap_err();
        assert_eq!(err.code, ErrorCode::NoSuchRoast);

        // list_roasts no longer shows it.
        assert!(store
            .list_roasts()
            .unwrap()
            .iter()
            .all(|r| r.roast_uuid != uuid));
    }
}
