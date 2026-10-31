// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Index Value Types
//!
//! 인덱스 키로 사용할 수 있는 값 타입 및 정규화 함수

use indexmap::IndexMap;
use std::collections::BTreeMap;

use crate::core::Value;

/// 인덱스 키로 사용할 수 있는 값 타입
/// HashMap 키로 사용하기 위해 Hash + Eq 구현
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IndexValue {
    Null,
    Boolean(bool),
    Integer(i64),
    /// f64를 비트 패턴으로 저장 (Hash 지원)
    Float(u64),
    String(String),
}

impl From<&Value> for IndexValue {
    fn from(value: &Value) -> Self {
        match value {
            Value::Null => IndexValue::Null,
            Value::Boolean(b) => IndexValue::Boolean(*b),
            Value::Integer(i) => IndexValue::Integer(*i),
            Value::Float(f) => IndexValue::Float(f.to_bits()),
            Value::String(s) => IndexValue::String(s.clone()),
            // 복합 타입은 정규화된 문자열로 변환
            Value::Array(arr) => IndexValue::String(normalize_array(arr)),
            Value::Map(map) => IndexValue::String(normalize_map(map)),
            Value::Node(n) => IndexValue::Integer(*n as i64),
            Value::Edge(e) => IndexValue::Integer(*e as i64),
            Value::Path(p) => IndexValue::String(format!("{:?}", p)),
            Value::Date(d) => IndexValue::String(d.to_string()),
            Value::Time(t) => IndexValue::String(t.to_string()),
            Value::DateTime(dt) => IndexValue::String(dt.to_string()),
            Value::LocalDateTime(ldt) => IndexValue::String(ldt.to_string()),
            Value::Duration(d) => IndexValue::String(format!("{:?}", d)),
            Value::Vector(v) => IndexValue::String(format!("vector({})", v.dimensions())),
        }
    }
}

/// Array 값을 정규화된 문자열로 변환
/// 형식: [elem1,elem2,...]
pub fn normalize_array(arr: &[Value]) -> String {
    let elements: Vec<String> = arr.iter().map(normalize_value).collect();
    format!("[{}]", elements.join(","))
}

/// Map 값을 정규화된 문자열로 변환
/// 키를 정렬하여 결정적인 순서 보장
/// 형식: {key1:value1,key2:value2,...}
pub fn normalize_map(map: &IndexMap<String, Value>) -> String {
    // BTreeMap으로 변환하여 키 정렬
    let sorted: BTreeMap<&String, &Value> = map.iter().collect();
    let pairs: Vec<String> = sorted
        .iter()
        .map(|(k, v)| format!("{}:{}", k, normalize_value(v)))
        .collect();
    format!("{{{}}}", pairs.join(","))
}

/// 단일 Value를 정규화된 문자열로 변환 (재귀적)
pub fn normalize_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => {
            // NaN과 Infinity 처리
            if f.is_nan() {
                "NaN".to_string()
            } else if f.is_infinite() {
                if *f > 0.0 {
                    "Inf".to_string()
                } else {
                    "-Inf".to_string()
                }
            } else {
                f.to_string()
            }
        }
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Array(arr) => normalize_array(arr),
        Value::Map(map) => normalize_map(map),
        Value::Node(n) => format!("node({})", n),
        Value::Edge(e) => format!("edge({})", e),
        Value::Path(p) => format!("path({:?})", p),
        Value::Date(d) => format!("date({})", d),
        Value::Time(t) => format!("time({})", t),
        Value::DateTime(dt) => format!("datetime({})", dt),
        Value::LocalDateTime(ldt) => format!("localdatetime({})", ldt),
        Value::Duration(d) => format!("duration({:?})", d),
        Value::Vector(v) => format!("vector({})", v.dimensions()),
    }
}
