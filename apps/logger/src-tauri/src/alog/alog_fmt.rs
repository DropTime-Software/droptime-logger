//! `.alog` (Artisan) parse + write. Python-literal in/out via `py_literal`.
//! Key meanings are recorded in `PROVENANCE.md` (documented behavior only).

use py_literal::Value;

use crate::error::LoggerError;
use crate::model::{EventDto, GetRoastResult, SampleDto};

use super::pyval::{
    as_f64, as_nonempty_str, dict_get, int_vec, num_vec, py_float, py_float_list, py_int_list,
    py_str,
};
use super::ImportedRoast;

// ---------------------------------------------------------------------------
// parse
// ---------------------------------------------------------------------------

pub fn parse(path: &str) -> Result<ImportedRoast, LoggerError> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| LoggerError::io(format!("read {path}: {e}")))?;
    let value: Value = raw.trim().parse().map_err(|e| {
        LoggerError::parse(format!("{path}: not a valid .alog (Python literal): {e}"))
    })?;
    let dict = value
        .as_dict()
        .ok_or_else(|| LoggerError::parse(format!("{path}: .alog root is not a dict")))?;
    parse_dict(dict, path)
}

fn parse_dict(dict: &[(Value, Value)], path: &str) -> Result<ImportedRoast, LoggerError> {
    // Time axis (required). `timex[0]` is the rebase origin.
    let timex_raw = dict_get(dict, "timex")
        .and_then(num_vec)
        .ok_or_else(|| LoggerError::parse(format!("{path}: missing/!list `timex`")))?;
    let timex: Vec<f64> = timex_raw.iter().map(|o| o.unwrap_or(f64::NAN)).collect();
    let origin = *timex
        .first()
        .ok_or_else(|| LoggerError::parse(format!("{path}: empty `timex`")))?;
    if !origin.is_finite() {
        return Err(LoggerError::parse(format!(
            "{path}: `timex[0]` is not a finite number"
        )));
    }

    // BT is required (temp2), ET optional (temp1). Remember: temp2 is BT.
    let bt = dict_get(dict, "temp2")
        .and_then(num_vec)
        .ok_or_else(|| LoggerError::parse(format!("{path}: missing/!list `temp2` (BT)")))?;
    let et = dict_get(dict, "temp1").and_then(num_vec);

    // Unit: `mode` wins; otherwise auto-detect from the BT range.
    let is_celsius = match dict_get(dict, "mode")
        .and_then(|v| v.as_string())
        .map(|s| s.trim())
    {
        Some("C") | Some("c") => true,
        Some("F") | Some("f") => false,
        _ => {
            let max_bt = bt
                .iter()
                .filter_map(|o| *o)
                .filter(|v| v.is_finite())
                .fold(f64::MIN, f64::max);
            super::looks_celsius(max_bt)
        }
    };
    let conv = |v: f64| if is_celsius { super::c_to_f(v) } else { v };

    let ambient_f = dict_get(dict, "ambientTemp")
        .and_then(as_f64)
        .filter(|v| v.is_finite())
        .map(conv);

    // Samples: zip timex/temp2, drop dropout sentinels, rebase to first-at-0.
    let mut samples: Vec<SampleDto> = Vec::new();
    let n = timex.len().min(bt.len());
    let mut seq = 0u64;
    for i in 0..n {
        let Some(bt_raw) = bt[i] else { continue };
        let t = timex[i];
        if super::is_no_reading(bt_raw) || !t.is_finite() {
            continue;
        }
        seq += 1;
        let et_f = et
            .as_ref()
            .and_then(|e| e.get(i).copied().flatten())
            .filter(|v| !super::is_no_reading(*v))
            .map(conv);
        samples.push(SampleDto {
            seq,
            session_sec: t - origin,
            bt_f: conv(bt_raw),
            et_f,
            ambient_f,
            heater: None,
            fan: None,
            drum: None,
        });
    }
    if samples.is_empty() {
        return Err(LoggerError::parse(format!(
            "{path}: no usable samples in `timex`/`temp2`"
        )));
    }

    // Markers from `timeindex`: <0 unset for all; 0 unset for all but CHARGE.
    let mut events: Vec<EventDto> = Vec::new();
    if let Some(indices) = dict_get(dict, "timeindex").and_then(int_vec) {
        for (slot, kind) in super::TIMEINDEX_KINDS.iter().enumerate() {
            let Some(&idx) = indices.get(slot) else { break };
            let is_charge = slot == 0;
            let set = idx > 0 || (idx == 0 && is_charge);
            if !set {
                continue;
            }
            let ui = idx as usize;
            let Some(&t) = timex.get(ui) else { continue };
            if !t.is_finite() {
                continue;
            }
            events.push(EventDto {
                kind: *kind,
                session_sec: t - origin,
                note: None,
            });
        }
    }

    let coffee_name = dict_get(dict, "beans")
        .and_then(as_nonempty_str)
        .or_else(|| dict_get(dict, "title").and_then(as_nonempty_str))
        .map(|s| s.to_string());

    let (charge_weight_lb, drop_weight_lb) = parse_weight(dict);
    let notes = parse_notes(dict);
    let started_wall_ms = parse_started(dict).unwrap_or_else(|| super::file_mtime_ms(path));

    Ok(ImportedRoast {
        device_id: String::new(),
        source_id: "import:alog".to_string(),
        imported_from: "alog".to_string(),
        started_wall_ms,
        coffee_name,
        charge_weight_lb,
        drop_weight_lb,
        notes,
        samples,
        events,
    })
}

