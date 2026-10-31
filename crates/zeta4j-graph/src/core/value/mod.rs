// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 그래프 데이터 값 타입

mod duration;
mod path;
mod vector;

pub use duration::CypherDuration;
pub use path::Path;
pub use vector::{VectorError, VectorValue};

use std::fmt;
use std::hash::{Hash, Hasher};

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

// ============================================================================
// Value Accessor Macros
// ============================================================================

/// 참조 반환 accessor 매크로 (as_xxx -> Option<&T>)
macro_rules! impl_as_ref {
    ($method:ident, $variant:ident, $ty:ty) => {
        pub fn $method(&self) -> Option<&$ty> {
            match self {
                Value::$variant(v) => Some(v),
                _ => None,
            }
        }
    };
}

/// 복사 반환 accessor 매크로 (as_xxx -> Option<T> where T: Copy)
macro_rules! impl_as_copy {
    ($method:ident, $variant:ident, $ty:ty) => {
        pub fn $method(&self) -> Option<$ty> {
            match self {
                Value::$variant(v) => Some(*v),
                _ => None,
            }
        }
    };
}

/// is_xxx 체크 매크로
macro_rules! impl_is_variant {
    ($method:ident, $variant:ident) => {
        pub fn $method(&self) -> bool {
            matches!(self, Value::$variant(_))
        }
    };
    // Null 같은 단위 variant용
    ($method:ident, $variant:ident, unit) => {
        pub fn $method(&self) -> bool {
            matches!(self, Value::$variant)
        }
    };
}

/// into_xxx 소유권 이전 매크로
macro_rules! impl_into {
    ($method:ident, $variant:ident, $ty:ty) => {
        pub fn $method(self) -> Option<$ty> {
            match self {
                Value::$variant(v) => Some(v),
                _ => None,
            }
        }
    };
}

/// 그래프에서 사용되는 값 타입
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// 정수 값
    Integer(i64),

    /// 실수 값
    Float(f64),

    /// 문자열 값
    String(String),

    /// 불린 값
    Boolean(bool),

    /// 배열 값
    Array(Vec<Value>),

    /// 맵 값 (삽입 순서 유지)
    Map(IndexMap<String, Value>),

    /// 날짜/시간 값 (UTC 타임존 포함)
    DateTime(DateTime<Utc>),

    /// 날짜만 (타임존 없음)
    Date(NaiveDate),

    /// 시간만 (타임존 없음)
    Time(NaiveTime),

    /// 로컬 날짜/시간 (타임존 없음)
    LocalDateTime(NaiveDateTime),

    /// 기간 (Duration)
    Duration(CypherDuration),

    /// 노드 참조 (노드 ID)
    Node(u64),

    /// 엣지 참조 (엣지 ID)
    Edge(u64),

    /// 경로 (노드와 관계의 시퀀스)
    Path(Path),

    /// Phase 3: 고차원 벡터 (임베딩)
    Vector(VectorValue),

    /// null 값
    Null,
}

impl Value {
    /// 값을 정수로 변환
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }

    /// 값을 실수로 변환
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Integer(i) => Some(*i as f64),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }

    /// 값을 문자열로 변환
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    // 매크로로 생성된 accessor 메서드들
    impl_as_copy!(as_boolean, Boolean, bool);
    impl_as_ref!(as_array, Array, Vec<Value>);
    impl_as_ref!(as_map, Map, IndexMap<String, Value>);
    impl_as_ref!(as_datetime, DateTime, DateTime<Utc>);

    /// 값을 DateTime으로 변환 (복사본)
    pub fn as_datetime_owned(&self) -> Option<DateTime<Utc>> {
        match self {
            Value::DateTime(dt) => Some(*dt),
            Value::String(s) => s.parse::<DateTime<Utc>>().ok(),
            _ => None,
        }
    }

    impl_is_variant!(is_null, Null, unit);
    impl_as_ref!(as_date, Date, NaiveDate);
    impl_as_ref!(as_time, Time, NaiveTime);
    impl_as_ref!(as_local_datetime, LocalDateTime, NaiveDateTime);
    impl_as_ref!(as_duration, Duration, CypherDuration);

    /// 값의 타입 이름 반환
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Integer(_) => "integer",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Boolean(_) => "boolean",
            Value::Array(_) => "array",
            Value::Map(_) => "map",
            Value::DateTime(_) => "datetime",
            Value::Date(_) => "date",
            Value::Time(_) => "time",
            Value::LocalDateTime(_) => "localdatetime",
            Value::Duration(_) => "duration",
            Value::Node(_) => "node",
            Value::Edge(_) => "edge",
            Value::Path(_) => "path",
            Value::Vector(_) => "vector",
            Value::Null => "null",
        }
    }

    impl_as_ref!(as_path, Path, Path);
    impl_is_variant!(is_path, Path);

    /// 값을 노드 ID로 변환
    pub fn as_node_id(&self) -> Option<u64> {
        match self {
            Value::Node(id) => Some(*id),
            Value::Integer(i) if *i >= 0 => Some(*i as u64),
            _ => None,
        }
    }

    /// 값을 엣지 ID로 변환
    pub fn as_edge_id(&self) -> Option<u64> {
        match self {
            Value::Edge(id) => Some(*id),
            _ => None,
        }
    }

    impl_is_variant!(is_node, Node);
    impl_is_variant!(is_edge, Edge);
    impl_is_variant!(is_vector, Vector);
    impl_as_ref!(as_vector, Vector, VectorValue);
    impl_into!(into_vector, Vector, VectorValue);

    /// 값의 메모리 사용량 추정 (바이트)
    pub fn memory_size(&self) -> usize {
        let inline = std::mem::size_of::<Value>();
        match self {
            Value::String(s) => inline.saturating_add(s.len()),
            Value::Array(arr) => arr
                .iter()
                .map(Value::memory_size)
                .fold(inline, usize::saturating_add),
            Value::Map(map) => map
                .iter()
                .map(|(key, value)| {
                    std::mem::size_of::<String>()
                        .saturating_add(key.len())
                        .saturating_add(value.memory_size())
                })
                .fold(inline, usize::saturating_add),
            Value::Path(path) => inline
                .saturating_add(path.nodes.len().saturating_mul(std::mem::size_of::<u64>()))
                .saturating_add(path.edges.len().saturating_mul(std::mem::size_of::<u64>())),
            Value::Vector(vector) => inline.saturating_add(
                vector
                    .as_slice()
                    .len()
                    .saturating_mul(std::mem::size_of::<f32>()),
            ),
            _ => inline,
        }
    }
}

