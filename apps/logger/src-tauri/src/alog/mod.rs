//! alog/ — Artisan `.alog` + CSV import/export (CONTRACTS.md §7.1).
//!
//! Owner: importer. Provenance rules are BINDING — see `PROVENANCE.md` in this
//! directory: the parser derives from our documented key map + self-generated
//! fixtures, never from Artisan source code.
//!
//! Formats (frozen):
//! - `.alog`: Python-literal parse/write via the `py_literal` crate.
//! - CSV import: header-sniffed `time,bt[,et]` (comma/tab/semicolon);
//!   temperatures auto-detected °C vs °F by range and converted to °F.
//! - JSON export: the `GetRoastResult` shape verbatim.
//!
//! `.alog`/CSV imports become `ImportedRoast` values (defined here so the store's
//! private `imports` submodule can name them) that `Store::insert_imported_roast`
//! persists.

mod alog_fmt;
mod csv_fmt;
mod pyval;

#[cfg(test)]
mod tests;

use std::path::Path;

use crate::error::LoggerError;
use crate::model::{
    EventDto, ExportFormat, ExportResultDto, ExportRoastArgs, ImportFailureDto, ImportPreviewDto,
    ImportResultDto, RoastEventKind, RoastMarkersDto, SampleDto,
};
use crate::store::Store;

/// A fully-parsed roast ready to persist (see `store::imports`).
///
/// Samples are already rebased so `session_sec = t + |min(t, 0)|` (first sample
/// at 0, charge at `charge_session_sec`); `events` carry session_sec on the same
/// axis. `seq` is contiguous from 1.
#[derive(Debug, Clone)]
pub struct ImportedRoast {
    /// This install's device id. When empty, `insert_imported_roast` mints/reads
    /// it from settings.
    pub device_id: String,
    /// `import:alog` | `import:csv` (the roasts.source_id column).
    pub source_id: String,
    /// `alog` | `csv` (the roasts.imported_from column).
    pub imported_from: String,
    /// From the file when recoverable, else the file's mtime.
    pub started_wall_ms: i64,
    pub coffee_name: Option<String>,
    pub charge_weight_lb: Option<f64>,
    pub drop_weight_lb: Option<f64>,
    pub notes: Option<String>,
    pub samples: Vec<SampleDto>,
    pub events: Vec<EventDto>,
}

/// The 8 `timeindex` slots, in Artisan order.
pub(super) const TIMEINDEX_KINDS: [RoastEventKind; 8] = [
    RoastEventKind::Charge,
    RoastEventKind::DryEnd,
    RoastEventKind::FcStart,
    RoastEventKind::FcEnd,
    RoastEventKind::ScStart,
    RoastEventKind::ScEnd,
    RoastEventKind::Drop,
    RoastEventKind::CoolEnd,
];

// ---------------------------------------------------------------------------
// public API (called by ipc.rs)
// ---------------------------------------------------------------------------

/// Parse-only preview of importable files — NEVER writes (§7.1). One
/// `ImportPreviewDto` per input path, `ok: false` + `error` on parse failure.
pub fn preview(paths: &[String]) -> Result<Vec<ImportPreviewDto>, LoggerError> {
    Ok(paths.iter().map(|p| preview_one(p)).collect())
}

fn preview_one(path: &str) -> ImportPreviewDto {
    match parse_file(path) {
        Ok(roast) => {
            let markers = compute_markers(&roast.samples, &roast.events);
            ImportPreviewDto {
                path: path.to_string(),
                ok: true,
                error: None,
                coffee_name: roast.coffee_name.clone(),
                started_wall_ms: Some(roast.started_wall_ms),
                duration_sec: Some(duration_sec(&roast.samples, &markers)),
                sample_count: Some(roast.samples.len() as u64),
                markers: Some(markers),
            }
        }
        Err(err) => ImportPreviewDto {
            path: path.to_string(),
            ok: false,
            error: Some(err.message),
            coffee_name: None,
            started_wall_ms: None,
            duration_sec: None,
            sample_count: None,
            markers: None,
        },
    }
}

