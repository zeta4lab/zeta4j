//! HTTP Server Configuration

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// Startup ceiling for a single HTTP request body (256 MiB).
pub const MAX_HTTP_BODY_SIZE: usize = 256 * 1024 * 1024;
/// Startup ceiling for simultaneously accepted HTTP connections.
pub const MAX_HTTP_CONNECTIONS: usize = 100_000;
/// Startup ceiling for a request or idle connection timeout.
pub const MAX_HTTP_TIMEOUT: Duration = Duration::from_secs(3_600);

/// HTTP 서버 설정
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// 바인딩 주소
    pub bind_addr: SocketAddr,
    /// 요청 타임아웃
    pub request_timeout: Duration,
    /// 최대 요청 본문 크기
    pub max_body_size: usize,
    /// CORS 설정
    pub cors: CorsConfig,
    /// 압축 활성화
    pub compression: bool,
    /// Keep-alive 타임아웃
    pub keep_alive_timeout: Duration,
    /// 최대 동시 연결
    pub max_connections: usize,
    /// 인증 설정
    pub auth: AuthHttpConfig,
    /// Rate Limiting 설정
    pub rate_limit: RateLimitHttpConfig,
    /// 서버 시간대 (utc, local)
    pub timezone: String,
}

impl HttpConfig {
    /// Reject values that would disable admission or permit accidental unbounded allocation.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_body_size == 0 || self.max_body_size > MAX_HTTP_BODY_SIZE {
            return Err(format!(
                "max_body_size must be between 1 and {MAX_HTTP_BODY_SIZE} bytes"
            ));
        }
        if self.max_connections == 0 || self.max_connections > MAX_HTTP_CONNECTIONS {
            return Err(format!(
                "max_connections must be between 1 and {MAX_HTTP_CONNECTIONS}"
            ));
        }
        if self.request_timeout.is_zero() || self.request_timeout > MAX_HTTP_TIMEOUT {
            return Err(format!(
                "request_timeout must be between 1ns and {} seconds",
                MAX_HTTP_TIMEOUT.as_secs()
            ));
        }
        if self.keep_alive_timeout.is_zero() || self.keep_alive_timeout > MAX_HTTP_TIMEOUT {
            return Err(format!(
                "keep_alive_timeout must be between 1ns and {} seconds",
                MAX_HTTP_TIMEOUT.as_secs()
            ));
        }
        Ok(())
    }
}

/// Rate Limiting 설정
#[derive(Debug, Clone)]
pub struct RateLimitHttpConfig {
    /// Rate Limiting 활성화 여부
    pub enabled: bool,
    /// 윈도우당 최대 요청 수 (기본: 1000)
    pub requests_per_window: u64,
    /// 윈도우 크기 (기본: 60초)
    pub window_duration: Duration,
    /// 버스트 허용량 (순간적 추가 요청, 기본: 100)
    pub burst_size: u64,
    /// 제한 초과 시 응답 헤더 포함 여부
    pub include_headers: bool,
    /// Peers allowed to supply a single-hop forwarded client address.
    pub trusted_proxy_ips: Vec<String>,
    /// Hard upper bound for retained client buckets.
    pub max_client_buckets: usize,
    /// Rate Limit 제외 경로
    pub excluded_paths: Vec<String>,
}

impl Default for RateLimitHttpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_window: 1000,
            window_duration: Duration::from_secs(60),
            burst_size: 100,
            include_headers: true,
            trusted_proxy_ips: Vec::new(),
            max_client_buckets: 4096,
            excluded_paths: vec![
                "/health".to_string(),
                "/ready".to_string(),
                "/metrics".to_string(),
            ],
        }
    }
}

impl RateLimitHttpConfig {
    /// Rate Limiting 비활성화
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// 요청 수 설정
    pub fn with_requests_per_window(mut self, requests: u64) -> Self {
        self.requests_per_window = requests;
        self
    }

    /// 윈도우 크기 설정
    pub fn with_window_duration(mut self, duration: Duration) -> Self {
        self.window_duration = duration;
        self
    }

    /// 버스트 크기 설정
    pub fn with_burst_size(mut self, size: u64) -> Self {
        self.burst_size = size;
        self
    }

    /// Add a peer allowed to provide a single forwarded client address.
    pub fn with_trusted_proxy_ip(mut self, ip: impl Into<String>) -> Self {
        self.trusted_proxy_ips.push(ip.into());
        self
    }

    /// 제외 경로 추가
    pub fn with_excluded_path(mut self, path: impl Into<String>) -> Self {
        self.excluded_paths.push(path.into());
        self
    }