impl crate::core::MemorySized for Value {
    fn memory_size(&self) -> usize {
        Self::memory_size(self)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, val) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", val)?;
                }
                write!(f, "]")
            }
            Value::Map(map) => {
                write!(f, "{{")?;
                let mut first = true;
                for (key, val) in map {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\": {}", key, val)?;
                    first = false;
                }
                write!(f, "}}")
            }
            Value::DateTime(dt) => write!(f, "\"{}\"", dt.to_rfc3339()),
            Value::Date(d) => write!(f, "\"{}\"", d),
            Value::Time(t) => write!(f, "\"{}\"", t.format("%H:%M:%S%.f")),
            Value::LocalDateTime(ldt) => {
                write!(f, "\"{}\"", ldt.format("%Y-%m-%dT%H:%M:%S%.f"))
            }
            Value::Duration(dur) => write!(f, "\"{}\"", dur),
            Value::Node(id) => write!(f, "Node({})", id),
            Value::Edge(id) => write!(f, "Edge({})", id),
            Value::Path(path) => write!(f, "{}", path),
            Value::Vector(v) => write!(f, "{}", v),
            Value::Null => write!(f, "null"),
        }
    }
}

// From trait 구현들
impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Integer(i)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}

impl From<Vec<Value>> for Value {
    fn from(arr: Vec<Value>) -> Self {
        Value::Array(arr)
    }
}

impl From<IndexMap<String, Value>> for Value {
    fn from(map: IndexMap<String, Value>) -> Self {
        Value::Map(map)
    }
}

impl From<DateTime<Utc>> for Value {
    fn from(dt: DateTime<Utc>) -> Self {
        Value::DateTime(dt)
    }
}

impl From<NaiveDate> for Value {
    fn from(d: NaiveDate) -> Self {
        Value::Date(d)
    }
}

impl From<NaiveTime> for Value {
    fn from(t: NaiveTime) -> Self {
        Value::Time(t)
    }
}

impl From<NaiveDateTime> for Value {
    fn from(ldt: NaiveDateTime) -> Self {
        Value::LocalDateTime(ldt)
    }
}

impl From<CypherDuration> for Value {
    fn from(d: CypherDuration) -> Self {
        Value::Duration(d)
    }
}

impl From<Path> for Value {
    fn from(p: Path) -> Self {
        Value::Path(p)
    }
}

impl From<VectorValue> for Value {
    fn from(v: VectorValue) -> Self {
        Value::Vector(v)
    }
}

impl TryFrom<Vec<f32>> for Value {
    type Error = VectorError;

    fn try_from(data: Vec<f32>) -> Result<Self, Self::Error> {
        VectorValue::new(data).map(Value::Vector)
    }
}

impl TryFrom<Value> for VectorValue {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Vector(v) => Ok(v),
            Value::Array(arr) => {
                // Array<Float>를 Vector로 변환 시도
                let floats: Result<Vec<f32>, _> = arr
                    .into_iter()
                    .map(|v| match v {
                        Value::Float(f) => Ok(f as f32),
                        Value::Integer(i) => Ok(i as f32),
                        _ => Err("Array contains non-numeric value"),
                    })
                    .collect();

                floats.and_then(|data| VectorValue::new(data).map_err(|_| "Invalid vector"))
            }
            _ => Err("Value is not a Vector"),
        }
    }
}

