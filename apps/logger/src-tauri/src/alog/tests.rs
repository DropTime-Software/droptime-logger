//! Tests for `.alog`/CSV import + `.alog`/CSV/JSON export.
//!
//! Fixtures are hand-authored from our documented key map (see `PROVENANCE.md`)
//! — never copied from Artisan source or files.

use crate::model::{ExportFormat, ExportRoastArgs, RoastEventKind as K, SampleDto, SessionMetaDto};
use crate::store::{test_db_path, Store};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn scratch_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("droptime-alog-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn write_file(name: &str, contents: &str) -> String {
    let path = scratch_path(name);
    std::fs::write(&path, contents).unwrap();
    path.to_string_lossy().into_owned()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn close_opt(a: Option<f64>, b: f64) -> bool {
    a.map(|v| close(v, b)).unwrap_or(false)
}

/// A clean 12-point washed profile in °F with pre-charge data. Charge at
/// timex[1] (t=30), dry_end at timex[6] (180), fc_start at timex[8] (300),
/// drop at timex[10] (420). ET = BT + 20; ambient 70°F; weight 24 → 20.4 lb.
const CLEAN_ALOG: &str = "{'mode': 'F', \
'timex': [0.0, 30.0, 60.0, 90.0, 120.0, 150.0, 180.0, 240.0, 300.0, 360.0, 420.0, 480.0], \
'temp1': [420.0, 405.0, 320.0, 250.0, 230.0, 235.0, 245.0, 280.0, 320.0, 360.0, 400.0, 420.0], \
'temp2': [400.0, 385.0, 300.0, 230.0, 210.0, 215.0, 225.0, 260.0, 300.0, 340.0, 380.0, 400.0], \
'timeindex': [1, 6, 8, -1, -1, -1, 10, -1], \
'title': 'Morning Roast', 'beans': 'Ethiopia Guji', \
'weight': [24.0, 20.4, 'lb'], \
'roastisodate': '2024-03-15', 'roasttime': '09:30:00', \
'roastingnotes': 'Bright and floral.', 'ambientTemp': 70.0}";

/// °C mode, Kg weights, ET omitted, dry_end written as 0 (unset for a
/// non-charge slot), charge written as 0 (a legitimate index-0 charge).
const CELSIUS_ALOG: &str = "{'mode': 'C', \
'timex': [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 300.0, 360.0], \
'temp2': [195.0, 100.0, 95.0, 120.0, 150.0, 175.0, 195.0, 205.0], \
'timeindex': [0, 0, 5, -1, -1, -1, 7, -1], \
'beans': 'Colombia Decaf', 'weight': [10.0, 8.5, 'Kg']}";

// ---------------------------------------------------------------------------
// .alog import
// ---------------------------------------------------------------------------

#[test]
fn clean_alog_fixture_parses_and_imports() {
    let path = write_file("guji.alog", CLEAN_ALOG);
    let store = Store::open(&test_db_path("import-clean")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();

    let result = super::import(&store, &device, &[path]).unwrap();
    assert_eq!(result.imported, 1);
    assert_eq!(result.roast_uuids.len(), 1);
    assert!(result.failed.is_empty());

    let roast = store.get_roast(&result.roast_uuids[0]).unwrap();
    let s = &roast.summary;

    // metadata
    assert_eq!(s.coffee_name.as_deref(), Some("Ethiopia Guji")); // beans wins over title
    assert!(close_opt(s.charge_weight_lb, 24.0));
    assert!(close_opt(s.drop_weight_lb, 20.4));
    assert!(close_opt(s.weight_loss_pct, 15.0));
    assert_eq!(s.notes.as_deref(), Some("Bright and floral."));
    assert_eq!(
        super::iso_date_time(s.started_wall_ms),
        ("2024-03-15".into(), "09:30:00".into())
    );

    // samples: 12 kept, rebased so the first sits at 0, second (charge) at 30.
    assert_eq!(roast.samples.len(), 12);
    assert!(close(roast.samples[0].session_sec, 0.0));
    assert!(close(roast.samples[1].session_sec, 30.0));
    assert!(close_opt(roast.samples[0].et_f, 420.0)); // ET carried through

    // canonical markers (seconds FROM CHARGE, not from recording start).
    assert!(close_opt(s.markers.dry_end_sec, 150.0)); // 180 - 30
    assert!(close_opt(s.markers.fc_start_sec, 270.0)); // 300 - 30
    assert!(close_opt(s.markers.drop_sec, 390.0)); // 420 - 30
    assert!(close_opt(s.markers.charge_temp_f, 385.0));
    assert!(close_opt(s.markers.drop_temp_f, 380.0));
    assert!(close_opt(s.markers.turning_point_sec, 90.0)); // low at t=120 → 120-30
    assert!(close_opt(s.markers.turning_point_temp_f, 210.0));
    assert_eq!(s.dtr, Some(0.308)); // (390 - 270) / 390

    // The charge tap survives on the raw event history at its absolute time.
    let charge = roast
        .events
        .iter()
        .find(|e| e.kind == K::Charge)
        .expect("charge event");
    assert!(close(charge.session_sec, 30.0));

    // Imported roasts are finished; it also shows up in the history list.
    assert_eq!(s.status, crate::model::RoastStatus::Finished);
    assert!(store
        .list_roasts()
        .unwrap()
        .iter()
        .any(|r| r.roast_uuid == s.roast_uuid));
}

#[test]
fn celsius_kg_and_unset_markers() {
    let path = write_file("decaf.alog", CELSIUS_ALOG);
    let store = Store::open(&test_db_path("import-celsius")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();
    let result = super::import(&store, &device, &[path]).unwrap();
    let roast = store.get_roast(&result.roast_uuids[0]).unwrap();
    let s = &roast.summary;

    assert_eq!(s.coffee_name.as_deref(), Some("Colombia Decaf"));
    // Kg → lb.
    assert!(close_opt(s.charge_weight_lb, 10.0 * 2.2046226218487757));
    assert!(close_opt(s.drop_weight_lb, 8.5 * 2.2046226218487757));

    // °C → °F on every temperature.
    assert_eq!(roast.samples.len(), 8);
    assert!(close(roast.samples[0].bt_f, 383.0)); // 195°C
    assert!(roast.samples.iter().all(|smp| smp.et_f.is_none())); // temp1 omitted

    // Markers: charge at index 0 is real; dry_end written as 0 is UNSET.
    assert!(s.markers.dry_end_sec.is_none());
    assert!(close_opt(s.markers.fc_start_sec, 240.0));
    assert!(close_opt(s.markers.drop_sec, 360.0));
    assert!(close_opt(s.markers.charge_temp_f, 383.0));
    assert!(close_opt(s.markers.drop_temp_f, 401.0)); // 205°C
    assert!(close_opt(s.markers.turning_point_sec, 60.0)); // low 95°C at t=60
    assert!(close_opt(s.markers.turning_point_temp_f, 203.0)); // 95°C

    // Charge at index 0 → charge event at absolute session 0.
    let charge = roast
        .events
        .iter()
        .find(|e| e.kind == K::Charge)
        .expect("charge");
    assert!(close(charge.session_sec, 0.0));
}

#[test]
fn mode_absent_auto_detects_celsius_by_range() {
    // No 'mode' key: a sub-250 BT series must be read as °C and converted.
    let alog = "{'timex': [0.0, 30.0, 60.0, 90.0], 'temp2': [200.0, 150.0, 145.0, 160.0], \
                 'timeindex': [0, -1, -1, -1, -1, -1, -1, -1]}";
    let path = write_file("nomode.alog", alog);
    let store = Store::open(&test_db_path("import-nomode")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();
    let result = super::import(&store, &device, &[path]).unwrap();
    let roast = store.get_roast(&result.roast_uuids[0]).unwrap();
    assert!(close(roast.samples[0].bt_f, 392.0)); // 200°C → 392°F
}

#[test]
fn dropout_sentinels_are_dropped_and_et_nulled() {
    // temp2 == -1 → sample skipped; temp1 == -1 → that sample's ET is null.
    let alog = "{'mode': 'F', 'timex': [0.0, 1.0, 2.0, 3.0], \
                 'temp1': [-1.0, 300.0, -1.0, 320.0], \
                 'temp2': [-1.0, 385.0, 390.0, 395.0], \
                 'timeindex': [0, -1, -1, -1, -1, -1, -1, -1]}";
    let path = write_file("dropout.alog", alog);
    let store = Store::open(&test_db_path("import-dropout")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();
    let result = super::import(&store, &device, &[path]).unwrap();
    let roast = store.get_roast(&result.roast_uuids[0]).unwrap();

    // The first row (BT -1) is gone; three samples remain, reseq'd from 1.
    assert_eq!(roast.samples.len(), 3);
    assert_eq!(roast.samples[0].seq, 1);
    assert!(close(roast.samples[0].bt_f, 385.0));
    assert!(close_opt(roast.samples[0].et_f, 300.0));
    assert!(roast.samples[1].et_f.is_none()); // temp1 -1 → null ET
}

#[test]
fn malformed_files_error_without_panicking() {
    let store = Store::open(&test_db_path("import-malformed")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();

    let junk = write_file("junk.alog", "this is not a python literal :(");
    let not_dict = write_file("list.alog", "[1, 2, 3]");
    let no_timex = write_file("empty.alog", "{'mode': 'F', 'temp2': [1.0]}");

    // preview never writes and reports each failure inline.
    let previews = super::preview(&[junk.clone(), not_dict.clone(), no_timex.clone()]).unwrap();
    assert!(previews.iter().all(|p| !p.ok && p.error.is_some()));

    // import surfaces them as per-file failures, importing nothing.
    let result = super::import(&store, &device, &[junk, not_dict, no_timex]).unwrap();
    assert_eq!(result.imported, 0);
    assert_eq!(result.failed.len(), 3);
    assert!(store.list_roasts().unwrap().is_empty());
}

#[test]
fn missing_file_is_an_error_not_a_panic() {
    let previews = super::preview(&["/no/such/file.alog".to_string()]).unwrap();
    assert_eq!(previews.len(), 1);
    assert!(!previews[0].ok);
    assert!(previews[0].error.is_some());
}

#[test]
fn rebase_shifts_first_sample_to_zero_and_pins_charge() {
    // A timex that does NOT start at zero must rebase so the first sample is 0
    // and the charge sits at charge_session_sec (index 1 here → 40 - 10 = 30).
    let alog = "{'mode': 'F', 'timex': [10.0, 40.0, 70.0, 100.0], \
                 'temp2': [300.0, 250.0, 240.0, 260.0], \
                 'timeindex': [1, -1, -1, -1, -1, -1, -1, -1]}";
    let path = write_file("shift.alog", alog);
    let store = Store::open(&test_db_path("import-rebase")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();
    let result = super::import(&store, &device, &[path]).unwrap();
    let roast = store.get_roast(&result.roast_uuids[0]).unwrap();

    assert!(close(roast.samples[0].session_sec, 0.0));
    assert!(close(roast.samples[3].session_sec, 90.0));
    let charge = roast
        .events
        .iter()
        .find(|e| e.kind == K::Charge)
        .expect("charge");
    assert!(close(charge.session_sec, 30.0)); // 40 - 10
}

#[test]
fn preview_markers_match_the_committed_roast() {
    let path = write_file("guji.alog", CLEAN_ALOG);
    let previews = super::preview(&[path.clone()]).unwrap();
    let preview = &previews[0];
    assert!(preview.ok);
    assert_eq!(preview.sample_count, Some(12));
    assert_eq!(preview.coffee_name.as_deref(), Some("Ethiopia Guji"));
    assert!(close_opt(preview.duration_sec, 390.0)); // charge → drop

    let store = Store::open(&test_db_path("preview-parity")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();
    let result = super::import(&store, &device, &[path]).unwrap();
    let committed = store
        .get_roast(&result.roast_uuids[0])
        .unwrap()
        .summary
        .markers;
    let pm = preview.markers.expect("preview markers");

    assert_eq!(pm.dry_end_sec, committed.dry_end_sec);
    assert_eq!(pm.fc_start_sec, committed.fc_start_sec);
    assert_eq!(pm.drop_sec, committed.drop_sec);
    assert_eq!(pm.turning_point_sec, committed.turning_point_sec);
    assert_eq!(pm.charge_temp_f, committed.charge_temp_f);
    assert_eq!(pm.drop_temp_f, committed.drop_temp_f);
    assert_eq!(pm.turning_point_temp_f, committed.turning_point_temp_f);
}

#[test]
fn batch_import_mixes_success_and_failure() {
    let good = write_file("good.alog", CLEAN_ALOG);
    let bad = write_file("bad.alog", "nonsense{");
    let store = Store::open(&test_db_path("import-batch")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();
    let result = super::import(&store, &device, &[good, bad.clone()]).unwrap();
    assert_eq!(result.imported, 1);
    assert_eq!(result.roast_uuids.len(), 1);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].path, bad);
}

// ---------------------------------------------------------------------------
// CSV import
// ---------------------------------------------------------------------------

#[test]
fn csv_comma_header_fahrenheit() {
    let csv = "time,bt,et\n0,400,420\n30,300,320\n60,250,270\n90,240,260\n120,260,280\n";
    let samples = super::csv_fmt::parse_samples(csv).unwrap();
    assert_eq!(samples.len(), 5);
    assert!(close(samples[0].session_sec, 0.0));
    assert!(close(samples[1].session_sec, 30.0));
    assert!(close(samples[0].bt_f, 400.0)); // already °F → unchanged
    assert!(close_opt(samples[0].et_f, 420.0));
}

#[test]
fn csv_semicolon_mmss_celsius() {
    // Semicolon-delimited, mm:ss time, sub-250 BT → detected as °C.
    let csv = "time;bt\n0:00;200\n0:30;150\n1:00;145\n1:30;160\n";
    let samples = super::csv_fmt::parse_samples(csv).unwrap();
    assert_eq!(samples.len(), 4);
    assert!(close(samples[1].session_sec, 30.0)); // 0:30
    assert!(close(samples[2].session_sec, 60.0)); // 1:00
    assert!(close(samples[0].bt_f, 392.0)); // 200°C → 392°F
    assert!(samples[0].et_f.is_none());
}

#[test]
fn csv_tab_headerless_positional() {
    let csv = "0\t400\t420\n30\t300\t320\n60\t250\t270\n";
    let samples = super::csv_fmt::parse_samples(csv).unwrap();
    assert_eq!(samples.len(), 3);
    assert!(close(samples[2].session_sec, 60.0));
    assert!(close(samples[1].bt_f, 300.0));
    assert!(close_opt(samples[1].et_f, 320.0));
}

#[test]
fn csv_import_commits_with_filename_as_coffee() {
    let csv = "time,bt,et\n0,400,420\n30,300,320\n60,250,270\n";
    let path = write_file("Kenya AA.csv", csv);
    let store = Store::open(&test_db_path("import-csv")).unwrap();
    let device = store.get_setting("device_id").unwrap().unwrap();
    let result = super::import(&store, &device, &[path]).unwrap();
    let roast = store.get_roast(&result.roast_uuids[0]).unwrap();
    assert_eq!(roast.samples.len(), 3);
    assert_eq!(roast.summary.coffee_name.as_deref(), Some("Kenya AA"));
    // CSV has no markers → no charge; drop/fc are None.
    assert!(roast.summary.markers.drop_sec.is_none());
}

#[test]
fn csv_malformed_reports_error() {
    let previews = super::preview(&[write_file("bad.csv", "header only\n")]).unwrap();
    assert!(!previews[0].ok);
    assert!(previews[0].error.is_some());
}

// ---------------------------------------------------------------------------
// export + round-trip
// ---------------------------------------------------------------------------

fn bt_at(session_sec: f64) -> f64 {
    if session_sec <= 60.0 {
        380.0 - 3.5 * session_sec // 380 → 170
    } else {
        170.0 + 0.55 * (session_sec - 60.0)
    }
}

/// A finished, live-style roast: charge at 0, drop at 299, weights + ET set.
fn finished_roast(store: &Store, uuid: &str) {
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
        store
            .append_sample(crate::store::SampleRow {
                roast_uuid: uuid.into(),
                sample: SampleDto {
                    seq,
                    session_sec: t,
                    bt_f: bt_at(t),
                    et_f: Some(bt_at(t) + 30.0),
                    ambient_f: None,
                    heater: None,
                    fan: None,
                    drum: None,
                },
            })
            .unwrap();
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
fn json_export_is_verbatim_get_roast() {
    let store = Store::open(&test_db_path("export-json")).unwrap();
    finished_roast(&store, "r-json");
    let dest = scratch_path("r.json").to_string_lossy().into_owned();
    let out = super::export(
        &store,
        &ExportRoastArgs {
            roast_uuid: "r-json".into(),
            format: ExportFormat::Json,
            dest_path: dest.clone(),
        },
    )
    .unwrap();
    assert_eq!(out.path, dest);

    let written = std::fs::read_to_string(&dest).unwrap();
    let expected = serde_json::to_string_pretty(&store.get_roast("r-json").unwrap()).unwrap();
    assert_eq!(written, expected);
    // sanity: it round-trips through serde as the same shape.
    let reparsed: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(reparsed["summary"]["dropSec"], 299.0);
}

#[test]
fn csv_export_has_meta_header_and_rows() {
    let store = Store::open(&test_db_path("export-csv")).unwrap();
    finished_roast(&store, "r-csv");
    let dest = scratch_path("r.csv").to_string_lossy().into_owned();
    super::export(
        &store,
        &ExportRoastArgs {
            roast_uuid: "r-csv".into(),
            format: ExportFormat::Csv,
            dest_path: dest.clone(),
        },
    )
    .unwrap();
    let text = std::fs::read_to_string(&dest).unwrap();
    assert!(text.contains("# coffee: Ethiopia Guji"));
    assert!(text.contains("time,bt,et,ror"));
    let data_lines = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("time,"))
        .count();
    assert_eq!(data_lines, 300);
}

#[test]
fn alog_export_writes_openable_dict_with_unset_minus_one() {
    let store = Store::open(&test_db_path("export-alog")).unwrap();
    finished_roast(&store, "r-alog");
    let dest = scratch_path("r.alog").to_string_lossy().into_owned();
    super::export(
        &store,
        &ExportRoastArgs {
            roast_uuid: "r-alog".into(),
            format: ExportFormat::Alog,
            dest_path: dest.clone(),
        },
    )
    .unwrap();
    let text = std::fs::read_to_string(&dest).unwrap();
    // It re-parses as a Python dict...
    let value: py_literal::Value = text
        .trim()
        .parse()
        .expect("export is a valid python literal");
    let dict = value.as_dict().expect("dict");
    // ...with mode F and an 8-slot timeindex whose unset slots are -1.
    assert_eq!(
        super::pyval::dict_get(dict, "mode")
            .and_then(|v| v.as_string())
            .map(|s| s.as_str()),
        Some("F")
    );
    let ti = super::pyval::int_vec(super::pyval::dict_get(dict, "timeindex").unwrap()).unwrap();
    assert_eq!(ti.len(), 8);
    assert_eq!(ti[3], -1); // fc_end unset
    assert_eq!(ti[7], -1); // cool_end unset
    assert!(ti[0] >= 0 && ti[6] >= 0); // charge + drop are set
}

#[test]
fn alog_round_trips_markers_and_samples() {
    let store = Store::open(&test_db_path("roundtrip")).unwrap();
    finished_roast(&store, "orig");
    let device = store.get_setting("device_id").unwrap().unwrap();

    let dest = scratch_path("roundtrip.alog")
        .to_string_lossy()
        .into_owned();
    super::export(
        &store,
        &ExportRoastArgs {
            roast_uuid: "orig".into(),
            format: ExportFormat::Alog,
            dest_path: dest.clone(),
        },
    )
    .unwrap();

    let result = super::import(&store, &device, &[dest]).unwrap();
    assert_eq!(result.imported, 1);

    let orig = store.get_roast("orig").unwrap();
    let back = store.get_roast(&result.roast_uuids[0]).unwrap();

    // Markers survive the trip exactly.
    let (om, bm) = (orig.summary.markers, back.summary.markers);
    assert_eq!(bm.dry_end_sec, om.dry_end_sec);
    assert_eq!(bm.fc_start_sec, om.fc_start_sec);
    assert_eq!(bm.drop_sec, om.drop_sec);
    assert_eq!(bm.drop_temp_f, om.drop_temp_f);
    assert_eq!(bm.charge_temp_f, om.charge_temp_f);
    assert_eq!(bm.turning_point_sec, om.turning_point_sec);
    assert_eq!(bm.turning_point_temp_f, om.turning_point_temp_f);
    // Derived stats too.
    assert_eq!(back.summary.dtr, orig.summary.dtr);
    assert_eq!(back.summary.coffee_name, orig.summary.coffee_name);
    assert!(close_opt(back.summary.charge_weight_lb, 24.0));
    assert!(close_opt(back.summary.drop_weight_lb, 20.4));
    // Whole-second wall time round-trips through roastisodate/roasttime.
    assert_eq!(back.summary.started_wall_ms, orig.summary.started_wall_ms);

    // Samples: same count, and each (session_sec, bt, et) matches.
    assert_eq!(back.samples.len(), orig.samples.len());
    for (a, b) in orig.samples.iter().zip(&back.samples) {
        assert!(close(a.session_sec, b.session_sec));
        assert!(close(a.bt_f, b.bt_f));
        assert_eq!(a.et_f.is_some(), b.et_f.is_some());
        if let (Some(x), Some(y)) = (a.et_f, b.et_f) {
            assert!(close(x, y));
        }
    }
}

// ---------------------------------------------------------------------------
// unit helpers
// ---------------------------------------------------------------------------

#[test]
fn weight_conversions() {
    assert!(close(super::weight_to_lb(453.59237, "g"), 1.0));
    assert!(close(super::weight_to_lb(1.0, "Kg"), 2.2046226218487757));
    assert!(close(super::weight_to_lb(16.0, "oz"), 1.0));
    assert!(close(super::weight_to_lb(5.0, "lb"), 5.0));
    assert!(close(super::weight_to_lb(5.0, "unknown"), 5.0)); // treated as lb
}

#[test]
fn date_helpers_round_trip() {
    // A known instant: 2024-03-15 09:30:00 UTC.
    let ms = super::epoch_ms_from_iso("2024-03-15", Some("09:30:00")).unwrap();
    assert_eq!(
        super::iso_date_time(ms),
        ("2024-03-15".into(), "09:30:00".into())
    );
    // Time defaults to midnight when absent.
    let ms0 = super::epoch_ms_from_iso("2024-03-15", None).unwrap();
    assert_eq!(
        super::iso_date_time(ms0),
        ("2024-03-15".into(), "00:00:00".into())
    );
    // Garbage date → None (caller falls back to file mtime).
    assert!(super::epoch_ms_from_iso("not-a-date", None).is_none());
}

#[test]
fn py_int_handles_negatives() {
    // The writer builds ints (incl. -1) without naming num_bigint.
    let v = super::pyval::py_int(-1);
    assert_eq!(v.format_ascii().unwrap(), "-1");
    assert_eq!(super::pyval::py_int(42).format_ascii().unwrap(), "42");
}
