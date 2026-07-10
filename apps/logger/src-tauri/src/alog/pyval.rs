//! Thin, dependency-light helpers over `py_literal::Value`.
//!
//! We only ever touch `Value` through these accessors so the parser reads as a
//! plain key-map lookup and never has to name `num_bigint` (a transitive dep of
//! `py_literal` we don't declare directly). Integers are constructed for the
//! writer by round-tripping a decimal string through the parser.

use py_literal::Value;

/// Look up a string-keyed entry in a Python dict value.
pub fn dict_get<'a>(dict: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    dict.iter()
        .find(|(k, _)| k.as_string().map(|s| s.as_str()) == Some(key))
        .map(|(_, v)| v)
}

/// Coerce a scalar numeric `Value` to `f64` (int, float, or bool).
pub fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        // BigInt has no direct f64 accessor exposed; its Display is a decimal
        // string that parses losslessly for the magnitudes we see here.
        Value::Integer(i) => i.to_string().parse::<f64>().ok(),
        Value::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Coerce a scalar numeric `Value` to `i64` (truncating floats).
pub fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Integer(i) => i.to_string().parse::<i64>().ok(),
        Value::Float(f) => Some(*f as i64),
        _ => None,
    }
}

/// A borrowed string, trimmed; `None` when empty or not a string.
pub fn as_nonempty_str(v: &Value) -> Option<&str> {
    v.as_string().map(|s| s.trim()).filter(|s| !s.is_empty())
}

/// A list (or tuple) of numeric `Value`s as `Vec<Option<f64>>`, position-aligned
/// (a non-numeric entry becomes `None` rather than shifting later indices).
pub fn num_vec(v: &Value) -> Option<Vec<Option<f64>>> {
    let items = match v {
        Value::List(items) | Value::Tuple(items) => items,
        _ => return None,
    };
    Some(items.iter().map(as_f64).collect())
}

/// A list (or tuple) of integer `Value`s as `Vec<i64>` (non-ints become `0`).
pub fn int_vec(v: &Value) -> Option<Vec<i64>> {
    let items = match v {
        Value::List(items) | Value::Tuple(items) => items,
        _ => return None,
    };
    Some(items.iter().map(|x| as_i64(x).unwrap_or(0)).collect())
}

// --- writer builders -------------------------------------------------------

pub fn py_str(s: &str) -> Value {
    Value::String(s.to_string())
}

pub fn py_float(f: f64) -> Value {
    // Keep NaN/Inf out of the output — Artisan expects finite floats.
    Value::Float(if f.is_finite() { f } else { 0.0 })
}

/// Build a Python integer without naming `num_bigint`: the parser turns a
/// decimal string (including a leading `-`) into `Value::Integer`.
pub fn py_int(i: i64) -> Value {
    i.to_string()
        .parse::<Value>()
        .unwrap_or(Value::Float(i as f64))
}

pub fn py_float_list(xs: &[f64]) -> Value {
    Value::List(xs.iter().map(|x| py_float(*x)).collect())
}

pub fn py_int_list(xs: &[i64]) -> Value {
    Value::List(xs.iter().map(|x| py_int(*x)).collect())
}
