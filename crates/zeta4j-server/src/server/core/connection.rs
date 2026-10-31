// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Connection Management

use dashmap::DashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::broadcast;

use crate::server::common::config::ServerConfig;
use crate::server::common::error::{ConnectionError, TrackerError};

/// 연결 ID (글로벌 유니크)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(u64);

impl ConnectionId {
    /// 새 연결 ID 생성
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// u64 값으로 변환
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "conn-{:016x}", self.0)
    }
}

/// 연결 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// 새 연결 (accept 직후)
    New,
    /// 핸드셰이크 진행 중
    Handshaking,
    /// 준비 완료 (요청 대기)
    Ready,
    /// 요청 처리 중
    Busy,
    /// 종료 중
    Closing,
    /// 종료됨
    Closed,
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::New => write!(f, "New"),
            Self::Handshaking => write!(f, "Handshaking"),
            Self::Ready => write!(f, "Ready"),
            Self::Busy => write!(f, "Busy"),
            Self::Closing => write!(f, "Closing"),
            Self::Closed => write!(f, "Closed"),
        }
    }
}

/// 연결 정보
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: ConnectionId,
    pub peer_addr: SocketAddr,
    pub local_addr: SocketAddr,
    pub state: ConnectionState,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub requests_processed: u64,
    pub client_agent: Option<String>,
    pub authenticated_user: Option<String>,
}

impl ConnectionInfo {
    /// 연결 수명
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// 유휴 시간
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }
}

/// 연결 통계
#[derive(Default)]
struct ConnectionStats {
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    requests_processed: AtomicU64,
}

/// TCP 연결
pub struct Connection {
    id: ConnectionId,
    stream: TcpStream,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    state: ConnectionState,
    created_at: Instant,
    last_activity: Instant,
    stats: ConnectionStats,
    client_agent: Option<String>,
    authenticated_user: Option<String>,
}

impl Connection {
    /// 새 연결 생성
    pub fn new(id: ConnectionId, stream: TcpStream, peer_addr: SocketAddr) -> Self {
        let local_addr = stream
            .local_addr()
            .unwrap_or_else(|_| ([0, 0, 0, 0], 0).into());
        let now = Instant::now();

        Self {
            id,
            stream,
            peer_addr,
            local_addr,
            state: ConnectionState::New,
            created_at: now,
            last_activity: now,
            stats: ConnectionStats::default(),
            client_agent: None,
            authenticated_user: None,
        }
    }

    /// 연결 ID
    pub fn id(&self) -> ConnectionId {
        self.id
    }

    /// 피어 주소
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    /// 로컬 주소
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// 현재 상태
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// 상태 설정
    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    /// 연결 수명
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// 유휴 시간
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// 활동 시간 갱신
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// 스트림 참조
    pub fn stream(&self) -> &TcpStream {
        &self.stream
    }

    /// 스트림 가변 참조
    pub fn stream_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }

    /// 읽은 바이트 수 기록
    pub fn record_bytes_read(&self, bytes: u64) {
        self.stats.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }

    /// 쓴 바이트 수 기록
    pub fn record_bytes_written(&self, bytes: u64) {
        self.stats.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    /// 처리된 요청 수 증가
    pub fn record_request_processed(&self) {
        self.stats
            .requests_processed
            .fetch_add(1, Ordering::Relaxed);
    }

    /// 클라이언트 에이전트 설정
    pub fn set_client_agent(&mut self, agent: String) {
        self.client_agent = Some(agent);
    }

    /// 인증된 사용자 설정
    pub fn set_authenticated_user(&mut self, user: String) {
        self.authenticated_user = Some(user);
    }

    /// 연결 정보 생성
    pub fn info(&self) -> ConnectionInfo {
        ConnectionInfo {
            id: self.id,
            peer_addr: self.peer_addr,
            local_addr: self.local_addr,
            state: self.state,
            created_at: self.created_at,
            last_activity: self.last_activity,
            bytes_read: self.stats.bytes_read.load(Ordering::Relaxed),
            bytes_written: self.stats.bytes_written.load(Ordering::Relaxed),
            requests_processed: self.stats.requests_processed.load(Ordering::Relaxed),
            client_agent: self.client_agent.clone(),
            authenticated_user: self.authenticated_user.clone(),
        }
    }

    /// 연결 실행 (메인 루프)
    pub async fn run(
        mut self,
        config: Arc<ServerConfig>,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), ConnectionError> {
        // 핸드셰이크
        self.state = ConnectionState::Handshaking;

        let handshake_result =
            tokio::time::timeout(config.handshake_timeout, self.perform_handshake()).await;

        match handshake_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(ConnectionError::Timeout("handshake")),
        }

        self.state = ConnectionState::Ready;

        // 메인 요청 처리 루프
        // Note: 현재는 shutdown/idle timeout만 처리하는 stub 구현
        #[allow(clippy::never_loop)]
        loop {
            self.touch();

            tokio::select! {
                biased;

                // Shutdown 시그널
                _ = shutdown_rx.recv() => {
                    tracing::debug!("Connection {} received shutdown signal", self.id);
                    break;
                }

                // 유휴 타임아웃
                _ = tokio::time::sleep(config.idle_timeout) => {
                    tracing::debug!("Connection {} idle timeout", self.id);
                    break;
                }
            }
        }

        // 정리
        self.state = ConnectionState::Closing;
        self.perform_close().await?;
        self.state = ConnectionState::Closed;

        Ok(())
    }

    /// 핸드셰이크 수행
    async fn perform_handshake(&mut self) -> Result<(), ConnectionError> {
        // 기본 TCP 연결에서는 특별한 핸드셰이크 없음
        // Bolt/HTTP 프로토콜 구현 시 오버라이드
        Ok(())
    }

    /// 연결 종료
    async fn perform_close(&mut self) -> Result<(), ConnectionError> {
        use tokio::io::AsyncWriteExt;
        self.stream.shutdown().await.ok();
        Ok(())
    }
}

