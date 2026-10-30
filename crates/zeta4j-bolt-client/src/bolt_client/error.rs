//! Bolt 클라이언트 에러 타입.

use std::io;

use zeta4j_bolt_protocol::packstream::PackStreamError;

/// Bolt 클라이언트 에러
#[derive(Debug)]
pub enum BoltClientError {
    /// I/O 에러
    Io(io::Error),
    /// 연결 실패
    ConnectionFailed(String),
    /// 핸드셰이크 실패
    HandshakeFailed(String),
    /// 인증 실패
    AuthenticationFailed(String),
    /// 프로토콜 에러
    ProtocolError(String),
    /// 쿼리 실패
    QueryFailed { code: String, message: String },
    /// 연결되지 않음
    NotConnected,
    /// 타임아웃
    Timeout,
    /// TLS 에러
    Tls(String),
}

impl std::fmt::Display for BoltClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoltClientError::Io(e) => write!(f, "I/O error: {}", e),
            BoltClientError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            BoltClientError::HandshakeFailed(msg) => write!(f, "Handshake failed: {}", msg),
            BoltClientError::AuthenticationFailed(msg) => {
                write!(f, "Authentication failed: {}", msg)
            }
            BoltClientError::ProtocolError(msg) => write!(f, "Protocol error: {}", msg),
            BoltClientError::QueryFailed { code, message } => write!(f, "{}: {}", code, message),
            BoltClientError::NotConnected => write!(f, "Not connected to server"),
            BoltClientError::Timeout => write!(f, "Connection timed out"),
            BoltClientError::Tls(msg) => write!(f, "TLS error: {}", msg),
        }
    }
}

impl std::error::Error for BoltClientError {}

impl From<io::Error> for BoltClientError {
    fn from(err: io::Error) -> Self {
        BoltClientError::Io(err)
    }
}

impl From<PackStreamError> for BoltClientError {
    fn from(err: PackStreamError) -> Self {
        BoltClientError::ProtocolError(format!("PackStream error: {}", err))
    }
}

/// Bolt 클라이언트 결과 타입
pub type BoltClientResult<T> = Result<T, BoltClientError>;
