// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Driver Error Types
//!
//! 드라이버 에러 정의

use std::fmt;
use std::io;
use thiserror::Error;

// ============================================================================
// DriverError - 드라이버 에러
// ============================================================================

/// 드라이버 에러
#[derive(Error, Debug)]
pub enum DriverError {
    /// 연결 에러
    #[error("Connection error: {0}")]
    Connection(String),

    /// 인증 에러
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// 프로토콜 에러
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// 세션 에러
    #[error("Session error: {0}")]
    Session(String),

    /// 트랜잭션 에러
    #[error("Transaction error: {0}")]
    Transaction(String),

    /// 쿼리 에러
    #[error("Query error: {code} - {message}")]
    Query { code: String, message: String },

    /// 타임아웃 에러
    #[error("Timeout: {0}")]
    Timeout(String),

    /// 풀 에러
    #[error("Pool error: {0}")]
    Pool(String),

    /// 설정 에러
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// 직렬화 에러
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// 타입 변환 에러
    #[error("Type conversion error: {0}")]
    TypeConversion(String),

    /// 서버 에러
    #[error("Server error: {code} - {message}")]
    Server { code: String, message: String },

    /// 서비스 불가
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// I/O 에러
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// 내부 에러
    #[error("Internal error: {0}")]
    Internal(String),
}

impl DriverError {
    /// 연결 에러 생성
    pub fn connection(msg: impl Into<String>) -> Self {
        Self::Connection(msg.into())
    }

    /// 인증 에러 생성
    pub fn authentication(msg: impl Into<String>) -> Self {
        Self::Authentication(msg.into())
    }

    /// 프로토콜 에러 생성
    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol(msg.into())
    }

    /// 세션 에러 생성
    pub fn session(msg: impl Into<String>) -> Self {
        Self::Session(msg.into())
    }

    /// 트랜잭션 에러 생성
    pub fn transaction(msg: impl Into<String>) -> Self {
        Self::Transaction(msg.into())
    }

    /// 쿼리 에러 생성
    pub fn query(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Query {
            code: code.into(),
            message: message.into(),
        }
    }

    /// 타임아웃 에러 생성
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout(msg.into())
    }

    /// 풀 에러 생성
    pub fn pool(msg: impl Into<String>) -> Self {
        Self::Pool(msg.into())
    }

    /// 설정 에러 생성
    pub fn configuration(msg: impl Into<String>) -> Self {
        Self::Configuration(msg.into())
    }

    /// 서비스 불가 에러 생성
    pub fn service_unavailable(msg: impl Into<String>) -> Self {
        Self::ServiceUnavailable(msg.into())
    }

    /// 타입 변환 에러 생성
    pub fn type_conversion(msg: impl Into<String>) -> Self {
        Self::TypeConversion(msg.into())
    }

    /// 서버 에러 생성
    pub fn server(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Server {
            code: code.into(),
            message: message.into(),
        }
    }

    /// 재시도 가능 여부
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Connection(_) | Self::Timeout(_) | Self::ServiceUnavailable(_) => true,
            Self::Server { code, .. } => is_retryable_code(code),
            _ => false,
        }
    }

    /// 클라이언트 에러 여부
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::Authentication(_)
                | Self::Configuration(_)
                | Self::TypeConversion(_)
                | Self::Query { .. }
        )
    }
}

/// 재시도 가능한 에러 코드 확인
fn is_retryable_code(code: &str) -> bool {
    code.starts_with("Zeta.TransientError")
        || code == "Zeta.ClientError.General.ForbiddenOnReadOnlyDatabase"
}

// ============================================================================
// Result Type
// ============================================================================

/// 드라이버 결과 타입
pub type DriverResult<T> = Result<T, DriverError>;

// ============================================================================
// Bolt Server Error Codes
// ============================================================================

/// Bolt 서버 에러 코드
///
/// Bolt 프로토콜 서버에서 반환하는 에러 코드를 나타냅니다.
/// 에러 코드는 "Zeta.{Category}.{SubCategory}.{ErrorType}" 형식을 따릅니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoltError {
    /// 에러 코드
    pub code: String,
    /// 에러 메시지
    pub message: String,
}

impl BoltError {
    /// 새 에러 생성
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// 클라이언트 에러 여부
    pub fn is_client_error(&self) -> bool {
        self.code.starts_with("Zeta.ClientError")
    }

    /// 데이터베이스 에러 여부
    pub fn is_database_error(&self) -> bool {
        self.code.starts_with("Zeta.DatabaseError")
    }

    /// 트랜지언트 에러 여부 (재시도 가능)
    pub fn is_transient_error(&self) -> bool {
        self.code.starts_with("Zeta.TransientError")
    }

    /// 인증 에러 여부
    pub fn is_authentication_error(&self) -> bool {
        self.code.contains("Security") || self.code.contains("Authentication")
    }

    /// 권한 에러 여부
    pub fn is_authorization_error(&self) -> bool {
        self.code.contains("Forbidden") || self.code.contains("Authorization")
    }
}

impl fmt::Display for BoltError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BoltError {}

impl From<BoltError> for DriverError {
    fn from(err: BoltError) -> Self {
        if err.is_authentication_error() {
            DriverError::Authentication(err.message)
        } else if err.is_transient_error() {
            DriverError::ServiceUnavailable(err.message)
        } else {
            DriverError::Server {
                code: err.code,
                message: err.message,
            }
        }
    }
}
