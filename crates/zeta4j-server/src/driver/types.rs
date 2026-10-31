// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Driver Types
//!
//! 드라이버에서 사용하는 타입 정의

use indexmap::IndexMap;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use std::fmt;

use super::error::{DriverError, DriverResult};
use crate::server::bolt::packstream::{
    PackStreamNode, PackStreamRelationship, PackStreamValue, PackStreamVector,
};
use zeta4j_graph::core::value::VectorValue;

// ============================================================================
// Value - 그래프 값
// ============================================================================

/// 그래프 값 타입
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// Null
    Null,
    /// Boolean
    Boolean(bool),
    /// Integer (i64)
    Integer(i64),
    /// Float (f64)
    Float(f64),
    /// String
    String(String),
    /// Bytes
    Bytes(Vec<u8>),
    /// List
    List(Vec<Value>),
    /// Map
    Map(IndexMap<String, Value>),
    /// Node
    Node(Node),
    /// Relationship
    Relationship(Relationship),
    /// Path
    Path(Path),
    /// Point (2D/3D)
    Point(Point),
    /// Date
    Date(NaiveDate),
    /// Time
    Time(NaiveTime),
    /// LocalTime
    LocalTime(NaiveTime),
    /// DateTime
    DateTime(DateTime<FixedOffset>),
    /// LocalDateTime
    LocalDateTime(NaiveDateTime),
    /// Duration
    Duration(Duration),
    /// Vector (Phase 3: high-dimensional embedding)
    Vector(VectorValue),
}

impl Value {
    /// Null 여부
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Boolean으로 변환
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Integer로 변환
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Float로 변환
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// String으로 변환
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// List로 변환
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    /// Map으로 변환
    pub fn as_map(&self) -> Option<&IndexMap<String, Value>> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Node로 변환
    pub fn as_node(&self) -> Option<&Node> {
        match self {
            Value::Node(n) => Some(n),
            _ => None,
        }
    }

    /// Relationship으로 변환
    pub fn as_relationship(&self) -> Option<&Relationship> {
        match self {
            Value::Relationship(r) => Some(r),
            _ => None,
        }
    }

    /// Path로 변환
    pub fn as_path(&self) -> Option<&Path> {
        match self {
            Value::Path(p) => Some(p),
            _ => None,
        }
    }

    /// Vector로 변환
    pub fn as_vector(&self) -> Option<&VectorValue> {
        match self {
            Value::Vector(v) => Some(v),
            _ => None,
        }
    }

    /// Vector 여부 확인
    pub fn is_vector(&self) -> bool {
        matches!(self, Value::Vector(_))
    }

    /// 타입 이름
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "Null",
            Value::Boolean(_) => "Boolean",
            Value::Integer(_) => "Integer",
            Value::Float(_) => "Float",
            Value::String(_) => "String",
            Value::Bytes(_) => "Bytes",
            Value::List(_) => "List",
            Value::Map(_) => "Map",
            Value::Node(_) => "Node",
            Value::Relationship(_) => "Relationship",
            Value::Path(_) => "Path",
            Value::Point(_) => "Point",
            Value::Date(_) => "Date",
            Value::Time(_) => "Time",
            Value::LocalTime(_) => "LocalTime",
            Value::DateTime(_) => "DateTime",
            Value::LocalDateTime(_) => "LocalDateTime",
            Value::Duration(_) => "Duration",
            Value::Vector(_) => "Vector",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Integer(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
            Value::List(l) => write!(f, "[{} items]", l.len()),
            Value::Map(m) => write!(f, "{{{} entries}}", m.len()),
            Value::Node(n) => write!(f, "{}", n),
            Value::Relationship(r) => write!(f, "{}", r),
            Value::Path(p) => write!(f, "{}", p),
            Value::Point(p) => write!(f, "{}", p),
            Value::Date(d) => write!(f, "{}", d),
            Value::Time(t) => write!(f, "{}", t),
            Value::LocalTime(t) => write!(f, "{}", t),
            Value::DateTime(dt) => write!(f, "{}", dt),
            Value::LocalDateTime(dt) => write!(f, "{}", dt),
            Value::Duration(d) => write!(f, "{}", d),
            Value::Vector(v) => write!(f, "{}", v),
        }
    }
}

// From implementations
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Boolean(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Integer(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Integer(v as i64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_string())
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::List(v.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(val) => val.into(),
            None => Value::Null,
        }
    }
}

// ============================================================================
// Node - 그래프 노드
// ============================================================================