/// `weight = [in, out, unit]` → (charge_lb, drop_lb). A `0` component is unset.
fn parse_weight(dict: &[(Value, Value)]) -> (Option<f64>, Option<f64>) {
    let Some(w) = dict_get(dict, "weight") else {
        return (None, None);
    };
    let items = match w {
        Value::List(items) | Value::Tuple(items) => items,
        _ => return (None, None),
    };
    let unit = items
        .get(2)
        .and_then(|v| v.as_string())
        .map(|s| s.as_str())
        .unwrap_or("lb");
    let to_lb = |v: f64| (v > 0.0).then(|| super::weight_to_lb(v, unit));
    let charge = items.first().and_then(as_f64).and_then(to_lb);
    let drop = items.get(1).and_then(as_f64).and_then(to_lb);
    (charge, drop)
}

fn parse_notes(dict: &[(Value, Value)]) -> Option<String> {
    let roasting = dict_get(dict, "roastingnotes").and_then(as_nonempty_str);
    let cupping = dict_get(dict, "cuppingnotes").and_then(as_nonempty_str);
    match (roasting, cupping) {
        (Some(r), Some(c)) => Some(format!("{r}\n\n{c}")),
        (Some(r), None) => Some(r.to_string()),
        (None, Some(c)) => Some(c.to_string()),
        (None, None) => None,
    }
}

fn parse_started(dict: &[(Value, Value)]) -> Option<i64> {
    let date = dict_get(dict, "roastisodate").and_then(|v| v.as_string())?;
    let time = dict_get(dict, "roasttime")
        .and_then(|v| v.as_string())
        .map(|s| s.as_str());
    super::epoch_ms_from_iso(date, time)
}

// ---------------------------------------------------------------------------
// write
// ---------------------------------------------------------------------------

/// Serialize a roast as an `.alog` dict Artisan can open (mode `'F'`).
pub fn write(roast: &GetRoastResult) -> Result<String, LoggerError> {
    let samples = &roast.samples;
    let summary = &roast.summary;

    let timex: Vec<f64> = samples.iter().map(|s| s.session_sec).collect();
    let temp2: Vec<f64> = samples.iter().map(|s| s.bt_f).collect();
    let temp1: Vec<f64> = samples.iter().map(|s| s.et_f.unwrap_or(-1.0)).collect();
    let timeindex = build_timeindex(&roast.events, &timex);

    let coffee = summary.coffee_name.clone().unwrap_or_default();
    let charge_w = summary.charge_weight_lb.unwrap_or(0.0);
    let drop_w = summary.drop_weight_lb.unwrap_or(0.0);
    let (iso_date, iso_time) = super::iso_date_time(summary.started_wall_ms);
    let ambient = samples.iter().find_map(|s| s.ambient_f);

    let mut entries: Vec<(Value, Value)> = vec![
        (py_str("mode"), py_str("F")),
        (py_str("timex"), py_float_list(&timex)),
        (py_str("temp1"), py_float_list(&temp1)),
        (py_str("temp2"), py_float_list(&temp2)),
        (py_str("timeindex"), py_int_list(&timeindex)),
        (py_str("title"), py_str(&coffee)),
        (py_str("beans"), py_str(&coffee)),
        (
            py_str("weight"),
            Value::List(vec![py_float(charge_w), py_float(drop_w), py_str("lb")]),
        ),
        (py_str("roastisodate"), py_str(&iso_date)),
        (py_str("roasttime"), py_str(&iso_time)),
        (
            py_str("roastingnotes"),
            py_str(summary.notes.as_deref().unwrap_or("")),
        ),
    ];
    if let Some(a) = ambient {
        entries.push((py_str("ambientTemp"), py_float(a)));
    }

    Value::Dict(entries)
        .format_ascii()
        .map_err(|e| LoggerError::io(format!("format .alog: {e}")))
}

/// Build the 8-slot `timeindex`: each marker's nearest sample index, or `-1`
/// when that marker is unset (unambiguous across every slot on re-import).
fn build_timeindex(events: &[EventDto], timex: &[f64]) -> Vec<i64> {
    super::TIMEINDEX_KINDS
        .iter()
        .map(|kind| match events.iter().rev().find(|e| e.kind == *kind) {
            Some(e) => nearest_index(timex, e.session_sec)
                .map(|i| i as i64)
                .unwrap_or(-1),
            None => -1,
        })
        .collect()
}

fn nearest_index(timex: &[f64], sec: f64) -> Option<usize> {
    timex
        .iter()
        .enumerate()
        .fold(None::<(f64, usize)>, |best, (i, &t)| {
            let dist = (t - sec).abs();
            match best {
                Some((bd, _)) if bd <= dist => best,
                _ => Some((dist, i)),
            }
        })
        .map(|(_, i)| i)
}
