//! 공통 타입 정의
//!
//! 모든 zeta4j 크레이트에서 사용하는 기본 타입입니다.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 노드 ID 타입
pub type NodeId = u64;

/// 엣지 ID 타입
pub type EdgeId = u64;

/// 레이블 타입
pub type Label = String;

/// 프로퍼티 맵 타입
pub type PropertyMap = HashMap<String, PropertyValue>;

/// 프로퍼티 값
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[derive(Default)]
pub enum PropertyValue {
    /// Null 값
    #[default]
    Null,
    /// 불리언 값
    Boolean(bool),
    /// 정수 값
    Integer(i64),
    /// 실수 값
    Float(f64),
    /// 문자열 값
    String(String),
    /// 바이트 배열
    Bytes(Vec<u8>),
    /// 배열 값
    Array(Vec<PropertyValue>),
    /// 맵 값
    Map(HashMap<String, PropertyValue>),
}

impl PropertyValue {
    /// Null 여부 확인
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// 불리언 값 추출
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(v) => Some(*v),
            _ => None,
        }
    }

    /// 정수 값 추출
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            Self::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    /// 실수 값 추출
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// 문자열 값 추출
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    /// 배열 값 추출
    pub fn as_array(&self) -> Option<&[PropertyValue]> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }
}

impl From<bool> for PropertyValue {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}

impl From<i64> for PropertyValue {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}

impl From<i32> for PropertyValue {
    fn from(v: i32) -> Self {
        Self::Integer(v as i64)
    }
}

impl From<f64> for PropertyValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<String> for PropertyValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for PropertyValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl<T: Into<PropertyValue>> From<Vec<T>> for PropertyValue {
    fn from(v: Vec<T>) -> Self {
        Self::Array(v.into_iter().map(Into::into).collect())
    }
}

/// 버전 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    /// 주 버전
    pub major: u32,
    /// 부 버전
    pub minor: u32,
    /// 패치 버전
    pub patch: u32,
    /// 프리릴리즈 태그
    pub prerelease: Option<String>,
}

impl Version {
    /// 새 버전 생성
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: None,
        }
    }

    /// 현재 버전
    pub fn current() -> Self {
        Self::new(1, 4, 0)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(pre) = &self.prerelease {
            write!(f, "{}.{}.{}-{}", self.major, self.minor, self.patch, pre)
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

/// 방향 열거형
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Direction {
    /// 나가는 방향
    Outgoing,
    /// 들어오는 방향
    Incoming,
    /// 양방향
    #[default]
    Both,
}