impl TryFrom<Value> for Vec<f32> {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        VectorValue::try_from(value).map(|v| v.into_vec())
    }
}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Value::Integer(i) => {
                0u8.hash(state);
                i.hash(state);
            }
            Value::Float(f) => {
                1u8.hash(state);
                // Float는 직접 Hash할 수 없으므로 비트 표현을 사용
                f.to_bits().hash(state);
            }
            Value::String(s) => {
                2u8.hash(state);
                s.hash(state);
            }
            Value::Boolean(b) => {
                3u8.hash(state);
                b.hash(state);
            }
            Value::Array(arr) => {
                4u8.hash(state);
                for item in arr {
                    item.hash(state);
                }
            }
            Value::Map(map) => {
                5u8.hash(state);
                // HashMap의 순서는 보장되지 않으므로 키를 정렬하여 해시
                let mut pairs: Vec<_> = map.iter().collect();
                pairs.sort_by_key(|(k, _)| *k);
                for (key, value) in pairs {
                    key.hash(state);
                    value.hash(state);
                }
            }
            Value::DateTime(dt) => {
                6u8.hash(state);
                dt.timestamp().hash(state);
                dt.timestamp_subsec_nanos().hash(state);
            }
            Value::Date(d) => {
                10u8.hash(state);
                d.hash(state);
            }
            Value::Time(t) => {
                11u8.hash(state);
                t.hash(state);
            }
            Value::LocalDateTime(ldt) => {
                12u8.hash(state);
                ldt.hash(state);
            }
            Value::Duration(dur) => {
                13u8.hash(state);
                dur.hash(state);
            }
            Value::Node(id) => {
                7u8.hash(state);
                id.hash(state);
            }
            Value::Edge(id) => {
                8u8.hash(state);
                id.hash(state);
            }
            Value::Path(path) => {
                14u8.hash(state);
                path.hash(state);
            }
            Value::Vector(v) => {
                15u8.hash(state);
                v.hash(state);
            }
            Value::Null => {
                9u8.hash(state);
            }
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // 타입 순서 결정 함수 (openCypher 호환)
        fn type_order(v: &Value) -> u8 {
            match v {
                Value::Null => 0,
                Value::Boolean(_) => 1,
                Value::Integer(_) => 2,
                Value::Float(_) => 3,
                Value::String(_) => 4,
                Value::Array(_) => 5,
                Value::Map(_) => 6,
                Value::Date(_) => 7,
                Value::Time(_) => 8,
                Value::LocalDateTime(_) => 9,
                Value::DateTime(_) => 10,
                Value::Duration(_) => 11,
                Value::Node(_) => 12,
                Value::Edge(_) => 13,
                Value::Path(_) => 14,
                Value::Vector(_) => 15,
            }
        }

        // 먼저 타입 순서 비교
        let self_order = type_order(self);
        let other_order = type_order(other);

        if self_order != other_order {
            return self_order.cmp(&other_order);
        }

        // 같은 타입 내에서 값 비교
        match (self, other) {
            (Value::Null, Value::Null) => Ordering::Equal,

            (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),

            (Value::Integer(a), Value::Integer(b)) => a.cmp(b),

            (Value::Float(a), Value::Float(b)) => a.total_cmp(b),

            (Value::String(a), Value::String(b)) => a.cmp(b),

            (Value::Array(a), Value::Array(b)) => a.cmp(b),

            (Value::Map(a), Value::Map(b)) => {
                // HashMap은 순서가 없으므로 키를 정렬하여 비교
                let mut a_keys: Vec<_> = a.keys().collect();
                let mut b_keys: Vec<_> = b.keys().collect();
                a_keys.sort();
                b_keys.sort();

                // 먼저 키 목록 비교
                match a_keys.cmp(&b_keys) {
                    Ordering::Equal => {
                        // 키가 같으면 값 비교
                        for key in a_keys {
                            match a[key].cmp(&b[key]) {
                                Ordering::Equal => continue,
                                other => return other,
                            }
                        }
                        Ordering::Equal
                    }
                    other => other,
                }
            }

            (Value::Date(a), Value::Date(b)) => a.cmp(b),

            (Value::Time(a), Value::Time(b)) => a.cmp(b),

            (Value::LocalDateTime(a), Value::LocalDateTime(b)) => a.cmp(b),

            (Value::DateTime(a), Value::DateTime(b)) => a.cmp(b),

            (Value::Duration(a), Value::Duration(b)) => a.cmp(b),

            (Value::Node(a), Value::Node(b)) => a.cmp(b),

            (Value::Edge(a), Value::Edge(b)) => a.cmp(b),

            (Value::Path(a), Value::Path(b)) => a.cmp(b),

            // 벡터는 차원 수로 비교하고, 같으면 요소별 비교
            (Value::Vector(a), Value::Vector(b)) => match a.dimensions().cmp(&b.dimensions()) {
                Ordering::Equal => {
                    // 요소별 비교 (f32는 total_cmp 사용)
                    for (av, bv) in a.as_slice().iter().zip(b.as_slice().iter()) {
                        match av.total_cmp(bv) {
                            Ordering::Equal => continue,
                            other => return other,
                        }
                    }
                    Ordering::Equal
                }
                other => other,
            },

            // 이 경우는 발생하지 않음 (위에서 타입 순서로 먼저 분기)
            _ => unreachable!(),
        }
    }
}