/// 그래프 노드
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// 노드 ID
    pub id: i64,
    /// 레이블
    pub labels: Vec<String>,
    /// 속성
    pub properties: IndexMap<String, Value>,
    /// 엘리먼트 ID (Neo4j 5.x)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,
}

impl Node {
    /// 새 노드 생성
    pub fn new(id: i64, labels: Vec<String>, properties: IndexMap<String, Value>) -> Self {
        Self {
            id,
            labels,
            properties,
            element_id: None,
        }
    }

    /// 레이블 포함 여부
    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }

    /// 속성 가져오기
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }

    /// 속성 가져오기 (타입 변환)
    pub fn get_as<T: TryFrom<Value, Error = DriverError>>(&self, key: &str) -> DriverResult<T> {
        self.properties
            .get(key)
            .cloned()
            .ok_or_else(|| DriverError::type_conversion(format!("Property '{}' not found", key)))
            .and_then(|v| T::try_from(v))
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let labels = if self.labels.is_empty() {
            String::new()
        } else {
            format!(":{}", self.labels.join(":"))
        };
        write!(f, "({}{})", self.id, labels)
    }
}

// ============================================================================
// Relationship - 그래프 관계
// ============================================================================

/// 그래프 관계
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    /// 관계 ID
    pub id: i64,
    /// 시작 노드 ID
    pub start_node_id: i64,
    /// 끝 노드 ID
    pub end_node_id: i64,
    /// 타입
    #[serde(rename = "type")]
    pub rel_type: String,
    /// 속성
    pub properties: IndexMap<String, Value>,
    /// 엘리먼트 ID (Neo4j 5.x)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,
    /// 시작 노드 엘리먼트 ID (Neo4j 5.x)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_node_element_id: Option<String>,
    /// 끝 노드 엘리먼트 ID (Neo4j 5.x)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_node_element_id: Option<String>,
}

impl Relationship {
    /// 새 관계 생성
    pub fn new(
        id: i64,
        start_node_id: i64,
        end_node_id: i64,
        rel_type: String,
        properties: IndexMap<String, Value>,
    ) -> Self {
        Self {
            id,
            start_node_id,
            end_node_id,
            rel_type,
            properties,
            element_id: None,
            start_node_element_id: None,
            end_node_element_id: None,
        }
    }

    /// 속성 가져오기
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }
}

impl fmt::Display for Relationship {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({})-[:{}]->({})  [id: {}]",
            self.start_node_id, self.rel_type, self.end_node_id, self.id
        )
    }
}

// ============================================================================
// Path - 그래프 경로
// ============================================================================

/// 그래프 경로
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Path {
    /// 노드들
    pub nodes: Vec<Node>,
    /// 관계들
    pub relationships: Vec<Relationship>,
}

impl Path {
    /// 새 경로 생성
    pub fn new(nodes: Vec<Node>, relationships: Vec<Relationship>) -> Self {
        Self {
            nodes,
            relationships,
        }
    }

    /// 경로 길이 (관계 수)
    pub fn len(&self) -> usize {
        self.relationships.len()
    }

    /// 빈 경로 여부
    pub fn is_empty(&self) -> bool {
        self.relationships.is_empty()
    }

    /// 시작 노드
    pub fn start(&self) -> Option<&Node> {
        self.nodes.first()
    }

    /// 끝 노드
    pub fn end(&self) -> Option<&Node> {
        self.nodes.last()
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<Path: {} nodes, {} rels>",
            self.nodes.len(),
            self.relationships.len()
        )
    }
}

// ============================================================================
// Point - 공간 좌표
// ============================================================================

/// 공간 좌표
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// SRID (Spatial Reference ID)
    pub srid: i32,
    /// X 좌표 (경도)
    pub x: f64,
    /// Y 좌표 (위도)
    pub y: f64,
    /// Z 좌표 (고도, 선택적)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
}

impl Point {
    /// 2D 포인트 생성
    pub fn new_2d(srid: i32, x: f64, y: f64) -> Self {
        Self {
            srid,
            x,
            y,
            z: None,
        }
    }

    /// 3D 포인트 생성
    pub fn new_3d(srid: i32, x: f64, y: f64, z: f64) -> Self {
        Self {
            srid,
            x,
            y,
            z: Some(z),
        }
    }

    /// WGS84 2D 포인트 (경도, 위도)
    pub fn wgs84_2d(longitude: f64, latitude: f64) -> Self {
        Self::new_2d(4326, longitude, latitude)
    }