/// Re-parse and commit files via `Store::insert_imported_roast` (§7.1).
/// `device_id` is this install's id. Per-file failures land in
/// `ImportResultDto.failed`; only a systemic error returns `Err`.
pub fn import(
    store: &Store,
    device_id: &str,
    paths: &[String],
) -> Result<ImportResultDto, LoggerError> {
    let mut roast_uuids = Vec::new();
    let mut failed = Vec::new();
    for path in paths {
        match parse_file(path) {
            Ok(mut roast) => {
                roast.device_id = device_id.to_string();
                match store.insert_imported_roast(&roast) {
                    Ok(uuid) => roast_uuids.push(uuid),
                    Err(err) => failed.push(ImportFailureDto {
                        path: path.clone(),
                        error: err.message,
                    }),
                }
            }
            Err(err) => failed.push(ImportFailureDto {
                path: path.clone(),
                error: err.message,
            }),
        }
    }
    Ok(ImportResultDto {
        imported: roast_uuids.len() as u64,
        roast_uuids,
        failed,
    })
}

/// Export one roast to `args.dest_path` in `args.format` (§7.1). `destPath` is
/// chosen by the frontend via the dialog plugin.
pub fn export(store: &Store, args: &ExportRoastArgs) -> Result<ExportResultDto, LoggerError> {
    let roast = store.get_roast(&args.roast_uuid)?;
    let contents = match args.format {
        ExportFormat::Alog => alog_fmt::write(&roast)?,
        ExportFormat::Csv => csv_fmt::write(&roast),
        ExportFormat::Json => serde_json::to_string_pretty(&roast)
            .map_err(|e| LoggerError::io(format!("serialize roast to JSON: {e}")))?,
    };
    std::fs::write(&args.dest_path, contents)
        .map_err(|e| LoggerError::io(format!("write {}: {e}", args.dest_path)))?;
    Ok(ExportResultDto {
        path: args.dest_path.clone(),
    })
}

// ---------------------------------------------------------------------------
// dispatch
// ---------------------------------------------------------------------------

fn parse_file(path: &str) -> Result<ImportedRoast, LoggerError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "csv" => csv_fmt::parse(path),
        _ => alog_fmt::parse(path), // .alog (and anything else) parses as a Python literal
    }
}

// ---------------------------------------------------------------------------
// shared helpers (visible to the format submodules + tests)
// ---------------------------------------------------------------------------

/// Artisan's "no reading" sentinel: temp values of exactly -1 (and non-finite).
pub(super) fn is_no_reading(v: f64) -> bool {
    !v.is_finite() || (v + 1.0).abs() < 1e-6
}

pub(super) fn c_to_f(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

/// True when a BT series looks like Celsius: even the hottest reading stays
/// below the 250 threshold that separates °C (≤ ~235) from °F (≥ ~350) roasts.
pub(super) fn looks_celsius(max_bt: f64) -> bool {
    max_bt < 250.0
}

pub(super) fn weight_to_lb(value: f64, unit: &str) -> f64 {
    match unit.trim().to_ascii_lowercase().as_str() {
        "g" => value / 453.59237,
        "kg" => value * 2.2046226218487757,
        "oz" => value / 16.0,
        _ => value, // 'lb' and anything unrecognized are treated as pounds
    }
}

/// File modification time in epoch millis, or `now` when unavailable.
pub(super) fn file_mtime_ms(path: &str) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(crate::store::now_ms)
}

/// Days since 1970-01-01 for a proleptic-Gregorian civil date (Hinnant).
pub(super) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Inverse of `days_from_civil` — `(year, month, day)` for a day count.
pub(super) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `(YYYY-MM-DD, HH:MM:SS)` in UTC for an epoch-millis instant.
pub(super) fn iso_date_time(ms: i64) -> (String, String) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    (
        format!("{y:04}-{mo:02}-{d:02}"),
        format!("{h:02}:{mi:02}:{s:02}"),
    )
}

