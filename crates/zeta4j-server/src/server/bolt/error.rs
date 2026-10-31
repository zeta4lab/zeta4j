// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bolt protocol error types.

use std::fmt;
use std::io;

use super::message::FailureMessage;
use super::packstream::PackStreamError;

/// Result type for Bolt operations.
pub type BoltResult<T> = Result<T, BoltError>;

/// Bolt protocol errors.
#[derive(Debug)]
pub enum BoltError {
    /// I/O error
    Io(io::Error),

    /// Handshake error
    Handshake(HandshakeError),

    /// PackStream serialization error
    PackStream(PackStreamError),

    /// Protocol error (invalid message format, etc.)
    Protocol(String),

    /// Authentication error
    Authentication(String),

    /// Connection error
    Connection(String),

    /// Query execution error
    Query(String),

    /// Query execution error with a stable product code.
    QueryExecution { code: String, message: String },

    /// Transaction error
    Transaction(String),

    /// State machine error (invalid state transition)
    InvalidState(String),

    /// Unsupported protocol version
    UnsupportedVersion(u32),

    /// Message too large
    MessageTooLarge { size: usize, max: usize },

    /// Timeout error
    Timeout,

    /// Connection closed
    ConnectionClosed,

    /// Server shutting down
    ServerShutdown,
}

impl fmt::Display for BoltError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoltError::Io(e) => write!(f, "I/O error: {}", e),
            BoltError::Handshake(e) => write!(f, "Handshake error: {}", e),
            BoltError::PackStream(e) => write!(f, "PackStream error: {}", e),
            BoltError::Protocol(msg) => write!(f, "Protocol error: {}", msg),
            BoltError::Authentication(msg) => write!(f, "Authentication error: {}", msg),
            BoltError::Connection(msg) => write!(f, "Connection error: {}", msg),
            BoltError::Query(msg) => write!(f, "Query error: {}", msg),
            BoltError::QueryExecution { code, message } => {
                write!(f, "Query error [{}]: {}", code, message)
            }
            BoltError::Transaction(msg) => write!(f, "Transaction error: {}", msg),
            BoltError::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            BoltError::UnsupportedVersion(v) => write!(f, "Unsupported version: 0x{:08X}", v),
            BoltError::MessageTooLarge { size, max } => {
                write!(f, "Message too large: {} bytes (max: {})", size, max)
            }
            BoltError::Timeout => write!(f, "Operation timed out"),
            BoltError::ConnectionClosed => write!(f, "Connection closed"),
            BoltError::ServerShutdown => write!(f, "Server is shutting down"),
        }
    }
}

impl BoltError {
    /// Convert error to a FAILURE message.
    pub fn to_failure(&self) -> FailureMessage {
        match self {
            BoltError::Authentication(msg) => FailureMessage::auth_error(msg),
            BoltError::Query(msg) => {
                // Determine if syntax or semantic error
                if msg.contains("syntax") || msg.contains("Syntax") {
                    FailureMessage::syntax_error(msg)
                } else {
                    FailureMessage::semantic_error(msg)
                }
            }
            BoltError::QueryExecution { code, message } => FailureMessage::new(code, message),
            BoltError::Transaction(msg) => FailureMessage::transaction_error(msg),
            BoltError::InvalidState(msg) => {
                FailureMessage::new("Zeta.ClientError.Request.Invalid", msg)
            }
            BoltError::Timeout => {
                FailureMessage::new(BoltErrorCode::TRANSACTION_TIMEOUT, "Operation timed out")
            }
            _ => FailureMessage::general_error(&self.to_string()),
        }
    }

    pub fn from_graph_error(error: zeta4j_graph::GraphError) -> Self {
        let zeta_error: zeta4j_common::ZetaError = error.into();
        Self::from_zeta_error(zeta_error)
    }

    pub fn from_zeta_error(zeta_error: zeta4j_common::ZetaError) -> Self {
        Self::QueryExecution {
            code: zeta_error.code().to_string(),
            message: zeta_error.to_string(),
        }
    }
}

