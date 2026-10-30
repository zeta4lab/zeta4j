//! Value 타입 변환 유틸리티
//!
//! Value ↔ JSON 변환 기능 제공

use indexmap::IndexMap;
use zeta4j_graph::core::Value;

/// Value 타입 변환기
pub struct ValueConverter;

impl ValueConverter {
    /// Value를 serde_json::Value로 변환
    pub fn to_json(value: &Value) -> serde_json::Value {
        match value {
            Value::Null => serde_json::Value::Null,
            Value::Boolean(b) => serde_json::Value::Bool(*b),
            Value::Integer(i) => serde_json::json!(*i),
            Value::Float(f) => {
                if f.is_nan() || f.is_infinite() {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(*f)
                }
            }
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::Array(arr) => serde_json::Value::Array(arr.iter().map(Self::to_json).collect()),
            Value::Map(map) => {
                let obj: serde_json::Map<String, serde_json::Value> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::to_json(v)))
                    .collect();
                serde_json::Value::Object(obj)
            }
            Value::DateTime(dt) => serde_json::Value::String(dt.to_rfc3339()),
            Value::Date(d) => serde_json::Value::String(d.to_string()),
            Value::Time(t) => serde_json::Value::String(t.to_string()),
            Value::LocalDateTime(ldt) => serde_json::Value::String(ldt.to_string()),
            Value::Duration(dur) => {
                // ISO 8601 duration 형식
                serde_json::json!({
                    "months": dur.months,
                    "days": dur.days,
                    "seconds": dur.seconds,
                    "nanoseconds": dur.nanoseconds
                })
            }
            Value::Node(node_id) => serde_json::json!({
                "type": "node",
                "id": node_id
            }),
            Value::Edge(edge_id) => serde_json::json!({
                "type": "edge",
                "id": edge_id
            }),
            Value::Path(path) => serde_json::json!({
                "type": "path",
                "nodes": path.nodes.clone(),
                "edges": path.edges.clone()
            }),
            Value::Vector(v) => serde_json::json!({
                "type": "vector",
                "dimensions": v.dimensions(),
                "data": v.as_slice()
            }),
        }
    }

    /// serde_json::Value를 Value로 변환
    pub fn from_json(json: &serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Null
                }
            }
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(arr) => {
                Value::Array(arr.iter().map(Self::from_json).collect())
            }
            serde_json::Value::Object(obj) => {
                let map: IndexMap<String, Value> = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::from_json(v)))
                    .collect();
                Value::Map(map)
            }
        }
    }

    /// 문자열에서 Value 타입 추론 (CSV용)
    pub fn infer_from_string(s: &str) -> Value {
        // 빈 문자열 → Null
        if s.is_empty() {
            return Value::Null;
        }

        // Boolean
        match s.to_lowercase().as_str() {
            "true" => return Value::Boolean(true),
            "false" => return Value::Boolean(false),
            _ => {}
        }

        // Integer
        if let Ok(i) = s.parse::<i64>() {
            return Value::Integer(i);
        }

        // Float
        if let Ok(f) = s.parse::<f64>() {
            return Value::Float(f);
        }

        // JSON Array 시도 ([ 로 시작하면)
        if s.starts_with('[')
            && s.ends_with(']')
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(s)
        {
            return Self::from_json(&json);
        }

        // String
        Value::String(s.to_string())
    }

    /// Value를 CSV용 문자열로 변환
    pub fn to_csv_string(value: &Value) -> String {
        match value {
            Value::Null => String::new(),
            Value::Boolean(b) => b.to_string(),
            Value::Integer(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Array(_) | Value::Map(_) => {
                // 복잡한 타입은 JSON 문자열로
                serde_json::to_string(&Self::to_json(value)).unwrap_or_default()
            }
            Value::DateTime(dt) => dt.to_rfc3339(),
            Value::Date(d) => d.to_string(),
            Value::Time(t) => t.to_string(),
            Value::LocalDateTime(ldt) => ldt.to_string(),
            Value::Duration(dur) => format!("P{}M{}DT{}S", dur.months, dur.days, dur.seconds),
            Value::Node(id) => format!("node:{}", id),
            Value::Edge(id) => format!("edge:{}", id),
            Value::Path(_) => {
                // Path는 JSON으로
                serde_json::to_string(&Self::to_json(value)).unwrap_or_default()
            }
            Value::Vector(v) => format!("vector({})", v.dimensions()),
        }
    }
}
