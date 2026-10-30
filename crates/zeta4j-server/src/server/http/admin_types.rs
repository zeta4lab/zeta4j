//! Admin API Types (M2.3)
//!
//! 관리 엔드포인트용 요청/응답 타입 정의
//! Bolt 관리 API 호환

use serde::{Deserialize, Serialize};

// ============================================================================
// Server Info Types
// ============================================================================

/// 서버 정보 응답
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfoResponse {
    pub name: String,
    pub version: String,
    pub edition: String,
    pub mode: ServerMode,
    pub status: ServerStatus,
    pub uptime_ms: u64,
}

/// 서버 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ServerMode {
    #[serde(rename = "SINGLE")]
    Single,
}

/// 서버 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ServerStatus {
    #[serde(rename = "AVAILABLE")]
    Available,
    #[serde(rename = "UNAVAILABLE")]
    Unavailable,
    #[serde(rename = "CORDONED")]
    Cordoned,
}

// ============================================================================
// Database Types
// ============================================================================

/// 데이터베이스 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DatabaseStatus {
    #[serde(rename = "online")]
    Online,
    #[serde(rename = "offline")]
    Offline,
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "stopping")]
    Stopping,
    #[serde(rename = "unknown")]
    #[default]
    Unknown,
}

impl std::fmt::Display for DatabaseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Online => write!(f, "online"),
            Self::Offline => write!(f, "offline"),
            Self::Starting => write!(f, "starting"),
            Self::Stopping => write!(f, "stopping"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// 데이터베이스 정보 응답
#[derive(Debug, Clone, Serialize)]
pub struct DatabaseInfo {
    pub name: String,
    pub address: String,
    pub role: String,
    pub status: String,
    #[serde(rename = "requestedStatus")]
    pub requested_status: String,
    pub default: bool,
    pub home: bool,
    #[serde(rename = "currentPrimariesCount")]
    pub current_primaries_count: u32,
    #[serde(rename = "currentSecondariesCount")]
    pub current_secondaries_count: u32,
    #[serde(rename = "creationTime")]
    pub creation_time: String,
    #[serde(rename = "lastStartTime", skip_serializing_if = "Option::is_none")]
    pub last_start_time: Option<String>,
    #[serde(rename = "lastStopTime", skip_serializing_if = "Option::is_none")]
    pub last_stop_time: Option<String>,
    #[serde(rename = "storeSize")]
    pub store_size: u64,
    #[serde(rename = "txCount")]
    pub tx_count: u64,
}

/// 데이터베이스 생성 요청
#[derive(Debug, Clone, Deserialize)]
pub struct CreateDatabaseRequest {
    pub name: String,
    #[serde(default)]
    pub options: Option<DatabaseOptions>,
}

/// 데이터베이스 생성 옵션
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DatabaseOptions {
    #[serde(rename = "existingData")]
    pub existing_data: Option<String>,
    #[serde(rename = "existingDataSeedInstance")]
    pub existing_data_seed_instance: Option<String>,
}

// ============================================================================
// User Types
// ============================================================================

/// 사용자 정보
#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub username: String,
    #[serde(rename = "passwordChangeRequired")]
    pub password_change_required: bool,
    pub suspended: bool,
    pub home: Option<String>,
}

/// 사용자 생성 요청
#[derive(Debug, Clone, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(rename = "requirePasswordChange", default)]
    pub require_password_change: bool,
}

// ============================================================================
// Metrics Types
// ============================================================================

/// JMX 호환 메트릭
#[derive(Debug, Clone, Serialize)]
pub struct JmxMetrics {
    pub store_size: JmxMetric,
    pub total_store_size: JmxMetric,
    pub transaction_committed: JmxMetric,
    pub transaction_rolled_back: JmxMetric,
    pub transaction_active: JmxMetric,
    pub bolt_connections_opened: JmxMetric,
    pub bolt_connections_closed: JmxMetric,
    pub bolt_connections_running: JmxMetric,
    pub bolt_messages_received: JmxMetric,
    pub bolt_messages_started: JmxMetric,
    pub bolt_messages_done: JmxMetric,
    pub bolt_messages_failed: JmxMetric,
}

/// 단일 JMX 메트릭
#[derive(Debug, Clone, Serialize)]
pub struct JmxMetric {
    pub description: String,
    pub value: serde_json::Value,
}

impl JmxMetric {
    pub fn new<T: Into<serde_json::Value>>(description: &str, value: T) -> Self {
        Self {
            description: description.to_string(),
            value: value.into(),
        }
    }
}

// ============================================================================
// Config Types
// ============================================================================

/// 서버 설정 응답
#[derive(Debug, Clone, Serialize)]
pub struct ServerConfigResponse {
    #[serde(flatten)]
    pub config: std::collections::HashMap<String, serde_json::Value>,
}
