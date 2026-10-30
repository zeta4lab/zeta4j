//! 서버 설정 모듈
//!
//! HTTP, Bolt, gRPC, transport와 인증 설정을 통합 관리합니다.
//!
//! # 예제 설정 파일
//!
//! ```toml
//! # ~/.zeta4j/server.toml
//!
//! [http]
//! bind = "0.0.0.0"
//! port = 9044
//! max_connections = 10000
//! request_timeout_secs = 30
//! max_body_size = 16777216  # 16MB
//! compression = true
//!
//! [http.cors]
//! allowed_origins = ["*"]
//! allow_credentials = false
//!
//! [bolt]
//! bind = "0.0.0.0"
//! port = 9045
//! max_connections = 1000
//! connection_timeout_secs = 30
//! [grpc]
//! bind = "0.0.0.0"
//! port = 9046
//!
//! [transport]
//! mode = "tls"
//! server_cert_ref = "file:/etc/zeta4j/tls/server.crt"
//! server_key_ref = "file:/etc/zeta4j/tls/server.key"
//!
//! [auth]
//! enabled = true
//! jwt_signing_key_file = "${ZETA4J_JWT_SIGNING_KEY_FILE}"
//! jwt_key_id = "key-1"
//! jwt_key_version = 1
//! access_token_ttl_secs = 3600
//! refresh_token_ttl_secs = 604800
//! ```

use serde::{Deserialize, Serialize};
use zeta4j_common::{RuntimeProfile, defaults::*};

/// 통합 서버 설정
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UnifiedServerConfig {
    /// Runtime security profile.
    #[serde(default)]
    pub runtime: RuntimeServerConfig,

    /// HTTP 서버 설정
    #[serde(default)]
    pub http: HttpServerConfig,

    /// Bolt 프로토콜 설정
    #[serde(default)]
    pub bolt: BoltServerConfig,

    /// gRPC 설정
    #[serde(default)]
    pub grpc: GrpcServerConfig,

    /// Canonical transport security settings.
    #[serde(default)]
    pub transport: TransportServerConfig,

    /// 인증 설정
    #[serde(default)]
    pub auth: AuthServerConfig,
}

/// Runtime security settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RuntimeServerConfig {
    /// Production is the fail-closed default.
    #[serde(default)]
    pub profile: RuntimeProfile,
}

impl UnifiedServerConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }
}

// ============================================================================
// HTTP Server Config
// ============================================================================

/// HTTP 서버 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpServerConfig {
    /// 바인딩 주소 (기본: 0.0.0.0)
    #[serde(default = "default_bind")]
    pub bind: String,

    /// HTTP 포트 (기본: 9044)
    #[serde(default = "default_http_port")]
    pub port: u16,

    /// 최대 동시 연결 (기본: 10000)
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// 요청 타임아웃 (초, 기본: 30)
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// 최대 요청 본문 크기 (기본: 16MB)
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,

    /// 압축 활성화 (기본: true)
    #[serde(default = "default_true")]
    pub compression: bool,

    /// Keep-alive 타임아웃 (초, 기본: 60)
    #[serde(default = "default_keep_alive_timeout")]
    pub keep_alive_timeout_secs: u64,

    /// 시간대 (utc, local)
    #[serde(default = "default_timezone")]
    pub timezone: String,

    /// CORS 설정
    #[serde(default)]
    pub cors: CorsServerConfig,
}

fn default_bind() -> String {
    DEFAULT_BIND_ADDRESS.to_string()
}
fn default_http_port() -> u16 {
    DEFAULT_HTTP_PORT
}
fn default_max_connections() -> usize {
    DEFAULT_MAX_CONNECTIONS
}
fn default_request_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECS
}
fn default_max_body_size() -> usize {
    DEFAULT_MAX_BODY_SIZE
}
fn default_keep_alive_timeout() -> u64 {
    DEFAULT_KEEP_ALIVE_TIMEOUT_SECS
}
fn default_timezone() -> String {
    DEFAULT_TIMEZONE.to_string()
}
fn default_true() -> bool {
    true
}