    /// WGS84 3D 포인트 (경도, 위도, 고도)
    pub fn wgs84_3d(longitude: f64, latitude: f64, height: f64) -> Self {
        Self::new_3d(4979, longitude, latitude, height)
    }

    /// Cartesian 2D 포인트
    pub fn cartesian_2d(x: f64, y: f64) -> Self {
        Self::new_2d(7203, x, y)
    }

    /// Cartesian 3D 포인트
    pub fn cartesian_3d(x: f64, y: f64, z: f64) -> Self {
        Self::new_3d(9157, x, y, z)
    }

    /// 3D 여부
    pub fn is_3d(&self) -> bool {
        self.z.is_some()
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.z {
            Some(z) => write!(
                f,
                "Point(srid={}, x={}, y={}, z={})",
                self.srid, self.x, self.y, z
            ),
            None => write!(f, "Point(srid={}, x={}, y={})", self.srid, self.x, self.y),
        }
    }
}

// ============================================================================
// Duration - 시간 간격
// ============================================================================

/// 시간 간격
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Duration {
    /// 개월
    pub months: i64,
    /// 일
    pub days: i64,
    /// 초
    pub seconds: i64,
    /// 나노초
    pub nanoseconds: i32,
}

impl Duration {
    /// 새 Duration 생성
    pub fn new(months: i64, days: i64, seconds: i64, nanoseconds: i32) -> Self {
        Self {
            months,
            days,
            seconds,
            nanoseconds,
        }
    }

    /// 초에서 생성
    pub fn from_seconds(seconds: i64) -> Self {
        Self::new(0, 0, seconds, 0)
    }

    /// 일에서 생성
    pub fn from_days(days: i64) -> Self {
        Self::new(0, days, 0, 0)
    }

    /// 개월에서 생성
    pub fn from_months(months: i64) -> Self {
        Self::new(months, 0, 0, 0)
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "P{}M{}DT{}S",
            self.months,
            self.days,
            self.seconds as f64 + self.nanoseconds as f64 / 1_000_000_000.0
        )
    }
}

// ============================================================================
// PackStreamValue conversions
// ============================================================================

