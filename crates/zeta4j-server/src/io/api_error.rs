// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Router API Error Types
//!
//! HTTP 라우터에서 사용하는 공통 에러 타입 정의
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::RouterApiError;
//! use axum::{http::StatusCode, Json};
//!
//! // 기본 에러 응답
//! fn handler() -> Result<Json<Data>, (StatusCode, Json<RouterApiError>)> {
//!     Err((
//!         StatusCode::NOT_FOUND,
//!         Json(RouterApiError::not_found("Resource not found")),
//!     ))
//! }
//! ```

use serde::{Deserialize, Serialize};

// ============================================================================
// RouterApiError
// ============================================================================

/// 라우터 API 에러 응답
///
/// HTTP 라우터에서 `(StatusCode, Json<RouterApiError>)` 형태로 사용됩니다.
///
/// # 필드
/// - `error`: 에러 타입/코드 (예: "NotFound", "BadRequest")
/// - `message`: 사람이 읽을 수 있는 에러 메시지
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterApiError {
    /// 에러 타입/이름
    pub error: String,
    /// 에러 메시지
    pub message: String,
}

impl RouterApiError {
    /// 새로운 RouterApiError 생성
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }

    /// 내부 서버 에러 생성
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("InternalError", message)
    }

    /// 잘못된 요청 에러 생성
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("BadRequest", message)
    }

    /// 리소스를 찾을 수 없음 에러 생성
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("NotFound", message)
    }

    /// 서비스 사용 불가 에러 생성
    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new("ServiceUnavailable", message)
    }

    /// 인증 실패 에러 생성
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new("Unauthorized", message)
    }

    /// 권한 없음 에러 생성
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new("Forbidden", message)
    }

    /// 충돌 에러 생성 (이미 존재함)
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("Conflict", message)
    }

    /// 유효성 검사 실패 에러 생성
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("ValidationError", message)
    }
}
