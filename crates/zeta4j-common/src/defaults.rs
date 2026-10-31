// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Default values for Zeta4j configuration
//!
//! 모든 zeta4j 크레이트에서 공유하는 기본값 상수입니다.
//!
//! # 사용법
//!
//! ```rust
//! use zeta4j_common::defaults::*;
//!
//! assert_eq!(DEFAULT_HTTP_PORT, 9044);
//! ```

use std::fmt;
use std::str::FromStr;

// =============================================================================
// Server Ports
// =============================================================================

/// Default HTTP port
pub const DEFAULT_HTTP_PORT: u16 = 9044;

/// Default HTTPS port
pub const DEFAULT_HTTPS_PORT: u16 = 9043;

/// Default Bolt protocol port
pub const DEFAULT_BOLT_PORT: u16 = 9045;

/// Default gRPC port (Admin service)
pub const DEFAULT_GRPC_PORT: u16 = 9046;

// =============================================================================
// Server Settings
// =============================================================================

/// Default bind address (localhost only)
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Default bind address (all interfaces)
pub const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0";

/// Default request timeout (seconds)
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default keep-alive timeout (seconds)
pub const DEFAULT_KEEP_ALIVE_TIMEOUT_SECS: u64 = 60;

/// Default timezone
pub const DEFAULT_TIMEZONE: &str = "utc";

/// Default maximum connections for HTTP
pub const DEFAULT_MAX_CONNECTIONS: usize = 10000;

/// Default maximum body size (16MB)
pub const DEFAULT_MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

/// Default maximum encoded or decoded message size for public gRPC services (16 MiB).
pub const DEFAULT_GRPC_MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Default maximum concurrent RPCs carried by one HTTP/2 connection.
pub const DEFAULT_GRPC_CONCURRENCY_PER_CONNECTION: usize = 128;

/// Default public gRPC request timeout in seconds.
pub const DEFAULT_GRPC_TIMEOUT_SECS: u64 = 30;

/// Default maximum duration for a TLS handshake at a public listener.
pub const DEFAULT_TLS_HANDSHAKE_TIMEOUT_SECS: u64 = 30;

/// Default maximum number of TLS handshakes processed concurrently per listener.
pub const DEFAULT_TLS_HANDSHAKE_CONCURRENCY: usize = 64;

/// Default maximum connections for Bolt
pub const DEFAULT_BOLT_MAX_CONNECTIONS: usize = 1000;

/// Default Bolt connection timeout (seconds)
pub const DEFAULT_BOLT_TIMEOUT_SECS: u64 = 30;

/// Default logical database name
pub const DEFAULT_DATABASE_NAME: &str = "zeta4j";

/// Default maximum databases
pub const DEFAULT_MAX_DATABASES: usize = 100;

/// Default metrics enabled
pub const DEFAULT_METRICS_ENABLED: bool = true;

// =============================================================================
// Query Settings
// =============================================================================

/// Default slow query threshold (milliseconds)
pub const DEFAULT_SLOW_QUERY_THRESHOLD_MS: u64 = 1000;

/// Default health check interval (seconds)
pub const DEFAULT_HEALTH_CHECK_INTERVAL_SECS: u64 = 30;

/// Default query max result size
pub const DEFAULT_MAX_RESULT_SIZE: usize = 100000;

/// Default query timeout (milliseconds)
pub const DEFAULT_QUERY_TIMEOUT_MS: u64 = 30000;

// =============================================================================
// Logging Settings
// =============================================================================

/// Default log level
pub const DEFAULT_LOG_LEVEL: &str = "info";

/// Default log format
pub const DEFAULT_LOG_FORMAT: &str = "text";

/// Default log rotation size (MB)
pub const DEFAULT_LOG_ROTATION_SIZE_MB: u64 = 100;

/// Default log rotation file count
pub const DEFAULT_LOG_ROTATION_COUNT: u32 = 7;

// =============================================================================
// TLS Settings
// =============================================================================

/// Default minimum TLS version
pub const DEFAULT_TLS_MIN_VERSION: &str = "1.2";

// =============================================================================
// Auth Settings
// =============================================================================

/// Default authentication method
pub const DEFAULT_AUTH_METHOD: &str = "basic";

/// Default access token TTL (seconds) - 1 hour
pub const DEFAULT_ACCESS_TOKEN_TTL_SECS: u64 = 3600;

