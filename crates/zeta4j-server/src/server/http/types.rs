// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! HTTP Request/Response Types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Bolt 프로토콜 버전 상수
pub const BOLT_VERSION: &str = "5.4";

/// API 응답 래퍼
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    /// 응답 데이터
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// 에러 목록
    pub errors: Vec<ApiError>,
    /// 메타데이터
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
}

impl<T> ApiResponse<T> {
    /// 성공 응답 생성
    pub fn success(data: T) -> Self {
        Self {
            data: Some(data),
            errors: vec![],
            meta: None,
        }
    }

    /// 성공 응답 (메타데이터 포함)
    pub fn success_with_meta(data: T, meta: ResponseMeta) -> Self {
        Self {
            data: Some(data),
            errors: vec![],
            meta: Some(meta),
        }
    }

    /// 에러 응답 생성
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            data: None,
            errors: vec![ApiError {
                code: code.into(),
                message: message.into(),
                details: None,
            }],
            meta: None,
        }
    }

    /// 다중 에러 응답
    pub fn errors(errors: Vec<ApiError>) -> Self {
        Self {
            data: None,
            errors,
            meta: None,
        }
    }
}

/// API 에러
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// 에러 코드
    pub code: String,
    /// 에러 메시지
    pub message: String,
    /// 추가 상세 정보
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// 새 에러 생성
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// 상세 정보 추가
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// 응답 메타데이터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMeta {
    /// 요청 ID
    pub request_id: String,
    /// 처리 시간 (밀리초)
    pub took_ms: u64,
    /// 북마크 (트랜잭션 추적용)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
}

impl ResponseMeta {
    /// 새 메타데이터 생성
    pub fn new(request_id: impl Into<String>, took_ms: u64) -> Self {
        Self {
            request_id: request_id.into(),
            took_ms,
            bookmark: None,
        }
    }

    /// 북마크 추가
    pub fn with_bookmark(mut self, bookmark: impl Into<String>) -> Self {
        self.bookmark = Some(bookmark.into());
        self
    }
}

/// 서버 정보 응답 (GET /)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Zeta4j 서버 버전
    pub zeta4j_version: String,
    /// Bolt 프로토콜 버전
    pub bolt_version: String,
    /// 에디션
    pub edition: String,
    /// 기본 데이터베이스
    pub database: String,
    /// 트랜잭션 URI
    #[serde(rename = "transaction")]
    pub transaction_uri: String,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            zeta4j_version: env!("CARGO_PKG_VERSION").to_string(),
            bolt_version: BOLT_VERSION.to_string(),
            edition: "Standard".to_string(),
            database: "zeta4j".to_string(),
            transaction_uri: "/db/zeta4j/tx".to_string(),
        }
    }
}

/// 헬스 체크 응답 (GET /health)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// 전체 상태
    pub status: HealthStatus,
    /// 서버 버전
    pub version: String,
    /// 에디션 (base)
    pub edition: String,
    /// 가동 시간 (초)
    pub uptime_seconds: u64,
    /// 시작 시간 (ISO 8601)
    pub started_at: String,
    /// 컴포넌트별 상태
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub components: HashMap<String, ComponentHealth>,
}

impl HealthResponse {
    /// 건강한 상태 생성 (기본값)
    pub fn healthy() -> Self {
        Self {
            status: HealthStatus::Up,
            version: env!("CARGO_PKG_VERSION").to_string(),
            edition: "Standard".to_string(),
            uptime_seconds: 0,
            started_at: chrono::Utc::now().to_rfc3339(),
            components: HashMap::new(),
        }
    }

    /// 에디션 설정
    pub fn with_edition(mut self, edition: impl Into<String>) -> Self {
        self.edition = edition.into();
        self
    }

    /// 시작 시간으로 uptime 설정
    pub fn with_uptime(mut self, started_at: std::time::Instant) -> Self {
        self.uptime_seconds = started_at.elapsed().as_secs();
        self
    }

    /// 시작 시간 문자열 설정
    pub fn with_started_at(mut self, started_at: impl Into<String>) -> Self {
        self.started_at = started_at.into();
        self
    }

    /// 시간대에 따른 시작 시간 설정
    pub fn with_timezone(mut self, timezone: &str) -> Self {
        self.started_at = if timezone.to_lowercase() == "local" {
            chrono::Local::now().to_rfc3339()
        } else {
            chrono::Utc::now().to_rfc3339()
        };
        self
    }

    /// 컴포넌트 상태 추가
    pub fn with_component(mut self, name: impl Into<String>, health: ComponentHealth) -> Self {
        self.components.insert(name.into(), health);
        self
    }

    /// 전체 상태 계산
    pub fn compute_status(&mut self) {
        if self.components.is_empty() {
            return;
        }

        let all_up = self
            .components
            .values()
            .all(|c| matches!(c.status, HealthStatus::Up));
        let any_down = self
            .components
            .values()
            .any(|c| matches!(c.status, HealthStatus::Down));

        self.status = if any_down {
            HealthStatus::Down
        } else if all_up {
            HealthStatus::Up
        } else {
            HealthStatus::Degraded
        };
    }
}

/// 헬스 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HealthStatus {
    /// 정상
    Up,
    /// 비정상
    Down,
    /// 성능 저하
    Degraded,
}

/// 컴포넌트 헬스
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    /// 상태
    pub status: HealthStatus,
    /// 상세 정보
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ComponentHealth {
    /// 건강한 컴포넌트
    pub fn up() -> Self {
        Self {
            status: HealthStatus::Up,
            details: None,
        }
    }

    /// 비정상 컴포넌트
    pub fn down(reason: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Down,
            details: Some(serde_json::json!({ "reason": reason.into() })),
        }
    }

    /// 성능 저하 컴포넌트
    pub fn degraded(reason: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Degraded,
            details: Some(serde_json::json!({ "reason": reason.into() })),
        }
    }

    /// 상세 정보와 함께 정상 상태
    pub fn up_with_details(details: serde_json::Value) -> Self {
        Self {
            status: HealthStatus::Up,
            details: Some(details),
        }
    }

    /// 상세 정보와 함께 성능 저하 상태
    pub fn degraded_with_details(details: serde_json::Value) -> Self {
        Self {
            status: HealthStatus::Degraded,
            details: Some(details),
        }
    }

    /// 상세 정보와 함께 비정상 상태
    pub fn down_with_details(details: serde_json::Value) -> Self {
        Self {
            status: HealthStatus::Down,
            details: Some(details),
        }
    }
}

/// 준비 상태 응답 (GET /ready)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyResponse {
    /// 준비 상태
    pub ready: bool,
    /// 메시지
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ReadyResponse {
    /// 준비됨
    pub fn ready() -> Self {
        Self {
            ready: true,
            message: None,
        }
    }

    /// 준비되지 않음
    pub fn not_ready(message: impl Into<String>) -> Self {
        Self {
            ready: false,
            message: Some(message.into()),
        }
    }
}
