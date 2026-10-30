//! 데이터베이스 메타데이터 타입.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 데이터베이스 메타데이터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseMetadata {
    /// 생성 시간
    pub created_at: DateTime<Utc>,
    /// 마지막 시작 시간
    pub last_started_at: Option<DateTime<Utc>>,
    /// 마지막 중지 시간
    pub last_stopped_at: Option<DateTime<Utc>>,
    /// 기본 데이터베이스 여부
    pub is_default: bool,
    /// 시스템 데이터베이스 여부
    pub is_system: bool,
    /// 스키마 버전
    pub schema_version: u32,
    /// 커스텀 속성
    pub properties: HashMap<String, String>,
}

impl DatabaseMetadata {
    /// 새 메타데이터 생성
    pub fn new() -> Self {
        Self {
            created_at: Utc::now(),
            last_started_at: None,
            last_stopped_at: None,
            is_default: false,
            is_system: false,
            schema_version: 1,
            properties: HashMap::new(),
        }
    }

    /// 기본 데이터베이스용 메타데이터
    pub fn default_database() -> Self {
        Self {
            created_at: Utc::now(),
            last_started_at: Some(Utc::now()),
            last_stopped_at: None,
            is_default: true,
            is_system: false,
            schema_version: 1,
            properties: HashMap::new(),
        }
    }

    /// 시스템 데이터베이스용 메타데이터
    pub fn system_database() -> Self {
        Self {
            created_at: Utc::now(),
            last_started_at: Some(Utc::now()),
            last_stopped_at: None,
            is_default: false,
            is_system: true,
            schema_version: 1,
            properties: HashMap::new(),
        }
    }
}

impl Default for DatabaseMetadata {
    fn default() -> Self {
        Self::new()
    }
}
