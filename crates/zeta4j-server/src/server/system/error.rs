// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! System Database Error Types

use std::io;
use thiserror::Error;
use zeta4j_credential::CredentialFailure;

/// System Database 에러
#[derive(Debug, Error)]
pub enum SystemError {
    // ========================================================================
    // Storage Errors
    // ========================================================================
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    // ========================================================================
    // Catalog Errors
    // ========================================================================
    #[error("Database not found: {0}")]
    DatabaseNotFound(String),

    #[error("Database already exists: {0}")]
    DatabaseAlreadyExists(String),

    #[error("Cannot delete default database")]
    CannotDeleteDefault,

    #[error("Invalid database name: {0}")]
    InvalidDatabaseName(String),

    #[error("Database is in invalid state: {0}")]
    InvalidDatabaseState(String),

    // ========================================================================
    // Security Errors
    // ========================================================================
    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("User already exists: {0}")]
    UserAlreadyExists(String),

    #[error("Cannot delete the last user")]
    CannotDeleteLastUser,

    #[error("Invalid password")]
    InvalidPassword,

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("{0}")]
    Credential(#[from] CredentialFailure),

    #[error("Access denied: {0}")]
    AccessDenied(String),

    #[error("Password change is required")]
    PasswordChangeRequired,

    #[error("Bootstrap credential has expired")]
    BootstrapExpired,

    #[error("Bootstrap credential has already been consumed")]
    BootstrapConsumed,

    // ========================================================================
    // Schema Errors
    // ========================================================================
    #[error("Index not found: {0}")]
    IndexNotFound(String),

    #[error("Index already exists: {0}")]
    IndexAlreadyExists(String),

    #[error("Constraint not found: {0}")]
    ConstraintNotFound(String),

    #[error("Constraint already exists: {0}")]
    ConstraintAlreadyExists(String),

    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    // ========================================================================
    // General Errors
    // ========================================================================
    #[error("Not initialized")]
    NotInitialized,

    #[error("Already initialized")]
    AlreadyInitialized,

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl SystemError {
    /// 스토리지 에러 생성
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    /// 직렬화 에러 생성
    pub fn serialization(msg: impl Into<String>) -> Self {
        Self::Serialization(msg.into())
    }

    /// 역직렬화 에러 생성
    pub fn deserialization(msg: impl Into<String>) -> Self {
        Self::Deserialization(msg.into())
    }

    /// 내부 에러 생성
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

/// Result type alias
pub type SystemResult<T> = Result<T, SystemError>;
