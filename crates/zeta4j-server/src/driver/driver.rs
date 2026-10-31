// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! M9.1: Driver
//!
//! 드라이버 인스턴스 및 설정

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use super::error::{DriverError, DriverResult};
use super::pool::{ConnectionPool, PoolConfig};
use super::session::{Session, SessionConfig};

// ============================================================================
// AuthToken - 인증 토큰
// ============================================================================

/// 인증 토큰
#[derive(Debug, Clone, Default)]
pub enum AuthToken {
    /// 인증 없음
    #[default]
    None,
    /// Basic 인증 (사용자명/비밀번호)
    Basic {
        username: String,
        password: String,
        realm: Option<String>,
    },
    /// Bearer 토큰
    Bearer { token: String },
    /// Kerberos 인증
    Kerberos { ticket: String },
    /// 커스텀 인증
    Custom {
        principal: String,
        credentials: String,
        realm: String,
        scheme: String,
        parameters: Option<std::collections::HashMap<String, String>>,
    },
}

impl AuthToken {
    /// Basic 인증 토큰 생성
    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Basic {
            username: username.into(),
            password: password.into(),
            realm: None,
        }
    }

    /// Basic 인증 토큰 생성 (realm 포함)
    pub fn basic_with_realm(
        username: impl Into<String>,
        password: impl Into<String>,
        realm: impl Into<String>,
    ) -> Self {
        Self::Basic {
            username: username.into(),
            password: password.into(),
            realm: Some(realm.into()),
        }
    }

    /// Bearer 토큰 생성
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::Bearer {
            token: token.into(),
        }
    }

    /// Kerberos 토큰 생성
    pub fn kerberos(ticket: impl Into<String>) -> Self {
        Self::Kerberos {
            ticket: ticket.into(),
        }
    }

    /// 인증 없음
    pub fn none() -> Self {
        Self::None
    }

    /// 인증 스킴
    pub fn scheme(&self) -> &str {
        match self {
            Self::None => "none",
            Self::Basic { .. } => "basic",
            Self::Bearer { .. } => "bearer",
            Self::Kerberos { .. } => "kerberos",
            Self::Custom { scheme, .. } => scheme,
        }
    }
}

// ============================================================================
// TrustStrategy - TLS 신뢰 전략
// ============================================================================

/// TLS 신뢰 전략
#[derive(Debug, Clone, Default)]
pub enum TrustStrategy {
    /// 시스템 인증서 사용
    #[default]
    TrustSystemCas,
    /// 모든 인증서 신뢰 (개발용)
    TrustAllCertificates,
    /// 특정 인증서만 신뢰
    TrustCustomCas { certificates: Vec<Vec<u8>> },
}

// ============================================================================
// ServerAddress - 서버 주소
// ============================================================================

/// 서버 주소
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerAddress {
    /// 호스트
    pub host: String,
    /// 포트
    pub port: u16,
}

impl ServerAddress {
    /// 새 서버 주소 생성
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    /// URI에서 파싱
    pub fn from_uri(uri: &str) -> DriverResult<Self> {
        // bolt://host:port 또는 zeta4j://host:port 형식 파싱
        let uri = uri
            .trim_start_matches("bolt://")
            .trim_start_matches("bolt+s://")
            .trim_start_matches("bolt+ssc://")
            .trim_start_matches("zeta4j://")
            .trim_start_matches("zeta4j+s://")
            .trim_start_matches("zeta4j+ssc://");

        let parts: Vec<&str> = uri.split(':').collect();
        match parts.len() {
            1 => Ok(Self::new(parts[0], 9045)),
            2 => {
                let port = parts[1]
                    .parse()
                    .map_err(|_| DriverError::configuration("Invalid port"))?;
                Ok(Self::new(parts[0], port))
            }
            _ => Err(DriverError::configuration("Invalid server address")),
        }
    }

