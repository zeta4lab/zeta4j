// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Authentication error types.
//!
//! Defines error types for authentication operations including
//! password hashing, token validation, and authorization failures.

use thiserror::Error;
use zeta4j_credential::CredentialFailure;

/// Authentication error types.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Canonical credential lifecycle failure.
    #[error("{0}")]
    Credential(#[from] CredentialFailure),

    /// Invalid credentials (username or password).
    #[error("Invalid credentials")]
    InvalidCredentials,

    /// Missing authentication header.
    #[error("Missing authentication header")]
    MissingAuthHeader,

    /// Invalid authentication header format.
    #[error("Invalid authentication header format: {0}")]
    InvalidAuthHeader(String),

    /// Invalid Base64 encoding.
    #[error("Invalid Base64 encoding: {0}")]
    InvalidBase64(String),

    /// Invalid UTF-8 encoding.
    #[error("Invalid UTF-8 encoding: {0}")]
    InvalidUtf8(String),

    /// Password hashing failed.
    #[error("Password hashing failed: {0}")]
    HashingFailed(String),

    /// Password verification failed.
    #[error("Password verification failed: {0}")]
    VerificationFailed(String),

    /// Token expired.
    #[error("Token expired")]
    TokenExpired,

    /// Invalid token.
    #[error("Invalid token: {0}")]
    InvalidToken(String),

    /// Token not found (for blacklist check).
    #[error("Token not found or revoked")]
    TokenRevoked,

    /// User not found.
    #[error("User not found: {0}")]
    UserNotFound(String),

    /// User disabled.
    #[error("User account is disabled: {0}")]
    UserDisabled(String),

    /// Internal error.
    #[error("Internal authentication error: {0}")]
    Internal(String),
}

impl AuthError {
    /// Returns the HTTP status code for this error.
    pub fn status_code(&self) -> u16 {
        match self {
            Self::Credential(CredentialFailure::Throttled) => 429,
            Self::Credential(CredentialFailure::Unavailable | CredentialFailure::Internal) => 503,
            Self::Credential(CredentialFailure::PasswordPolicy | CredentialFailure::Policy) => 400,
            Self::Credential(_) => 401,
            Self::InvalidCredentials => 401,
            Self::MissingAuthHeader => 401,
            Self::InvalidAuthHeader(_) => 400,
            Self::InvalidBase64(_) => 400,
            Self::InvalidUtf8(_) => 400,
            Self::HashingFailed(_) => 500,
            Self::VerificationFailed(_) => 500,
            Self::TokenExpired => 401,
            Self::InvalidToken(_) => 401,
            Self::TokenRevoked => 401,
            Self::UserNotFound(_) => 401,
            Self::UserDisabled(_) => 403,
            Self::Internal(_) => 500,
        }
    }

    /// Returns the error code string.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Credential(failure) => failure.code(),
            Self::InvalidCredentials => "INVALID_CREDENTIALS",
            Self::MissingAuthHeader => "MISSING_AUTH_HEADER",
            Self::InvalidAuthHeader(_) => "INVALID_AUTH_HEADER",
            Self::InvalidBase64(_) => "INVALID_BASE64",
            Self::InvalidUtf8(_) => "INVALID_UTF8",
            Self::HashingFailed(_) => "HASHING_FAILED",
            Self::VerificationFailed(_) => "VERIFICATION_FAILED",
            Self::TokenExpired => "TOKEN_EXPIRED",
            Self::InvalidToken(_) => "INVALID_TOKEN",
            Self::TokenRevoked => "TOKEN_REVOKED",
            Self::UserNotFound(_) => "USER_NOT_FOUND",
            Self::UserDisabled(_) => "USER_DISABLED",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

/// Result type alias for authentication operations.
pub type AuthResult<T> = Result<T, AuthError>;