impl From<Value> for PackStreamValue {
    fn from(value: Value) -> Self {
        match value {
            Value::Null => PackStreamValue::Null,
            Value::Boolean(b) => PackStreamValue::Boolean(b),
            Value::Integer(i) => PackStreamValue::Integer(i),
            Value::Float(f) => PackStreamValue::Float(f),
            Value::String(s) => PackStreamValue::String(s),
            Value::Bytes(b) => PackStreamValue::Bytes(b),
            Value::List(l) => PackStreamValue::List(l.into_iter().map(Into::into).collect()),
            Value::Map(m) => {
                PackStreamValue::Map(m.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
            Value::Node(n) => {
                let ps_node = PackStreamNode::new(
                    n.id,
                    n.labels,
                    n.properties
                        .into_iter()
                        .map(|(k, v)| (k, v.into()))
                        .collect(),
                );
                ps_node.to_value()
            }
            Value::Relationship(r) => {
                let ps_rel = PackStreamRelationship::new(
                    r.id,
                    r.start_node_id,
                    r.end_node_id,
                    r.rel_type,
                    r.properties
                        .into_iter()
                        .map(|(k, v)| (k, v.into()))
                        .collect(),
                );
                ps_rel.to_value()
            }
            // For temporal types, convert to string representation
            Value::Date(d) => PackStreamValue::String(d.to_string()),
            Value::Time(t) => PackStreamValue::String(t.to_string()),
            Value::LocalTime(t) => PackStreamValue::String(t.to_string()),
            Value::DateTime(dt) => PackStreamValue::String(dt.to_rfc3339()),
            Value::LocalDateTime(dt) => PackStreamValue::String(dt.to_string()),
            Value::Duration(d) => PackStreamValue::String(d.to_string()),
            Value::Point(p) => {
                let mut map = std::collections::HashMap::new();
                map.insert("srid".to_string(), PackStreamValue::Integer(p.srid as i64));
                map.insert("x".to_string(), PackStreamValue::Float(p.x));
                map.insert("y".to_string(), PackStreamValue::Float(p.y));
                if let Some(z) = p.z {
                    map.insert("z".to_string(), PackStreamValue::Float(z));
                }
                PackStreamValue::Map(map)
            }
            Value::Path(_) => {
                // Path is complex, convert to string for now
                PackStreamValue::String("<path>".to_string())
            }
            Value::Vector(v) => {
                let ps_vec = PackStreamVector::new(v.as_slice().to_vec());
                ps_vec.to_value()
            }
        }
    }
}

impl From<PackStreamValue> for Value {
    fn from(value: PackStreamValue) -> Self {
        match value {
            PackStreamValue::Null => Value::Null,
            PackStreamValue::Boolean(b) => Value::Boolean(b),
            PackStreamValue::Integer(i) => Value::Integer(i),
            PackStreamValue::Float(f) => Value::Float(f),
            PackStreamValue::String(s) => Value::String(s),
            PackStreamValue::Bytes(b) => Value::Bytes(b),
            PackStreamValue::List(l) => Value::List(l.into_iter().map(Into::into).collect()),
            PackStreamValue::Map(m) => {
                Value::Map(m.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
            PackStreamValue::Structure(s) => {
                // Try to parse as Node or Relationship
                if let Ok(node) = PackStreamNode::from_value(&PackStreamValue::Structure(s.clone()))
                {
                    Value::Node(Node {
                        id: node.id,
                        labels: node.labels,
                        properties: node
                            .properties
                            .into_iter()
                            .map(|(k, v)| (k, v.into()))
                            .collect(),
                        element_id: node.element_id,
                    })
                } else if let Ok(rel) =
                    PackStreamRelationship::from_value(&PackStreamValue::Structure(s.clone()))
                {
                    Value::Relationship(Relationship {
                        id: rel.id,
                        start_node_id: rel.start_node_id,
                        end_node_id: rel.end_node_id,
                        rel_type: rel.rel_type,
                        properties: rel
                            .properties
                            .into_iter()
                            .map(|(k, v)| (k, v.into()))
                            .collect(),
                        element_id: rel.element_id,
                        start_node_element_id: rel.start_node_element_id,
                        end_node_element_id: rel.end_node_element_id,
                    })
                } else if let Ok(vec) =
                    PackStreamVector::from_value(&PackStreamValue::Structure(s.clone()))
                {
                    // Try to parse as Vector
                    if let Ok(v) = VectorValue::new(vec.data) {
                        Value::Vector(v)
                    } else {
                        // Invalid vector data, fallback to map
                        let mut map = IndexMap::new();
                        map.insert("_tag".to_string(), Value::Integer(s.tag as i64));
                        for (i, field) in s.fields.into_iter().enumerate() {
                            map.insert(format!("_{}", i), field.into());
                        }
                        Value::Map(map)
                    }
                } else {
                    // Unknown structure, convert to map
                    let mut map = IndexMap::new();
                    map.insert("_tag".to_string(), Value::Integer(s.tag as i64));
                    for (i, field) in s.fields.into_iter().enumerate() {
                        map.insert(format!("_{}", i), field.into());
                    }
                    Value::Map(map)
                }
            }
        }
    }
}

// ============================================================================
// TryFrom implementations
// ============================================================================

impl TryFrom<Value> for bool {
    type Error = DriverError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Boolean(b) => Ok(b),
            _ => Err(DriverError::type_conversion(format!(
                "Cannot convert {} to bool",
                value.type_name()
            ))),
        }
    }
}

impl TryFrom<Value> for i64 {
    type Error = DriverError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Integer(i) => Ok(i),
            _ => Err(DriverError::type_conversion(format!(
                "Cannot convert {} to i64",
                value.type_name()
            ))),
        }
    }
}

impl TryFrom<Value> for f64 {
    type Error = DriverError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Float(f) => Ok(f),
            Value::Integer(i) => Ok(i as f64),
            _ => Err(DriverError::type_conversion(format!(
                "Cannot convert {} to f64",
                value.type_name()
            ))),
        }
    }
}

impl TryFrom<Value> for String {
    type Error = DriverError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(s) => Ok(s),
            _ => Err(DriverError::type_conversion(format!(
                "Cannot convert {} to String",
                value.type_name()
            ))),
        }
    }
}

impl TryFrom<Value> for Node {
    type Error = DriverError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Node(n) => Ok(n),
            _ => Err(DriverError::type_conversion(format!(
                "Cannot convert {} to Node",
                value.type_name()
            ))),
        }
    }
}

impl TryFrom<Value> for Relationship {
    type Error = DriverError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Relationship(r) => Ok(r),
            _ => Err(DriverError::type_conversion(format!(
                "Cannot convert {} to Relationship",
                value.type_name()
            ))),
        }
    }
}

impl TryFrom<Value> for Path {
    type Error = DriverError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Path(p) => Ok(p),
            _ => Err(DriverError::type_conversion(format!(
                "Cannot convert {} to Path",
                value.type_name()
            ))),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================
