//! 스키마 타입 정의
//!
//! 프로퍼티 데이터 타입 및 프로퍼티 정의

use crate::core::Value;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 프로퍼티 데이터 타입
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PropertyType {
    /// 문자열 타입
    String,
    /// 정수 타입
    Integer,
    /// 실수 타입
    Float,
    /// 불린 타입
    Boolean,
    /// 날짜 타입 (타임존 없음)
    Date,
    /// 날짜/시간 타입 (UTC 타임존)
    DateTime,
    /// 로컬 날짜/시간 타입 (타임존 없음)
    LocalDateTime,
    /// 시간 타입 (타임존 없음)
    Time,
    /// 기간 타입
    Duration,
    /// 리스트 타입 (요소 타입 지정)
    List(Box<PropertyType>),
    /// 맵 타입
    Map,
    /// 모든 타입 허용
    Any,
}

impl PropertyType {
    /// 문자열에서 PropertyType 파싱
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "STRING" => Some(PropertyType::String),
            "INTEGER" | "INT" => Some(PropertyType::Integer),
            "FLOAT" | "DOUBLE" => Some(PropertyType::Float),
            "BOOLEAN" | "BOOL" => Some(PropertyType::Boolean),
            "DATE" => Some(PropertyType::Date),
            "DATETIME" => Some(PropertyType::DateTime),
            "LOCALDATETIME" => Some(PropertyType::LocalDateTime),
            "TIME" => Some(PropertyType::Time),
            "DURATION" => Some(PropertyType::Duration),
            "MAP" => Some(PropertyType::Map),
            "ANY" => Some(PropertyType::Any),
            _ => {
                // LIST<TYPE> 형식 파싱
                if s.to_uppercase().starts_with("LIST<") && s.ends_with(">") {
                    let inner = &s[5..s.len() - 1];
                    PropertyType::parse(inner).map(|t| PropertyType::List(Box::new(t)))
                } else {
                    None
                }
            }
        }
    }

    /// Value가 이 타입과 호환되는지 확인
    pub fn matches(&self, value: &Value) -> bool {
        match (self, value) {
            (PropertyType::Any, _) => true,
            (_, Value::Null) => true, // Null은 모든 타입에 허용 (nullable 체크는 별도)
            (PropertyType::String, Value::String(_)) => true,
            (PropertyType::Integer, Value::Integer(_)) => true,
            (PropertyType::Float, Value::Float(_)) => true,
            (PropertyType::Float, Value::Integer(_)) => true, // Integer는 Float으로 변환 가능
            (PropertyType::Boolean, Value::Boolean(_)) => true,
            (PropertyType::Date, Value::Date(_)) => true,
            (PropertyType::DateTime, Value::DateTime(_)) => true,
            (PropertyType::LocalDateTime, Value::LocalDateTime(_)) => true,
            (PropertyType::Time, Value::Time(_)) => true,
            (PropertyType::Duration, Value::Duration(_)) => true,
            (PropertyType::Map, Value::Map(_)) => true,
            (PropertyType::List(inner_type), Value::Array(arr)) => {
                arr.iter().all(|v| inner_type.matches(v))
            }
            _ => false,
        }
    }
}

impl fmt::Display for PropertyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PropertyType::String => write!(f, "STRING"),
            PropertyType::Integer => write!(f, "INTEGER"),
            PropertyType::Float => write!(f, "FLOAT"),
            PropertyType::Boolean => write!(f, "BOOLEAN"),
            PropertyType::Date => write!(f, "DATE"),
            PropertyType::DateTime => write!(f, "DATETIME"),
            PropertyType::LocalDateTime => write!(f, "LOCALDATETIME"),
            PropertyType::Time => write!(f, "TIME"),
            PropertyType::Duration => write!(f, "DURATION"),
            PropertyType::List(inner) => write!(f, "LIST<{}>", inner),
            PropertyType::Map => write!(f, "MAP"),
            PropertyType::Any => write!(f, "ANY"),
        }
    }
}

/// 프로퍼티 정의
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyDefinition {
    /// 프로퍼티 이름
    pub name: String,
    /// 프로퍼티 타입
    pub property_type: PropertyType,
    /// null 허용 여부
    pub nullable: bool,
    /// 기본값
    pub default_value: Option<Value>,
}

impl PropertyDefinition {
    /// 새 프로퍼티 정의 생성
    pub fn new(name: impl Into<String>, property_type: PropertyType) -> Self {
        Self {
            name: name.into(),
            property_type,
            nullable: true,
            default_value: None,
        }
    }

    /// NOT NULL 제약조건 설정
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    /// 기본값 설정
    pub fn with_default(mut self, value: Value) -> Self {
        self.default_value = Some(value);
        self
    }

    /// 값 검증
    pub fn validate(&self, value: Option<&Value>) -> Result<(), String> {
        match value {
            None | Some(Value::Null) => {
                if !self.nullable && self.default_value.is_none() {
                    Err(format!("Property '{}' cannot be null", self.name))
                } else {
                    Ok(())
                }
            }
            Some(v) => {
                if self.property_type.matches(v) {
                    Ok(())
                } else {
                    Err(format!(
                        "Property '{}' expected type {}, got {}",
                        self.name,
                        self.property_type,
                        v.type_name()
                    ))
                }
            }
        }
    }

    /// 기본값 적용 (값이 없거나 null인 경우)
    pub fn apply_default(&self, value: Option<Value>) -> Option<Value> {
        match value {
            None | Some(Value::Null) => self.default_value.clone(),
            Some(v) => Some(v),
        }
    }
}
