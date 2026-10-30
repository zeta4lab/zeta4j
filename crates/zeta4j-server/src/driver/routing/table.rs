//! 라우팅 테이블
//!
//! 클러스터의 서버 역할별 목록을 관리합니다.

use std::time::Instant;

use super::super::driver::ServerAddress;

/// 서버 역할
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerRole {
    /// 라우팅 테이블 제공자
    Route,
    /// 쓰기 트랜잭션 처리 (리더)
    Write,
    /// 읽기 트랜잭션 처리 (팔로워)
    Read,
}

impl ServerRole {
    /// 문자열에서 역할 파싱
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "ROUTE" => Some(Self::Route),
            "WRITE" => Some(Self::Write),
            "READ" => Some(Self::Read),
            _ => None,
        }
    }

    /// 역할을 문자열로 변환
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Route => "ROUTE",
            Self::Write => "WRITE",
            Self::Read => "READ",
        }
    }
}

/// 라우팅 테이블
#[derive(Debug, Clone)]
pub struct RoutingTable {
    /// 라우터 목록 (라우팅 테이블 조회용)
    pub routers: Vec<ServerAddress>,
    /// 라이터 목록 (쓰기 트랜잭션용)
    pub writers: Vec<ServerAddress>,
    /// 리더 목록 (읽기 트랜잭션용)
    pub readers: Vec<ServerAddress>,
    /// 데이터베이스 이름
    pub database: String,
    /// TTL (초)
    pub ttl_seconds: u64,
    /// 생성/갱신 시간
    pub updated_at: Instant,
}

impl RoutingTable {
    /// 새 라우팅 테이블 생성
    pub fn new(database: impl Into<String>) -> Self {
        Self {
            routers: Vec::new(),
            writers: Vec::new(),
            readers: Vec::new(),
            database: database.into(),
            ttl_seconds: 300, // 기본 5분
            updated_at: Instant::now(),
        }
    }

    /// 초기 라우터로 테이블 생성
    pub fn with_initial_routers(database: impl Into<String>, routers: Vec<ServerAddress>) -> Self {
        Self {
            routers: routers.clone(),
            writers: Vec::new(),
            readers: routers,
            database: database.into(),
            ttl_seconds: 0, // 즉시 갱신 필요
            updated_at: Instant::now(),
        }
    }

    /// 라우터 추가
    pub fn add_router(&mut self, address: ServerAddress) {
        if !self.routers.contains(&address) {
            self.routers.push(address);
        }
    }

    /// 라이터 추가
    pub fn add_writer(&mut self, address: ServerAddress) {
        if !self.writers.contains(&address) {
            self.writers.push(address);
        }
    }

    /// 리더 추가
    pub fn add_reader(&mut self, address: ServerAddress) {
        if !self.readers.contains(&address) {
            self.readers.push(address);
        }
    }

    /// 역할별 서버 추가
    pub fn add_server(&mut self, role: ServerRole, address: ServerAddress) {
        match role {
            ServerRole::Route => self.add_router(address),
            ServerRole::Write => self.add_writer(address),
            ServerRole::Read => self.add_reader(address),
        }
    }

    /// 라우팅 테이블이 만료되었는지 확인
    pub fn is_expired(&self) -> bool {
        self.updated_at.elapsed().as_secs() >= self.ttl_seconds
    }

    /// 갱신이 필요한지 확인 (TTL의 80% 경과)
    pub fn needs_refresh(&self) -> bool {
        let threshold = (self.ttl_seconds as f64 * 0.8) as u64;
        self.updated_at.elapsed().as_secs() >= threshold
    }

    /// 테이블 갱신 시간 업데이트
    pub fn mark_updated(&mut self) {
        self.updated_at = Instant::now();
    }

    /// TTL 설정
    pub fn set_ttl(&mut self, seconds: u64) {
        self.ttl_seconds = seconds;
    }

    /// 서버가 있는지 확인
    pub fn has_servers(&self) -> bool {
        !self.writers.is_empty() || !self.readers.is_empty()
    }

    /// 쓰기 가능한 서버가 있는지 확인
    pub fn has_writers(&self) -> bool {
        !self.writers.is_empty()
    }

    /// 읽기 가능한 서버가 있는지 확인
    pub fn has_readers(&self) -> bool {
        !self.readers.is_empty()
    }

    /// 라우터가 있는지 확인
    pub fn has_routers(&self) -> bool {
        !self.routers.is_empty()
    }

    /// 서버 제거
    pub fn remove_server(&mut self, address: &ServerAddress) {
        self.routers.retain(|a| a != address);
        self.writers.retain(|a| a != address);
        self.readers.retain(|a| a != address);
    }

    /// 테이블 초기화
    pub fn clear(&mut self) {
        self.routers.clear();
        self.writers.clear();
        self.readers.clear();
    }
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self::new("zeta4j")
    }
}