    /// 소켓 주소로 변환
    pub fn to_socket_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl fmt::Display for ServerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

impl Default for ServerAddress {
    fn default() -> Self {
        Self::new("localhost", 9045)
    }
}

// ============================================================================
// DriverConfig - 드라이버 설정
// ============================================================================

/// 드라이버 설정
#[derive(Debug, Clone)]
pub struct DriverConfig {
    /// 서버 주소
    pub address: ServerAddress,
    /// 인증 토큰
    pub auth: AuthToken,
    /// TLS 암호화
    pub encrypted: bool,
    /// TLS 신뢰 전략
    pub trust_strategy: TrustStrategy,
    /// 연결 풀 최대 크기
    pub max_connection_pool_size: usize,
    /// 연결 획득 타임아웃
    pub connection_acquisition_timeout: Duration,
    /// 연결 타임아웃
    pub connection_timeout: Duration,
    /// 최대 트랜잭션 재시도 시간
    pub max_transaction_retry_time: Duration,
    /// Keep-Alive 활성화
    pub keep_alive: bool,
    /// User Agent
    pub user_agent: String,
    /// Fetch Size
    pub fetch_size: usize,
}

impl DriverConfig {
    /// 새 설정 생성
    pub fn new(uri: &str, auth: AuthToken) -> DriverResult<Self> {
        let encrypted = uri.contains("+s://") || uri.contains("+ssc://");
        let address = ServerAddress::from_uri(uri)?;

        Ok(Self {
            address,
            auth,
            encrypted,
            trust_strategy: TrustStrategy::default(),
            max_connection_pool_size: 100,
            connection_acquisition_timeout: Duration::from_secs(60),
            connection_timeout: Duration::from_secs(30),
            max_transaction_retry_time: Duration::from_secs(30),
            keep_alive: true,
            user_agent: format!("Zeta4j/{}", env!("CARGO_PKG_VERSION")),
            fetch_size: 1000,
        })
    }

    /// 빌더 시작
    pub fn builder(uri: &str, auth: AuthToken) -> DriverResult<DriverConfigBuilder> {
        let config = Self::new(uri, auth)?;
        Ok(DriverConfigBuilder { config })
    }
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            address: ServerAddress::default(),
            auth: AuthToken::default(),
            encrypted: false,
            trust_strategy: TrustStrategy::default(),
            max_connection_pool_size: 100,
            connection_acquisition_timeout: Duration::from_secs(60),
            connection_timeout: Duration::from_secs(30),
            max_transaction_retry_time: Duration::from_secs(30),
            keep_alive: true,
            user_agent: format!("Zeta4j/{}", env!("CARGO_PKG_VERSION")),
            fetch_size: 1000,
        }
    }
}

// ============================================================================
// DriverConfigBuilder - 설정 빌더
// ============================================================================

/// 드라이버 설정 빌더
pub struct DriverConfigBuilder {
    config: DriverConfig,
}

impl DriverConfigBuilder {
    /// TLS 암호화 설정
    pub fn with_encrypted(mut self, encrypted: bool) -> Self {
        self.config.encrypted = encrypted;
        self
    }

    /// TLS 신뢰 전략 설정
    pub fn with_trust_strategy(mut self, strategy: TrustStrategy) -> Self {
        self.config.trust_strategy = strategy;
        self
    }

    /// 연결 풀 크기 설정
    pub fn with_max_connection_pool_size(mut self, size: usize) -> Self {
        self.config.max_connection_pool_size = size;
        self
    }

    /// 연결 획득 타임아웃 설정
    pub fn with_connection_acquisition_timeout(mut self, timeout: Duration) -> Self {
        self.config.connection_acquisition_timeout = timeout;
        self
    }

    /// 연결 타임아웃 설정
    pub fn with_connection_timeout(mut self, timeout: Duration) -> Self {
        self.config.connection_timeout = timeout;
        self
    }

    /// 최대 트랜잭션 재시도 시간 설정
    pub fn with_max_transaction_retry_time(mut self, time: Duration) -> Self {
        self.config.max_transaction_retry_time = time;
        self
    }

    /// Keep-Alive 설정
    pub fn with_keep_alive(mut self, keep_alive: bool) -> Self {
        self.config.keep_alive = keep_alive;
        self
    }

    /// User Agent 설정
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config.user_agent = user_agent.into();
        self
    }

    /// Fetch Size 설정
    pub fn with_fetch_size(mut self, size: usize) -> Self {
        self.config.fetch_size = size;
        self
    }

    /// 빌드
    pub fn build(self) -> DriverConfig {
        self.config
    }
}

// ============================================================================
// Driver - 드라이버
// ============================================================================

