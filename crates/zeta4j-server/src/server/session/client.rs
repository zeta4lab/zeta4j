//! Client Information and Connection Statistics
//!
//! Phase 2 Step 5 M5.3: 클라이언트 추적
//!
//! 기능:
//! - 클라이언트 ID 및 정보 관리
//! - 드라이버 정보 파싱
//! - 연결 통계 수집

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 클라이언트 ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(Uuid);

impl ClientId {
    /// 새 클라이언트 ID 생성
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// 특정 UUID로 생성
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// 문자열에서 파싱
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }

    /// UUID 조회
    pub fn uuid(&self) -> Uuid {
        self.0
    }

    /// 짧은 형식 (앞 8자리)
    pub fn short(&self) -> String {
        self.0.to_string()[..8].to_string()
    }
}

impl Default for ClientId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ClientId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// 드라이버 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInfo {
    /// 드라이버 이름 (예: "neo4j-java-driver", "neo4j-python-driver")
    pub name: String,
    /// 드라이버 버전
    pub version: String,
    /// 플랫폼 정보
    pub platform: Option<String>,
    /// 언어 버전
    pub language_version: Option<String>,
}

impl DriverInfo {
    /// 새 드라이버 정보 생성
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            platform: None,
            language_version: None,
        }
    }

    /// 유저 에이전트 문자열에서 파싱
    ///
    /// 형식 예시:
    /// - "neo4j-java/4.4.0"
    /// - "neo4j-python/4.4.0 Python/3.9.0-Linux"
    /// - "neo4j-javascript/4.4.0"
    pub fn parse_user_agent(user_agent: &str) -> Option<Self> {
        if user_agent.is_empty() {
            return None;
        }

        let parts: Vec<&str> = user_agent.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        // 첫 번째 부분에서 드라이버 이름과 버전 추출
        let first = parts[0];
        let (name, version) = if first.contains('/') {
            let mut split = first.splitn(2, '/');
            let name = split.next()?;
            let version = split.next().unwrap_or("unknown");
            (name.to_string(), version.to_string())
        } else {
            (first.to_string(), "unknown".to_string())
        };

        // 플랫폼 정보 추출
        let platform = if parts.len() > 1 {
            Some(parts[1..].join(" "))
        } else {
            None
        };

        // 언어 버전 추출
        let language_version = platform.as_ref().and_then(|p| {
            if p.contains('/') {
                let mut split = p.split('/');
                split.next();
                split
                    .next()
                    .map(|v| v.split('-').next().unwrap_or(v).to_string())
            } else {
                None
            }
        });

        Some(Self {
            name,
            version,
            platform,
            language_version,
        })
    }

    /// 유저 에이전트 문자열 생성
    pub fn to_user_agent(&self) -> String {
        let mut ua = format!("{}/{}", self.name, self.version);
        if let Some(ref platform) = self.platform {
            ua.push(' ');
            ua.push_str(platform);
        }
        ua
    }

    /// Neo4j 공식 드라이버인지 확인
    pub fn is_neo4j_driver(&self) -> bool {
        self.name.starts_with("neo4j-")
    }
}

impl std::fmt::Display for DriverInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.name, self.version)
    }
}

/// 클라이언트 정보
#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    /// 클라이언트 ID
    pub id: ClientId,
    /// 원격 주소
    #[serde(skip)]
    pub remote_addr: Option<SocketAddr>,
    /// 유저 에이전트
    pub user_agent: String,
    /// 드라이버 정보
    pub driver: Option<DriverInfo>,
    /// 연결 시간
    #[serde(skip)]
    pub connected_at: Instant,
    /// 인증된 사용자
    pub authenticated_user: Option<String>,
    /// 프로토콜 버전 (예: "4.4", "5.0")
    pub protocol_version: Option<String>,
    /// 원격 IP (문자열)
    pub remote_ip: Option<String>,
    /// 원격 포트
    pub remote_port: Option<u16>,
}

impl ClientInfo {
    /// 새 클라이언트 정보 생성
    pub fn new() -> Self {
        Self {
            id: ClientId::new(),
            remote_addr: None,
            user_agent: String::new(),
            driver: None,
            connected_at: Instant::now(),
            authenticated_user: None,
            protocol_version: None,
            remote_ip: None,
            remote_port: None,
        }
    }

    /// 원격 주소 설정
    pub fn with_remote_addr(mut self, addr: SocketAddr) -> Self {
        self.remote_ip = Some(addr.ip().to_string());
        self.remote_port = Some(addr.port());
        self.remote_addr = Some(addr);
        self
    }

    /// 유저 에이전트 설정
    pub fn with_user_agent(mut self, user_agent: &str) -> Self {
        self.user_agent = user_agent.to_string();
        self.driver = DriverInfo::parse_user_agent(user_agent);
        self
    }

    /// 프로토콜 버전 설정
    pub fn with_protocol_version(mut self, version: &str) -> Self {
        self.protocol_version = Some(version.to_string());
        self
    }

    /// 인증 사용자 설정
    pub fn set_authenticated_user(&mut self, user: &str) {
        self.authenticated_user = Some(user.to_string());
    }

    /// 연결 지속 시간
    pub fn connection_duration(&self) -> Duration {
        self.connected_at.elapsed()
    }

    /// 클라이언트 ID 조회
    pub fn client_id(&self) -> ClientId {
        self.id
    }

    /// 드라이버 이름 조회
    pub fn driver_name(&self) -> Option<&str> {
        self.driver.as_ref().map(|d| d.name.as_str())
    }