impl Default for HttpServerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND_ADDRESS.to_string(),
            port: DEFAULT_HTTP_PORT,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            request_timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            compression: true,
            keep_alive_timeout_secs: DEFAULT_KEEP_ALIVE_TIMEOUT_SECS,
            timezone: DEFAULT_TIMEZONE.to_string(),
            cors: CorsServerConfig::default(),
        }
    }
}

impl HttpServerConfig {
    /// SocketAddr 생성
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind, self.port)
    }
}

/// CORS 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorsServerConfig {
    /// 허용된 오리진 (기본: ["*"])
    #[serde(default = "default_allowed_origins")]
    pub allowed_origins: Vec<String>,

    /// 허용된 메서드
    #[serde(default = "default_allowed_methods")]
    pub allowed_methods: Vec<String>,

    /// 허용된 헤더
    #[serde(default = "default_allowed_headers")]
    pub allowed_headers: Vec<String>,

    /// 자격 증명 허용 (기본: false)
    #[serde(default)]
    pub allow_credentials: bool,

    /// preflight 캐시 시간 (초)
    #[serde(default = "default_cors_max_age")]
    pub max_age_secs: u64,
}

fn default_allowed_origins() -> Vec<String> {
    vec!["*".to_string()]
}
fn default_allowed_methods() -> Vec<String> {
    vec![
        "GET".to_string(),
        "POST".to_string(),
        "PUT".to_string(),
        "DELETE".to_string(),
        "OPTIONS".to_string(),
    ]
}
fn default_allowed_headers() -> Vec<String> {
    vec![
        "content-type".to_string(),
        "authorization".to_string(),
        "x-request-id".to_string(),
        "accept".to_string(),
    ]
}
fn default_cors_max_age() -> u64 {
    DEFAULT_CORS_MAX_AGE_SECS
}

impl Default for CorsServerConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_allowed_origins(),
            allowed_methods: default_allowed_methods(),
            allowed_headers: default_allowed_headers(),
            allow_credentials: false,
            max_age_secs: DEFAULT_CORS_MAX_AGE_SECS,
        }
    }
}

// ============================================================================
// Bolt Server Config
// ============================================================================

/// Bolt 프로토콜 서버 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoltServerConfig {
    /// 바인딩 주소 (기본: 0.0.0.0)
    #[serde(default = "default_bind")]
    pub bind: String,

    /// Bolt 포트 (기본: 9045)
    #[serde(default = "default_bolt_port")]
    pub port: u16,

    /// 최대 동시 연결 (기본: 1000)
    #[serde(default = "default_bolt_max_connections")]
    pub max_connections: usize,

    /// 연결 타임아웃 (초, 기본: 30)
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,

    /// 기본 데이터베이스 (기본: zeta4j)
    #[serde(default = "default_database")]
    pub default_database: String,
}

fn default_bolt_port() -> u16 {
    DEFAULT_BOLT_PORT
}
fn default_bolt_max_connections() -> usize {
    DEFAULT_BOLT_MAX_CONNECTIONS
}
fn default_connection_timeout() -> u64 {
    DEFAULT_BOLT_TIMEOUT_SECS
}
fn default_database() -> String {
    DEFAULT_DATABASE_NAME.to_string()
}

impl Default for BoltServerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND_ADDRESS.to_string(),
            port: DEFAULT_BOLT_PORT,
            max_connections: DEFAULT_BOLT_MAX_CONNECTIONS,
            connection_timeout_secs: DEFAULT_BOLT_TIMEOUT_SECS,
            default_database: DEFAULT_DATABASE_NAME.to_string(),
        }
    }
}

impl BoltServerConfig {
    /// SocketAddr 생성
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind, self.port)
    }
}

// ============================================================================
// gRPC Server Config
// ============================================================================