/// 그래프 데이터베이스 드라이버
pub struct Driver {
    /// 설정
    config: DriverConfig,
    /// 연결 풀
    pool: Arc<ConnectionPool>,
    /// 열린 상태
    open: Arc<RwLock<bool>>,
}

impl Driver {
    /// 새 드라이버 생성
    pub fn new(uri: &str, auth: AuthToken) -> DriverResult<Self> {
        let config = DriverConfig::new(uri, auth)?;
        Self::with_config(config)
    }

    /// 설정으로 드라이버 생성
    pub fn with_config(config: DriverConfig) -> DriverResult<Self> {
        let pool_config = PoolConfig {
            max_size: config.max_connection_pool_size,
            min_idle: 1,
            warmup_on_init: false,
            warmup_size: 0,
            max_lifetime: Duration::from_secs(3600),
            idle_timeout: Duration::from_secs(300),
            connection_timeout: config.connection_timeout,
            validation_timeout: Duration::from_secs(5),
        };

        let pool = ConnectionPool::new(config.address.clone(), pool_config);

        Ok(Self {
            config,
            pool: Arc::new(pool),
            open: Arc::new(RwLock::new(true)),
        })
    }

    /// 세션 생성
    pub fn session(&self, config: SessionConfig) -> DriverResult<Session> {
        self.ensure_open()?;
        Session::new(Arc::new(self.config.clone()), self.pool.clone(), config)
    }

    /// 기본 설정으로 세션 생성
    pub fn default_session(&self) -> DriverResult<Session> {
        self.session(SessionConfig::default())
    }

    /// 드라이버 설정
    pub fn config(&self) -> &DriverConfig {
        &self.config
    }

    /// 드라이버 종료
    pub async fn close(&self) -> DriverResult<()> {
        {
            let mut open = self.open.write();
            if !*open {
                return Ok(());
            }
            *open = false;
        } // Guard dropped before await

        self.pool.close().await?;
        Ok(())
    }

    /// 연결 테스트
    pub async fn verify_connectivity(&self) -> DriverResult<()> {
        self.ensure_open()?;
        self.pool.verify_connectivity().await
    }

    /// 서버 정보 조회
    pub fn get_server_info(&self) -> ServerInfo {
        ServerInfo {
            address: self.config.address.clone(),
            agent: self.config.user_agent.clone(),
            protocol_version: "5.0".to_string(),
        }
    }

    /// 열린 상태 확인
    fn ensure_open(&self) -> DriverResult<()> {
        if *self.open.read() {
            Ok(())
        } else {
            Err(DriverError::session("Driver is closed"))
        }
    }

    /// 메트릭 조회
    pub fn metrics(&self) -> DriverMetrics {
        let pool_metrics = self.pool.metrics();
        DriverMetrics {
            pool_size: pool_metrics.size,
            idle_connections: pool_metrics.idle,
            in_use_connections: pool_metrics.in_use,
            total_acquisitions: pool_metrics.total_acquisitions,
            total_connections_created: pool_metrics.total_created,
        }
    }
}

impl fmt::Debug for Driver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Driver")
            .field("address", &self.config.address)
            .field("open", &*self.open.read())
            .finish()
    }
}

// ============================================================================
// ServerInfo - 서버 정보
// ============================================================================

/// 서버 정보
#[derive(Debug, Clone)]
pub struct ServerInfo {
    /// 서버 주소
    pub address: ServerAddress,
    /// 서버 에이전트
    pub agent: String,
    /// 프로토콜 버전
    pub protocol_version: String,
}

impl fmt::Display for ServerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Server @ {} (Agent: {}, Protocol: {})",
            self.address, self.agent, self.protocol_version
        )
    }
}

// ============================================================================
// DriverMetrics - 드라이버 메트릭
// ============================================================================

/// 드라이버 메트릭
#[derive(Debug, Clone, Default)]
pub struct DriverMetrics {
    /// 현재 풀 크기
    pub pool_size: usize,
    /// 유휴 연결 수
    pub idle_connections: usize,
    /// 사용 중인 연결 수
    pub in_use_connections: usize,
    /// 총 연결 획득 횟수
    pub total_acquisitions: u64,
    /// 총 생성된 연결 수
    pub total_connections_created: u64,
}
