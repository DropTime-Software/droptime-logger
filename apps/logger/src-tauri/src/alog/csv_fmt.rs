//! CSV import + export. Import: header-sniffed `time,bt[,et]`
//! (comma/semicolon/tab), `time` in seconds or `mm:ss`, °C/°F auto-detected.
//! Export: `#`-prefixed metadata header then `time,bt,et,ror` rows.

use std::path::Path;

use crate::error::LoggerError;
use crate::model::{GetRoastResult, SampleDto};

use super::ImportedRoast;

const DELIMITERS: [char; 3] = [',', '\t', ';'];

// ---------------------------------------------------------------------------
// parse
// ---------------------------------------------------------------------------

pub fn parse(path: &str) -> Result<ImportedRoast, LoggerError> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| LoggerError::io(format!("read {path}: {e}")))?;
    let samples =
        parse_samples(&raw).map_err(|e| LoggerError::parse(format!("{path}: {}", e.message)))?;
    let coffee_name = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    Ok(ImportedRoast {
        device_id: String::new(),
        source_id: "import:csv".to_string(),
        imported_from: "csv".to_string(),
        started_wall_ms: super::file_mtime_ms(path),
        coffee_name,
        charge_weight_lb: None,
        drop_weight_lb: None,
        notes: None,
        samples,
        events: Vec::new(), // CSV carries no markers
    })
}

/// Parse raw CSV text into rebased samples (seq from 1, first sample at 0).
pub(super) fn parse_samples(raw: &str) -> Result<Vec<SampleDto>, LoggerError> {
    let rows: Vec<Vec<&str>> = raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| split_row(l))
        .collect();
    let first = rows
        .first()
        .ok_or_else(|| LoggerError::parse("empty CSV".to_string()))?;

    // A first row is a header when any cell parses as neither a time nor a float.
    let is_header = first
        .iter()
        .any(|c| parse_time(c).is_none() && c.parse::<f64>().is_err());

    let (time_col, bt_col, et_col, data_start) = if is_header {
        (
            find_col(first, &["time"]).unwrap_or(0),
            find_col(first, &["bt", "bean"]).unwrap_or(1),
            find_col(first, &["et", "environ"]),
            1,
        )
    } else {
        (0, 1, Some(2), 0)
    };

    let mut times = Vec::new();
    let mut bts = Vec::new();
    let mut ets = Vec::new();
    for row in &rows[data_start..] {
        let Some(t) = row.get(time_col).and_then(|c| parse_time(c)) else {
            continue;
        };
        let Some(bt) = row
            .get(bt_col)
            .and_then(|c| c.parse::<f64>().ok())
            .filter(|v| v.is_finite())
        else {
            continue;
        };
        let et = et_col
            .and_then(|ei| row.get(ei))
            .and_then(|c| c.parse::<f64>().ok())
            .filter(|v| v.is_finite() && !super::is_no_reading(*v));
        times.push(t);
        bts.push(bt);
        ets.push(et);
    }
    if times.is_empty() {
        return Err(LoggerError::parse("no numeric data rows".to_string()));
    }

    let max_bt = bts.iter().cloned().fold(f64::MIN, f64::max);
    let is_celsius = super::looks_celsius(max_bt);
    let conv = |v: f64| if is_celsius { super::c_to_f(v) } else { v };
    let origin = times[0];

    Ok((0..times.len())
        .map(|i| SampleDto {
            seq: (i + 1) as u64,
            session_sec: times[i] - origin,
            bt_f: conv(bts[i]),
            et_f: ets[i].map(conv),
            ambient_f: None,
            heater: None,
            fan: None,
            drum: None,
        })
        .collect())
}

/// Split a line on the delimiter that yields the most columns.
fn split_row(line: &str) -> Vec<&str> {
    let delim = DELIMITERS
        .iter()
        .copied()
        .max_by_key(|d| line.matches(*d).count())
        .filter(|d| line.contains(*d))
        .unwrap_or(',');
    line.split(delim).map(|c| c.trim()).collect()
}

fn find_col(header: &[&str], keywords: &[&str]) -> Option<usize> {
    header.iter().position(|cell| {
        let lower = cell.to_ascii_lowercase();
        keywords.iter().any(|k| lower.contains(k))
    })
}

/// Seconds from a `time` cell — plain seconds, or `mm:ss` / `hh:mm:ss`.
fn parse_time(cell: &str) -> Option<f64> {
    let cell = cell.trim();
    if cell.is_empty() {
        return None;
    }
    if cell.contains(':') {
        let nums: Option<Vec<f64>> = cell
            .split(':')
            .map(|p| p.trim().parse::<f64>().ok())
            .collect();
        let nums = nums?;
        return match nums.as_slice() {
            [m, s] => Some(m * 60.0 + s),
            [h, m, s] => Some(h * 3600.0 + m * 60.0 + s),
            _ => None,
        };
    }
    cell.parse::<f64>().ok().filter(|v| v.is_finite())
}

// ---------------------------------------------------------------------------
// write
// ---------------------------------------------------------------------------

pub fn write(roast: &GetRoastResult) -> String {
    let s = &roast.summary;
    let mut out = String::new();
    out.push_str("# Droptime Logger roast export\n");
    if let Some(c) = &s.coffee_name {
        out.push_str(&format!("# coffee: {c}\n"));
    }
    let (iso_date, iso_time) = super::iso_date_time(s.started_wall_ms);
    out.push_str(&format!("# date: {iso_date} {iso_time} UTC\n"));
    if let Some(m) = &s.machine_name {
        out.push_str(&format!("# machine: {m}\n"));
    }
    if let Some(w) = s.charge_weight_lb {
        out.push_str(&format!("# charge_weight_lb: {w}\n"));
    }
    if let Some(w) = s.drop_weight_lb {
        out.push_str(&format!("# drop_weight_lb: {w}\n"));
    }
    out.push_str(&format!(
        "# markers_sec_from_charge: dry_end={} fc_start={} fc_end={} drop={}\n",
        fmt_opt(s.markers.dry_end_sec),
        fmt_opt(s.markers.fc_start_sec),
        fmt_opt(s.markers.fc_end_sec),
        fmt_opt(s.markers.drop_sec),
    ));
    out.push_str("time,bt,et,ror\n");

    let samples = &roast.samples;
    for (i, smp) in samples.iter().enumerate() {
        let et = smp.et_f.map(|v| format!("{v:.1}")).unwrap_or_default();
        let ror = if i > 0 {
            let prev = &samples[i - 1];
            let dt = smp.session_sec - prev.session_sec;
            if dt > 0.0 {
                (smp.bt_f - prev.bt_f) / dt * 60.0
            } else {
                0.0
            }
        } else {
            0.0
        };
        out.push_str(&format!(
            "{:.1},{:.1},{},{:.1}\n",
            smp.session_sec, smp.bt_f, et, ror
        ));
    }
    out
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}"))
        .unwrap_or_else(|| "-".to_string())
}
