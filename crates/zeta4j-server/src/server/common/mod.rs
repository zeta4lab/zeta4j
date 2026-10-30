//! Common Types and Configuration
//!
//! 서버 공통 타입, 설정, 에러 정의

pub mod config;
pub mod error;

// Config exports
pub use config::{ServerConfig, ServerConfigBuilder, TcpKeepalive};

// Error exports
pub use error::{ConfigError, ConnectionError, ServerError, TrackerError};