impl std::error::Error for BoltError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BoltError::Io(e) => Some(e),
            BoltError::Handshake(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for BoltError {
    fn from(err: io::Error) -> Self {
        BoltError::Io(err)
    }
}

impl From<HandshakeError> for BoltError {
    fn from(err: HandshakeError) -> Self {
        BoltError::Handshake(err)
    }
}

impl From<PackStreamError> for BoltError {
    fn from(err: PackStreamError) -> Self {
        BoltError::PackStream(err)
    }
}

/// Handshake-specific errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeError {
    /// Invalid magic number received
    InvalidMagic {
        expected: [u8; 4],
        received: [u8; 4],
    },

    /// No compatible protocol version found
    NoCompatibleVersion,

    /// Invalid handshake data (wrong size, etc.)
    InvalidData(String),

    /// Connection closed during handshake
    ConnectionClosed,

    /// Handshake timeout
    Timeout,
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::InvalidMagic { expected, received } => {
                write!(
                    f,
                    "Invalid magic number: expected {:02X?}, received {:02X?}",
                    expected, received
                )
            }
            HandshakeError::NoCompatibleVersion => {
                write!(f, "No compatible protocol version found")
            }
            HandshakeError::InvalidData(msg) => {
                write!(f, "Invalid handshake data: {}", msg)
            }
            HandshakeError::ConnectionClosed => {
                write!(f, "Connection closed during handshake")
            }
            HandshakeError::Timeout => {
                write!(f, "Handshake timed out")
            }
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Bolt 프로토콜 에러 코드 상수
///
/// Bolt 프로토콜 명세에 정의된 에러 코드입니다.
pub struct BoltErrorCode;

impl BoltErrorCode {
    // Client errors (recoverable)
    pub const AUTHENTICATION_FAILED: &'static str =
        "Zeta.ClientError.Security.AuthenticationFailed";
    pub const UNAUTHORIZED: &'static str = "Zeta.ClientError.Security.Unauthorized";
    pub const SYNTAX_ERROR: &'static str = "Zeta.ClientError.Statement.SyntaxError";
    pub const SEMANTIC_ERROR: &'static str = "Zeta.ClientError.Statement.SemanticError";
    pub const PARAMETER_MISSING: &'static str = "Zeta.ClientError.Statement.ParameterMissing";
    pub const CONSTRAINT_VIOLATION: &'static str =
        "Zeta.ClientError.Schema.ConstraintValidationFailed";
    pub const TRANSACTION_NOT_FOUND: &'static str =
        "Zeta.ClientError.Transaction.TransactionNotFound";
    pub const INVALID_BOOKMARK: &'static str = "Zeta.ClientError.Transaction.InvalidBookmark";

    // Database errors
    pub const GENERAL_ERROR: &'static str = "Zeta.DatabaseError.General.UnknownError";
    pub const EXECUTION_FAILED: &'static str = "Zeta.DatabaseError.Statement.ExecutionFailed";

    // Transient errors (retry may succeed)
    pub const TRANSACTION_TIMEOUT: &'static str =
        "Zeta.TransientError.Transaction.TransactionTimedOut";
    pub const DEADLOCK_DETECTED: &'static str = "Zeta.TransientError.Transaction.DeadlockDetected";
    pub const DATABASE_UNAVAILABLE: &'static str =
        "Zeta.TransientError.General.DatabaseUnavailable";
    pub const MEMORY_ADMISSION: &'static str = "GR-MEM-001";
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeta4j_common::{ZetaError, codes};

    #[test]
    fn structured_graph_error_reaches_bolt_failure_code() {
        let graph_error = zeta4j_graph::GraphError::Zeta(ZetaError::query_execution_with_code(
            codes::cypher::ADMIN_SERVICE_UNAVAILABLE,
            "admin service missing",
        ));
        let failure = BoltError::from_graph_error(graph_error).to_failure();
        assert_eq!(failure.code, "CY-ADMIN-001");
        assert!(failure.message.contains("admin service missing"));
    }

    #[test]
    fn transaction_lock_codes_reach_bolt_failure() {
        let timeout = BoltError::from_graph_error(zeta4j_graph::GraphError::Transaction(
            zeta4j_graph::transaction::TransactionError::LockTimeout(250),
        ))
        .to_failure();
        assert_eq!(timeout.code, codes::transaction::LOCK_TIMEOUT.to_string());

        let deadlock = BoltError::from_graph_error(zeta4j_graph::GraphError::Transaction(
            zeta4j_graph::transaction::TransactionError::DeadlockDetected(vec![1, 2, 1]),
        ))
        .to_failure();
        assert_eq!(deadlock.code, codes::transaction::DEADLOCK.to_string());
    }

    #[test]
    fn memory_admission_code_reaches_bolt_failure() {
        let graph_error = zeta4j_graph::GraphError::Memory(
            zeta4j_graph::storage::memory::MemoryLimitError::AllocationDenied {
                category: zeta4j_graph::storage::memory::MemoryCategory::Query,
                requested: 2,
                available: 1,
                limit: 1,
            },
        );
        let failure = BoltError::from_graph_error(graph_error).to_failure();
        assert_eq!(failure.code, BoltErrorCode::MEMORY_ADMISSION);
    }
}