/// 연결 추적기
pub struct ConnectionTracker {
    connections: DashMap<ConnectionId, ConnectionInfo>,
    by_ip: DashMap<IpAddr, AtomicUsize>,
    max_connections: usize,
    active_count: AtomicUsize,
    drain_notify: tokio::sync::Notify,
}

impl ConnectionTracker {
    /// 새 추적기 생성
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: DashMap::new(),
            by_ip: DashMap::new(),
            max_connections,
            active_count: AtomicUsize::new(0),
            drain_notify: tokio::sync::Notify::new(),
        }
    }

    /// 연결 추가 시도
    pub fn try_add(&self, peer_addr: SocketAddr) -> Result<ConnectionId, TrackerError> {
        // 최대 연결 수 체크
        loop {
            let current = self.active_count.load(Ordering::Acquire);
            if current >= self.max_connections {
                return Err(TrackerError::MaxConnectionsReached);
            }

            // CAS로 atomic하게 증가
            if self
                .active_count
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
        }

        let conn_id = ConnectionId::new();
        let now = Instant::now();

        self.connections.insert(
            conn_id,
            ConnectionInfo {
                id: conn_id,
                peer_addr,
                local_addr: ([0, 0, 0, 0], 0).into(),
                state: ConnectionState::New,
                created_at: now,
                last_activity: now,
                bytes_read: 0,
                bytes_written: 0,
                requests_processed: 0,
                client_agent: None,
                authenticated_user: None,
            },
        );

        // IP별 카운트 증가
        self.by_ip
            .entry(peer_addr.ip())
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);

        Ok(conn_id)
    }

    /// 연결 제거
    pub fn remove(&self, conn_id: ConnectionId) -> bool {
        if let Some((_, info)) = self.connections.remove(&conn_id) {
            self.active_count.fetch_sub(1, Ordering::AcqRel);

            // IP별 카운트 감소 (데드락 방지: get 참조를 먼저 해제)
            let should_remove_ip = {
                if let Some(count) = self.by_ip.get(&info.peer_addr.ip()) {
                    count.fetch_sub(1, Ordering::Relaxed) == 1
                } else {
                    false
                }
            }; // Ref가 여기서 drop됨

            if should_remove_ip {
                self.by_ip.remove(&info.peer_addr.ip());
            }

            // 드레인 완료 체크
            if self.active_count.load(Ordering::Acquire) == 0 {
                self.drain_notify.notify_waiters();
            }

            true
        } else {
            false
        }
    }

    /// 연결 정보 업데이트
    pub fn update<F>(&self, conn_id: ConnectionId, f: F) -> bool
    where
        F: FnOnce(&mut ConnectionInfo),
    {
        if let Some(mut entry) = self.connections.get_mut(&conn_id) {
            f(entry.value_mut());
            true
        } else {
            false
        }
    }

    /// 현재 연결 수
    pub fn count(&self) -> usize {
        self.active_count.load(Ordering::Acquire)
    }

    /// IP별 연결 수
    pub fn count_by_ip(&self, ip: &IpAddr) -> usize {
        self.by_ip
            .get(ip)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// IP별 연결 제한 체크
    pub fn check_per_ip_limit(&self, ip: &IpAddr, limit: usize) -> bool {
        if limit == 0 {
            return true; // 무제한
        }
        self.count_by_ip(ip) < limit
    }

    /// 드레인 완료 대기
    pub async fn wait_for_drain(&self) {
        while self.count() > 0 {
            self.drain_notify.notified().await;
        }
    }

    /// 모든 연결 강제 종료
    pub async fn force_close_all(&self) {
        self.connections.clear();
        self.by_ip.clear();
        self.active_count.store(0, Ordering::Release);
        self.drain_notify.notify_waiters();
    }

    /// 연결 목록 조회
    pub fn list(&self) -> Vec<ConnectionInfo> {
        self.connections.iter().map(|r| r.value().clone()).collect()
    }

    /// 특정 연결 조회
    pub fn get(&self, conn_id: ConnectionId) -> Option<ConnectionInfo> {
        self.connections.get(&conn_id).map(|r| r.value().clone())
    }

    /// 최대 연결 수
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// 사용률 (백분율)
    pub fn usage_percent(&self) -> f64 {
        let count = self.count() as f64;
        let max = self.max_connections as f64;
        (count / max) * 100.0
    }
}