/// Recover epoch millis (UTC) from an ISO date `YYYY-MM-DD` + optional
/// `HH:MM[:SS]` time. Returns `None` when the date does not parse.
pub(super) fn epoch_ms_from_iso(date: &str, time: Option<&str>) -> Option<i64> {
    let mut dp = date.trim().split('-');
    let y: i64 = dp.next()?.trim().parse().ok()?;
    let mo: i64 = dp.next()?.trim().parse().ok()?;
    let d: i64 = dp.next()?.trim().parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let (mut h, mut mi, mut s) = (0i64, 0i64, 0i64);
    if let Some(t) = time {
        let mut tp = t.trim().split(':');
        h = tp.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
        mi = tp.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
        s = tp.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
    }
    Some((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s) * 1000)
}

// ---------------------------------------------------------------------------
// in-memory markers (preview parity with the store's canonicalize + auto-TP)
// ---------------------------------------------------------------------------

/// BT of the sample nearest (in session_sec) to `sec`; earlier sample wins ties.
/// Mirrors `ops::nearest_bt` (`ORDER BY ABS(session_sec - ?), seq`).
pub(super) fn nearest_bt(samples: &[SampleDto], sec: f64) -> Option<f64> {
    samples
        .iter()
        .fold(None::<(f64, f64)>, |best, s| {
            let dist = (s.session_sec - sec).abs();
            match best {
                Some((bd, _)) if bd <= dist => best,
                _ => Some((dist, s.bt_f)),
            }
        })
        .map(|(_, bt)| bt)
}

/// Compute canonical markers from rebased samples + events, exactly as the store
/// would after insert: relative-to-charge seconds, pinned temps, and a
/// turning point from the marked tap or (failing that) the detected curve low.
pub(super) fn compute_markers(samples: &[SampleDto], events: &[EventDto]) -> RoastMarkersDto {
    let latest = |kind: RoastEventKind| {
        events
            .iter()
            .rev()
            .find(|e| e.kind == kind)
            .map(|e| e.session_sec)
    };
    let charge = latest(RoastEventKind::Charge);
    let origin = charge.unwrap_or(0.0);
    let rel = |kind: RoastEventKind| latest(kind).map(|sec| sec - origin);

    let (turning_point_sec, turning_point_temp_f) = match latest(RoastEventKind::TurningPoint) {
        Some(sec) => (Some(sec - origin), nearest_bt(samples, sec)),
        None => {
            let points: Vec<(f64, f64)> = samples
                .iter()
                .map(|s| (s.session_sec - origin, s.bt_f))
                .collect();
            match crate::math::detect_turning_point(&points) {
                Some((t, bt)) => (Some(t), Some(bt)),
                None => (None, None),
            }
        }
    };

    RoastMarkersDto {
        turning_point_sec,
        turning_point_temp_f,
        dry_end_sec: rel(RoastEventKind::DryEnd),
        fc_start_sec: rel(RoastEventKind::FcStart),
        fc_end_sec: rel(RoastEventKind::FcEnd),
        drop_sec: rel(RoastEventKind::Drop),
        drop_temp_f: latest(RoastEventKind::Drop).and_then(|sec| nearest_bt(samples, sec)),
        charge_temp_f: charge.and_then(|sec| nearest_bt(samples, sec)),
    }
}

/// Preview duration: charge→drop when known, else total recording length.
fn duration_sec(samples: &[SampleDto], markers: &RoastMarkersDto) -> f64 {
    if let Some(drop) = markers.drop_sec {
        return drop;
    }
    match (samples.first(), samples.last()) {
        (Some(a), Some(b)) => b.session_sec - a.session_sec,
        _ => 0.0,
    }
}