    /// 드라이버 버전 조회
    pub fn driver_version(&self) -> Option<&str> {
        self.driver.as_ref().map(|d| d.version.as_str())
    }
}

impl Default for ClientInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// 연결 통계 스냅샷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatsSnapshot {
    /// 총 쿼리 수
    pub total_queries: u64,
    /// 총 트랜잭션 수
    pub total_transactions: u64,
    /// 총 수신 바이트
    pub bytes_received: u64,
    /// 총 전송 바이트
    pub bytes_sent: u64,
    /// 총 에러 수
    pub errors: u64,
    /// 평균 응답 시간 (밀리초)
    pub avg_response_time_ms: f64,
    /// 마지막 쿼리 시간 (Unix timestamp ms)
    pub last_query_at_ms: Option<u64>,
}

/// 연결 통계
///
/// 클라이언트 연결의 실시간 통계를 추적
#[derive(Debug)]
pub struct ConnectionStats {
    /// 총 쿼리 수
    total_queries: AtomicU64,
    /// 총 트랜잭션 수
    total_transactions: AtomicU64,
    /// 총 수신 바이트
    bytes_received: AtomicU64,
    /// 총 전송 바이트
    bytes_sent: AtomicU64,
    /// 총 에러 수
    errors: AtomicU64,
    /// 총 응답 시간 (나노초)
    total_response_time_ns: AtomicU64,
    /// 응답 횟수 (평균 계산용)
    response_count: AtomicU64,
    /// 마지막 쿼리 시간 (Unix timestamp ms)
    last_query_at_ms: AtomicU64,
    /// 생성 시간
    created_at: Instant,
}

impl ConnectionStats {
    /// 새 연결 통계 생성
    pub fn new() -> Self {
        Self {
            total_queries: AtomicU64::new(0),
            total_transactions: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            total_response_time_ns: AtomicU64::new(0),
            response_count: AtomicU64::new(0),
            last_query_at_ms: AtomicU64::new(0),
            created_at: Instant::now(),
        }
    }

    /// 쿼리 완료 기록
    pub fn record_query(&self, duration: Duration, bytes_in: u64, bytes_out: u64) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes_in, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes_out, Ordering::Relaxed);

        let duration_ns = duration.as_nanos() as u64;
        self.total_response_time_ns
            .fetch_add(duration_ns, Ordering::Relaxed);
        self.response_count.fetch_add(1, Ordering::Relaxed);

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.last_query_at_ms.store(now_ms, Ordering::Relaxed);
    }

    /// 트랜잭션 기록
    pub fn record_transaction(&self) {
        self.total_transactions.fetch_add(1, Ordering::Relaxed);
    }

    /// 에러 기록
    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// 바이트 수신 기록
    pub fn record_bytes_received(&self, bytes: u64) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// 바이트 전송 기록
    pub fn record_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    /// 총 쿼리 수
    pub fn total_queries(&self) -> u64 {
        self.total_queries.load(Ordering::Relaxed)
    }

    /// 총 트랜잭션 수
    pub fn total_transactions(&self) -> u64 {
        self.total_transactions.load(Ordering::Relaxed)
    }

    /// 총 수신 바이트
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received.load(Ordering::Relaxed)
    }

    /// 총 전송 바이트
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent.load(Ordering::Relaxed)
    }

    /// 총 에러 수
    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    /// 평균 응답 시간 (Duration)
    pub fn avg_response_time(&self) -> Duration {
        let total_ns = self.total_response_time_ns.load(Ordering::Relaxed);
        let count = self.response_count.load(Ordering::Relaxed);

        Duration::from_nanos(total_ns.checked_div(count).unwrap_or(0))
    }

    /// 연결 지속 시간
    pub fn duration(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// 스냅샷 생성
    pub fn snapshot(&self) -> ConnectionStatsSnapshot {
        let total_ns = self.total_response_time_ns.load(Ordering::Relaxed);
        let count = self.response_count.load(Ordering::Relaxed);
        let avg_ms = if count == 0 {
            0.0
        } else {
            (total_ns as f64 / count as f64) / 1_000_000.0
        };

        let last_query = self.last_query_at_ms.load(Ordering::Relaxed);
        let last_query_at = if last_query > 0 {
            Some(last_query)
        } else {
            None
        };

        ConnectionStatsSnapshot {
            total_queries: self.total_queries.load(Ordering::Relaxed),
            total_transactions: self.total_transactions.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            avg_response_time_ms: avg_ms,
            last_query_at_ms: last_query_at,
        }
    }

    /// 초기화
    pub fn reset(&self) {
        self.total_queries.store(0, Ordering::Relaxed);
        self.total_transactions.store(0, Ordering::Relaxed);
        self.bytes_received.store(0, Ordering::Relaxed);
        self.bytes_sent.store(0, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);
        self.total_response_time_ns.store(0, Ordering::Relaxed);
        self.response_count.store(0, Ordering::Relaxed);
        self.last_query_at_ms.store(0, Ordering::Relaxed);
    }
}

impl Default for ConnectionStats {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ConnectionStats {
    fn clone(&self) -> Self {
        let new = Self::new();
        new.total_queries.store(
            self.total_queries.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.total_transactions.store(
            self.total_transactions.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.bytes_received.store(
            self.bytes_received.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.bytes_sent
            .store(self.bytes_sent.load(Ordering::Relaxed), Ordering::Relaxed);
        new.errors
            .store(self.errors.load(Ordering::Relaxed), Ordering::Relaxed);
        new.total_response_time_ns.store(
            self.total_response_time_ns.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.response_count.store(
            self.response_count.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.last_query_at_ms.store(
            self.last_query_at_ms.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new
    }
}