/// gRPC 서버 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrpcServerConfig {
    /// 바인딩 주소
    #[serde(default = "default_bind")]
    pub bind: String,

    /// gRPC 포트
    #[serde(default = "default_grpc_port")]
    pub port: u16,
}

fn default_grpc_port() -> u16 {
    DEFAULT_GRPC_PORT
}

impl Default for GrpcServerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND_ADDRESS.to_string(),
            port: DEFAULT_GRPC_PORT,
        }
    }
}

// ============================================================================
// Transport security config
// ============================================================================

/// TLS material references. Values are references, never inline secrets.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportServerConfig {
    /// `plaintext` or `tls`.
    #[serde(default = "default_transport_mode")]
    pub mode: String,
    /// Server certificate-chain reference (`file:<path>`).
    #[serde(default)]
    pub server_cert_ref: String,
    /// Server private-key reference (`file:<path>`).
    #[serde(default)]
    pub server_key_ref: String,
}

impl std::fmt::Debug for TransportServerConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransportServerConfig")
            .field("mode", &self.mode)
            .field("server_cert_ref", &"[REDACTED REFERENCE]")
            .field("server_key_ref", &"[REDACTED REFERENCE]")
            .finish()
    }
}

fn default_transport_mode() -> String {
    "plaintext".to_string()
}

impl Default for TransportServerConfig {
    fn default() -> Self {
        Self {
            mode: default_transport_mode(),
            server_cert_ref: String::new(),
            server_key_ref: String::new(),
        }
    }
}

// ============================================================================
// Auth Config
// ============================================================================

/// 인증 설정
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthServerConfig {
    /// 인증 활성화 (기본: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Ed25519 PKCS#8 signer file reference.
    #[serde(default)]
    pub jwt_signing_key_file: String,

    /// Public signing key identifier.
    #[serde(default = "default_jwt_key_id")]
    pub jwt_key_id: String,

    /// Monotonically increasing signing key version.
    #[serde(default = "default_jwt_key_version")]
    pub jwt_key_version: u64,

    /// Access Token TTL (초, 기본: 3600 = 1시간)
    #[serde(default = "default_access_token_ttl")]
    pub access_token_ttl_secs: u64,

    /// Refresh Token TTL (초, 기본: 604800 = 7일)
    #[serde(default = "default_refresh_token_ttl")]
    pub refresh_token_ttl_secs: u64,
}

impl std::fmt::Debug for AuthServerConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthServerConfig")
            .field("enabled", &self.enabled)
            .field("jwt_signing_key_file", &"[REDACTED REFERENCE]")
            .field("jwt_key_id", &self.jwt_key_id)
            .field("jwt_key_version", &self.jwt_key_version)
            .field("access_token_ttl_secs", &self.access_token_ttl_secs)
            .field("refresh_token_ttl_secs", &self.refresh_token_ttl_secs)
            .finish()
    }
}

fn default_access_token_ttl() -> u64 {
    DEFAULT_ACCESS_TOKEN_TTL_SECS
}
fn default_refresh_token_ttl() -> u64 {
    DEFAULT_REFRESH_TOKEN_TTL_SECS
}
fn default_jwt_key_id() -> String {
    "key-1".to_string()
}
fn default_jwt_key_version() -> u64 {
    1
}
impl Default for AuthServerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            jwt_signing_key_file: std::env::var("ZETA4J_JWT_SIGNING_KEY_FILE").unwrap_or_default(),
            jwt_key_id: std::env::var("ZETA4J_JWT_KEY_ID").unwrap_or_else(|_| default_jwt_key_id()),
            jwt_key_version: std::env::var("ZETA4J_JWT_KEY_VERSION")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(default_jwt_key_version),
            access_token_ttl_secs: DEFAULT_ACCESS_TOKEN_TTL_SECS,
            refresh_token_ttl_secs: DEFAULT_REFRESH_TOKEN_TTL_SECS,
        }
    }
}

impl AuthServerConfig {
    /// 인증 비활성화
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}
