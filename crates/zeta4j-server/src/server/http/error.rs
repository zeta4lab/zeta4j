// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! HTTP Error Types

use axum::Json;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde::Serialize;

use zeta4j_common::error::ZetaError;

/// HTTP 에러 타입
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// 잘못된 요청
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// 인증 필요
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// 권한 없음
    #[error("Forbidden: {0}")]
    Forbidden(String),

    /// 리소스를 찾을 수 없음
    #[error("Not found: {0}")]
    NotFound(String),

    /// 허용되지 않는 메서드
    #[error("Method not allowed")]
    MethodNotAllowed,

    /// 충돌
    #[error("Conflict: {0}")]
    Conflict(String),

    /// 처리할 수 없는 엔티티
    #[error("Unprocessable entity: {0}")]
    UnprocessableEntity(String),

    /// 요청이 너무 많음
    #[error("Too many requests")]
    TooManyRequests,

    /// 내부 서버 오류
    #[error("Internal server error: {0}")]
    InternalError(String),

    /// 서비스 이용 불가
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// 요청 타임아웃
    #[error("Request timeout")]
    RequestTimeout,

    /// 페이로드 크기 초과
    #[error("Payload too large")]
    PayloadTooLarge,
}

/// Bolt 호환 에러 코드
impl HttpError {
    /// 에러 코드 반환
    pub fn error_code(&self) -> &'static str {
        match self {
            HttpError::BadRequest(_) => "Zeta.ClientError.Request.Invalid",
            HttpError::Unauthorized(_) => "Zeta.ClientError.Security.Unauthorized",
            HttpError::Forbidden(_) => "Zeta.ClientError.Security.Forbidden",
            HttpError::NotFound(_) => "Zeta.ClientError.Request.NotFound",
            HttpError::MethodNotAllowed => "Zeta.ClientError.Request.InvalidFormat",
            HttpError::Conflict(_) => "Zeta.ClientError.Request.Conflict",
            HttpError::UnprocessableEntity(_) => "Zeta.ClientError.Statement.SyntaxError",
            HttpError::TooManyRequests => "Zeta.ClientError.Request.RateLimited",
            HttpError::InternalError(_) => "Zeta.DatabaseError.General.UnknownError",
            HttpError::ServiceUnavailable(_) => "Zeta.TransientError.General.DatabaseUnavailable",
            HttpError::RequestTimeout => "Zeta.TransientError.Transaction.Timeout",
            HttpError::PayloadTooLarge => "Zeta.ClientError.Request.PayloadTooLarge",
        }
    }

    /// HTTP 상태 코드 반환
    pub fn status_code(&self) -> StatusCode {
        match self {
            HttpError::BadRequest(_) => StatusCode::BAD_REQUEST,
            HttpError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            HttpError::Forbidden(_) => StatusCode::FORBIDDEN,
            HttpError::NotFound(_) => StatusCode::NOT_FOUND,
            HttpError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            HttpError::Conflict(_) => StatusCode::CONFLICT,
            HttpError::UnprocessableEntity(_) => StatusCode::UNPROCESSABLE_ENTITY,
            HttpError::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            HttpError::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            HttpError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            HttpError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            HttpError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }
}

/// 에러 응답 JSON
#[derive(Serialize)]
struct ErrorResponse {
    errors: Vec<ErrorDetail>,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorResponse {
            errors: vec![ErrorDetail {
                code: self.error_code().to_string(),
                message: self.to_string(),
            }],
        };

        (status, Json(body)).into_response()
    }
}

/// ZetaError 래퍼 (orphan rule 우회) (Phase 5)
///
/// axum IntoResponse를 구현하기 위한 newtype 패턴
pub struct ApiError(pub ZetaError);

impl From<ZetaError> for ApiError {
    fn from(err: ZetaError) -> Self {
        ApiError(err)
    }
}

impl From<super::db_manager::DbManagerError> for ApiError {
    fn from(err: super::db_manager::DbManagerError) -> Self {
        ApiError(err.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = ErrorResponse {
            errors: vec![ErrorDetail {
                code: self.0.code().to_string(),
                message: self.0.to_string(),
            }],
        };

        (status, Json(body)).into_response()
    }
}

/// 다른 에러 타입에서 변환
impl From<std::io::Error> for HttpError {
    fn from(err: std::io::Error) -> Self {
        HttpError::InternalError(err.to_string())
    }
}

impl From<serde_json::Error> for HttpError {
    fn from(err: serde_json::Error) -> Self {
        HttpError::BadRequest(format!("JSON error: {}", err))
    }
}