/// Default refresh token TTL (seconds) - 7 days
pub const DEFAULT_REFRESH_TOKEN_TTL_SECS: u64 = 604800;

// =============================================================================
// CORS Settings
// =============================================================================

/// Default CORS max age (seconds)
pub const DEFAULT_CORS_MAX_AGE_SECS: u64 = 3600;

// =============================================================================
// Connection Pool Settings
// =============================================================================

/// Default pool minimum size
pub const DEFAULT_POOL_MIN_SIZE: usize = 1;

/// Default pool maximum size
pub const DEFAULT_POOL_MAX_SIZE: usize = 10;

/// Default idle timeout (seconds)
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;

/// Default acquire timeout (seconds)
pub const DEFAULT_ACQUIRE_TIMEOUT_SECS: u64 = 30;

/// Default connection max lifetime (seconds)
pub const DEFAULT_MAX_LIFETIME_SECS: u64 = 3600;

// =============================================================================
// Vector Index (HNSW) Settings
// =============================================================================

/// Default vector dimensions
pub const DEFAULT_VECTOR_DIMENSIONS: usize = 1536;

/// Default vector distance function
pub const DEFAULT_VECTOR_DISTANCE: &str = "cosine";

/// Default HNSW M parameter (max neighbors per node)
pub const DEFAULT_HNSW_M: usize = 16;

/// Default HNSW M_MAX0 parameter (max neighbors at layer 0)
pub const DEFAULT_HNSW_M_MAX0: usize = 32;

/// Default HNSW ef_construction parameter
pub const DEFAULT_HNSW_EF_CONSTRUCTION: usize = 200;

/// Default HNSW ef_search parameter
pub const DEFAULT_HNSW_EF_SEARCH: usize = 50;

/// Default vector page offset
pub const DEFAULT_VECTOR_PAGE_OFFSET: usize = 10000;

// =============================================================================
// Vector Search Settings
// =============================================================================

/// Default vector search limit
pub const DEFAULT_VECTOR_SEARCH_LIMIT: usize = 10;

/// Default vector search max limit
pub const DEFAULT_VECTOR_SEARCH_MAX_LIMIT: usize = 1000;

/// Default vector search timeout (milliseconds)
pub const DEFAULT_VECTOR_SEARCH_TIMEOUT_MS: u64 = 5000;

/// Default vector inline threshold (dimensions)
pub const DEFAULT_VECTOR_INLINE_THRESHOLD: usize = 64;

/// Default vector cache size
pub const DEFAULT_VECTOR_CACHE_SIZE: usize = 10000;

// =============================================================================
// Cypher Safety Utilities
// =============================================================================

// =============================================================================
// Admin Job Status
// =============================================================================

/// Admin job 상태: 실행 중
pub const STATUS_RUNNING: &str = "running";
/// Admin job 상태: 완료
pub const STATUS_COMPLETED: &str = "completed";
/// Admin job 상태: 실패
pub const STATUS_FAILED: &str = "failed";

// =============================================================================
// Edition
// =============================================================================

/// Zeta4j 에디션 (단일 제품)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Edition {
    /// 그래프 DB 서버 + Vector/Fulltext
    #[default]
    Base,
}

impl Edition {
    /// 에디션 이름 (표시용)
    pub fn name(&self) -> &'static str {
        "Base"
    }

    /// 소문자 식별자
    pub fn as_str(&self) -> &'static str {
        "base"
    }

    /// 에디션 설명
    pub fn description(&self) -> &'static str {
        "Graph Database Server with Vector and Fulltext"
    }

    /// 로그 파일 이름
    pub fn log_file_name(&self) -> &'static str {
        "zeta4j.log"
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Edition {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "base" => Ok(Self::Base),
            _ => Err(format!("Unknown edition: '{}'", s)),
        }
    }
}

// =============================================================================
// Cypher Utilities
// =============================================================================

/// Cypher 문자열 리터럴에 삽입할 값을 이스케이프
///
/// single quote, double quote, backslash, newline 등을 이스케이프하여
/// Cypher 인젝션을 방지합니다.
pub fn escape_cypher_string(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\'' => escaped.push_str("\\'"),
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Cypher 정규식(`=~`) 연산자에 삽입할 값을 이스케이프
///
/// regex 메타문자를 이스케이프하여 ReDoS 및 패턴 인젝션을 방지합니다.
pub fn escape_cypher_regex(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    escaped
}