    /// 엄격한 Rate Limiting (낮은 한도)
    pub fn strict() -> Self {
        Self {
            enabled: true,
            requests_per_window: 100,
            window_duration: Duration::from_secs(60),
            burst_size: 10,
            include_headers: true,
            trusted_proxy_ips: Vec::new(),
            max_client_buckets: 1024,
            excluded_paths: vec!["/health".to_string()],
        }
    }

    /// 관대한 Rate Limiting (높은 한도)
    pub fn permissive() -> Self {
        Self {
            enabled: true,
            requests_per_window: 10000,
            window_duration: Duration::from_secs(60),
            burst_size: 1000,
            include_headers: true,
            trusted_proxy_ips: Vec::new(),
            max_client_buckets: 16_384,
            excluded_paths: vec![
                "/health".to_string(),
                "/ready".to_string(),
                "/metrics".to_string(),
            ],
        }
    }
}

/// HTTP 인증 설정
#[derive(Clone)]
pub struct AuthHttpConfig {
    /// 인증 활성화 여부
    pub enabled: bool,
    /// Ed25519 PKCS#8 signing key file reference.
    pub jwt_signing_key_file: PathBuf,
    /// Public key identifier.
    pub jwt_key_id: String,
    /// Monotonic signing key version.
    pub jwt_key_version: u64,
    /// Access Token 만료 시간
    pub access_token_ttl: Duration,
    /// Refresh Token 만료 시간
    pub refresh_token_ttl: Duration,
}

impl std::fmt::Debug for AuthHttpConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthHttpConfig")
            .field("enabled", &self.enabled)
            .field("jwt_signing_key_file", &"[REDACTED REFERENCE]")
            .field("jwt_key_id", &self.jwt_key_id)
            .field("jwt_key_version", &self.jwt_key_version)
            .field("access_token_ttl", &self.access_token_ttl)
            .field("refresh_token_ttl", &self.refresh_token_ttl)
            .finish()
    }
}

impl Default for AuthHttpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            jwt_signing_key_file: std::env::var_os("ZETA4J_JWT_SIGNING_KEY_FILE")
                .map(PathBuf::from)
                .unwrap_or_default(),
            jwt_key_id: std::env::var("ZETA4J_JWT_KEY_ID").unwrap_or_else(|_| "key-1".to_string()),
            jwt_key_version: std::env::var("ZETA4J_JWT_KEY_VERSION")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1),
            access_token_ttl: Duration::from_secs(3600), // 1시간
            refresh_token_ttl: Duration::from_secs(86400 * 7), // 7일
        }
    }
}

impl AuthHttpConfig {
    /// 인증 비활성화
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind_addr: ([127, 0, 0, 1], 9044).into(),
            request_timeout: Duration::from_secs(30),
            max_body_size: 16 * 1024 * 1024, // 16MB
            cors: CorsConfig::default(),
            compression: true,
            keep_alive_timeout: Duration::from_secs(60),
            max_connections: 10000,
            auth: AuthHttpConfig::default(),
            rate_limit: RateLimitHttpConfig::default(),
            timezone: "utc".to_string(),
        }
    }
}

/// HTTP 설정 빌더
#[derive(Debug, Default)]
pub struct HttpConfigBuilder {
    bind_addr: Option<SocketAddr>,
    request_timeout: Option<Duration>,
    max_body_size: Option<usize>,
    cors: Option<CorsConfig>,
    compression: Option<bool>,
    keep_alive_timeout: Option<Duration>,
    max_connections: Option<usize>,
    auth: Option<AuthHttpConfig>,
    rate_limit: Option<RateLimitHttpConfig>,
    timezone: Option<String>,
}

