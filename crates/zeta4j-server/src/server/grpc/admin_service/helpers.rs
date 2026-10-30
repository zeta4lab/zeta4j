//! Value conversion helpers for explicit data import/export.

use indexmap::IndexMap;
use zeta4j_graph::core::Value;

pub fn parse_csv_value(value: &str) -> Value {
    if value.is_empty() || value.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if value.eq_ignore_ascii_case("true") {
        return Value::Boolean(true);
    }
    if value.eq_ignore_ascii_case("false") {
        return Value::Boolean(false);
    }
    if let Ok(integer) = value.parse::<i64>() {
        return Value::Integer(integer);
    }
    if let Ok(float) = value.parse::<f64>() {
        return Value::Float(float);
    }
    Value::String(value.to_string())
}

pub fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Boolean(*value),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(Value::Integer)
            .or_else(|| value.as_f64().map(Value::Float))
            .unwrap_or(Value::Null),
        serde_json::Value::String(value) => Value::String(value.clone()),
        serde_json::Value::Array(values) => {
            Value::Array(values.iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(values) => Value::Map(
            values
                .iter()
                .map(|(key, value)| (key.clone(), json_to_value(value)))
                .collect::<IndexMap<_, _>>(),
        ),
    }
}

pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Boolean(value) => serde_json::Value::Bool(*value),
        Value::Integer(value) => serde_json::Value::Number((*value).into()),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(value_to_json).collect())
        }
        Value::Map(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), value_to_json(value)))
                .collect(),
        ),
        Value::DateTime(value) => serde_json::Value::String(value.to_rfc3339()),
        Value::Date(value) => serde_json::Value::String(value.to_string()),
        Value::Time(value) => serde_json::Value::String(value.format("%H:%M:%S%.f").to_string()),
        Value::LocalDateTime(value) => {
            serde_json::Value::String(value.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
        }
        Value::Duration(value) => serde_json::Value::String(value.to_string()),
        Value::Node(value) | Value::Edge(value) => serde_json::Value::Number((*value).into()),
        Value::Path(_) => serde_json::Value::Null,
        Value::Vector(values) => serde_json::Value::Array(
            values
                .as_slice()
                .iter()
                .map(|value| {
                    serde_json::Number::from_f64(f64::from(*value))
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                })
                .collect(),
        ),
    }
}