impl HttpConfigBuilder {
    /// 새 빌더 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 바인딩 주소 설정
    pub fn bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = Some(addr);
        self
    }

    /// 바인딩 주소 문자열로 설정
    pub fn bind(mut self, addr: &str) -> Result<Self, std::net::AddrParseError> {
        self.bind_addr = Some(addr.parse()?);
        Ok(self)
    }

    /// 요청 타임아웃 설정
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// 요청 타임아웃 (초) 설정
    pub fn request_timeout_secs(mut self, secs: u64) -> Self {
        self.request_timeout = Some(Duration::from_secs(secs));
        self
    }

    /// 최대 요청 본문 크기 설정
    pub fn max_body_size(mut self, size: usize) -> Self {
        self.max_body_size = Some(size);
        self
    }

    /// CORS 설정
    pub fn cors(mut self, config: CorsConfig) -> Self {
        self.cors = Some(config);
        self
    }

    /// 압축 활성화/비활성화
    pub fn compression(mut self, enabled: bool) -> Self {
        self.compression = Some(enabled);
        self
    }

    /// Keep-alive 타임아웃 설정
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.keep_alive_timeout = Some(timeout);
        self
    }

    /// 최대 동시 연결 수 설정
    pub fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = Some(max);
        self
    }

    /// 인증 설정
    pub fn auth(mut self, config: AuthHttpConfig) -> Self {
        self.auth = Some(config);
        self
    }

    /// 인증 비활성화
    pub fn disable_auth(mut self) -> Self {
        self.auth = Some(AuthHttpConfig::disabled());
        self
    }

    /// Rate Limiting 설정
    pub fn rate_limit(mut self, config: RateLimitHttpConfig) -> Self {
        self.rate_limit = Some(config);
        self
    }

    /// Rate Limiting 비활성화
    pub fn disable_rate_limit(mut self) -> Self {
        self.rate_limit = Some(RateLimitHttpConfig::disabled());
        self
    }

    /// 시간대 설정 (utc, local)
    pub fn timezone(mut self, tz: impl Into<String>) -> Self {
        self.timezone = Some(tz.into());
        self
    }

    /// 설정 빌드
    pub fn build(self) -> HttpConfig {
        let default = HttpConfig::default();
        HttpConfig {
            bind_addr: self.bind_addr.unwrap_or(default.bind_addr),
            request_timeout: self.request_timeout.unwrap_or(default.request_timeout),
            max_body_size: self.max_body_size.unwrap_or(default.max_body_size),
            cors: self.cors.unwrap_or(default.cors),
            compression: self.compression.unwrap_or(default.compression),
            keep_alive_timeout: self
                .keep_alive_timeout
                .unwrap_or(default.keep_alive_timeout),
            max_connections: self.max_connections.unwrap_or(default.max_connections),
            auth: self.auth.unwrap_or(default.auth),
            rate_limit: self.rate_limit.unwrap_or(default.rate_limit),
            timezone: self.timezone.unwrap_or(default.timezone),
        }
    }
}

/// CORS 설정
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// 허용된 오리진
    pub allowed_origins: Vec<String>,
    /// 허용된 메서드
    pub allowed_methods: Vec<String>,
    /// 허용된 헤더
    pub allowed_headers: Vec<String>,
    /// 노출 헤더
    pub expose_headers: Vec<String>,
    /// preflight 캐시 시간
    pub max_age: Duration,
    /// 자격 증명 허용
    pub allow_credentials: bool,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "OPTIONS".to_string(),
            ],
            allowed_headers: vec![
                "content-type".to_string(),
                "authorization".to_string(),
                "x-api-key".to_string(),
                "x-request-id".to_string(),
                "accept".to_string(),
            ],
            expose_headers: vec!["x-request-id".to_string()],
            max_age: Duration::from_secs(3600),
            allow_credentials: false,
        }
    }
}

impl CorsConfig {
    /// 모든 오리진 허용
    pub fn permissive() -> Self {
        Self::default()
    }

    /// 특정 오리진만 허용
    pub fn strict(origins: Vec<String>) -> Self {
        Self {
            allowed_origins: origins,
            allow_credentials: true,
            ..Self::default()
        }
    }

    /// CORS 비활성화 (동일 오리진만)
    pub fn disabled() -> Self {
        Self {
            allowed_origins: vec![],
            allowed_methods: vec![],
            allowed_headers: vec![],
            expose_headers: vec![],
            max_age: Duration::ZERO,
            allow_credentials: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_admission_rejects_disabled_and_excessive_limits() {
        let mut config = HttpConfig::default();
        assert!(config.validate().is_ok());

        config.max_body_size = 0;
        assert!(config.validate().is_err());
        config.max_body_size = MAX_HTTP_BODY_SIZE + 1;
        assert!(config.validate().is_err());

        config.max_body_size = 1;
        config.max_connections = 0;
        assert!(config.validate().is_err());
        config.max_connections = MAX_HTTP_CONNECTIONS + 1;
        assert!(config.validate().is_err());

        config.max_connections = 1;
        config.request_timeout = Duration::ZERO;
        assert!(config.validate().is_err());
        config.request_timeout = Duration::from_secs(1);
        config.keep_alive_timeout = MAX_HTTP_TIMEOUT + Duration::from_nanos(1);
        assert!(config.validate().is_err());
    }
}
