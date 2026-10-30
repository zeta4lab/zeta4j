//! Timeout Management
//!
//! Phase 2 Step 1 M1.3: 타임아웃 관리
//!
//! 연결, 요청, 유휴 타임아웃을 관리합니다.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::connection::ConnectionId;

/// 타임아웃 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeoutType {
    /// 연결 설정 타임아웃
    Connect,
    /// 핸드셰이크 타임아웃
    Handshake,
    /// 요청 읽기 타임아웃
    Read,
    /// 응답 쓰기 타임아웃
    Write,
    /// 유휴 타임아웃
    Idle,
    /// 트랜잭션 타임아웃
    Transaction,
    /// 쿼리 실행 타임아웃
    Query,
    /// Keep-Alive 타임아웃
    KeepAlive,
}

impl std::fmt::Display for TimeoutType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect => write!(f, "Connect"),
            Self::Handshake => write!(f, "Handshake"),
            Self::Read => write!(f, "Read"),
            Self::Write => write!(f, "Write"),
            Self::Idle => write!(f, "Idle"),
            Self::Transaction => write!(f, "Transaction"),
            Self::Query => write!(f, "Query"),
            Self::KeepAlive => write!(f, "KeepAlive"),
        }
    }
}

/// 타임아웃 설정
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// 연결 설정 타임아웃
    pub connect_timeout: Duration,
    /// 핸드셰이크 타임아웃
    pub handshake_timeout: Duration,
    /// 요청 읽기 타임아웃
    pub read_timeout: Duration,
    /// 응답 쓰기 타임아웃
    pub write_timeout: Duration,
    /// 유휴 타임아웃
    pub idle_timeout: Duration,
    /// 트랜잭션 타임아웃
    pub transaction_timeout: Duration,
    /// 쿼리 실행 타임아웃
    pub query_timeout: Duration,
    /// Keep-Alive 간격
    pub keepalive_interval: Duration,
    /// Keep-Alive 타임아웃
    pub keepalive_timeout: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(300), // 5분
            transaction_timeout: Duration::from_secs(600), // 10분
            query_timeout: Duration::from_secs(300), // 5분
            keepalive_interval: Duration::from_secs(60),
            keepalive_timeout: Duration::from_secs(15),
        }
    }
}

impl TimeoutConfig {
    /// 빌더 생성
    pub fn builder() -> TimeoutConfigBuilder {
        TimeoutConfigBuilder::default()
    }

    /// 타입에 해당하는 타임아웃 값
    pub fn get(&self, timeout_type: TimeoutType) -> Duration {
        match timeout_type {
            TimeoutType::Connect => self.connect_timeout,
            TimeoutType::Handshake => self.handshake_timeout,
            TimeoutType::Read => self.read_timeout,
            TimeoutType::Write => self.write_timeout,
            TimeoutType::Idle => self.idle_timeout,
            TimeoutType::Transaction => self.transaction_timeout,
            TimeoutType::Query => self.query_timeout,
            TimeoutType::KeepAlive => self.keepalive_timeout,
        }
    }
}

/// 타임아웃 설정 빌더
#[derive(Default)]
pub struct TimeoutConfigBuilder {
    connect_timeout: Option<Duration>,
    handshake_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    transaction_timeout: Option<Duration>,
    query_timeout: Option<Duration>,
    keepalive_interval: Option<Duration>,
    keepalive_timeout: Option<Duration>,
}

impl TimeoutConfigBuilder {
    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = Some(d);
        self
    }

    pub fn handshake_timeout(mut self, d: Duration) -> Self {
        self.handshake_timeout = Some(d);
        self
    }

    pub fn read_timeout(mut self, d: Duration) -> Self {
        self.read_timeout = Some(d);
        self
    }

    pub fn write_timeout(mut self, d: Duration) -> Self {
        self.write_timeout = Some(d);
        self
    }

    pub fn idle_timeout(mut self, d: Duration) -> Self {
        self.idle_timeout = Some(d);
        self
    }

    pub fn transaction_timeout(mut self, d: Duration) -> Self {
        self.transaction_timeout = Some(d);
        self
    }

    pub fn query_timeout(mut self, d: Duration) -> Self {
        self.query_timeout = Some(d);
        self
    }

    pub fn keepalive_interval(mut self, d: Duration) -> Self {
        self.keepalive_interval = Some(d);
        self
    }

    pub fn keepalive_timeout(mut self, d: Duration) -> Self {
        self.keepalive_timeout = Some(d);
        self
    }

    pub fn build(self) -> TimeoutConfig {
        let default = TimeoutConfig::default();
        TimeoutConfig {
            connect_timeout: self.connect_timeout.unwrap_or(default.connect_timeout),
            handshake_timeout: self.handshake_timeout.unwrap_or(default.handshake_timeout),
            read_timeout: self.read_timeout.unwrap_or(default.read_timeout),
            write_timeout: self.write_timeout.unwrap_or(default.write_timeout),
            idle_timeout: self.idle_timeout.unwrap_or(default.idle_timeout),
            transaction_timeout: self
                .transaction_timeout
                .unwrap_or(default.transaction_timeout),
            query_timeout: self.query_timeout.unwrap_or(default.query_timeout),
            keepalive_interval: self
                .keepalive_interval
                .unwrap_or(default.keepalive_interval),
            keepalive_timeout: self.keepalive_timeout.unwrap_or(default.keepalive_timeout),
        }
    }
}

/// 타임아웃 항목
#[derive(Debug, Clone)]
struct TimeoutEntry {
    conn_id: ConnectionId,
    timeout_type: TimeoutType,
    deadline: Instant,
    callback_id: u64,
}

/// 타임아웃 이벤트
#[derive(Debug, Clone)]
pub struct TimeoutEvent {
    pub conn_id: ConnectionId,
    pub timeout_type: TimeoutType,
    pub elapsed: Duration,
}

/// 타임아웃 관리자
///
/// 연결별 타임아웃을 추적하고 만료 시 알림을 보냅니다.
pub struct TimeoutManager {
    config: TimeoutConfig,
    entries: Arc<Mutex<HashMap<(ConnectionId, TimeoutType), TimeoutEntry>>>,
    event_tx: mpsc::Sender<TimeoutEvent>,
    event_rx: Mutex<Option<mpsc::Receiver<TimeoutEvent>>>,
    next_callback_id: std::sync::atomic::AtomicU64,
    running: std::sync::atomic::AtomicBool,
}

impl TimeoutManager {
    /// 새 타임아웃 관리자 생성
    pub fn new(config: TimeoutConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);

        Self {
            config,
            entries: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            event_rx: Mutex::new(Some(event_rx)),
            next_callback_id: std::sync::atomic::AtomicU64::new(1),
            running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(TimeoutConfig::default())
    }

    /// 타임아웃 설정
    pub fn set_timeout(&self, conn_id: ConnectionId, timeout_type: TimeoutType) -> u64 {
        let duration = self.config.get(timeout_type);
        self.set_timeout_duration(conn_id, timeout_type, duration)
    }

    /// 커스텀 타임아웃 설정
    pub fn set_timeout_duration(
        &self,
        conn_id: ConnectionId,
        timeout_type: TimeoutType,
        duration: Duration,
    ) -> u64 {
        let callback_id = self
            .next_callback_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let deadline = Instant::now() + duration;

        let entry = TimeoutEntry {
            conn_id,
            timeout_type,
            deadline,
            callback_id,
        };

        let mut entries = self.entries.lock();
        entries.insert((conn_id, timeout_type), entry);

        callback_id
    }

    /// 타임아웃 취소
    pub fn cancel_timeout(&self, conn_id: ConnectionId, timeout_type: TimeoutType) -> bool {
        let mut entries = self.entries.lock();
        entries.remove(&(conn_id, timeout_type)).is_some()
    }

    /// 연결의 모든 타임아웃 취소
    pub fn cancel_all(&self, conn_id: ConnectionId) {
        let mut entries = self.entries.lock();
        entries.retain(|(id, _), _| *id != conn_id);
    }

    /// 타임아웃 리셋 (기존 타임아웃 갱신)
    pub fn reset_timeout(&self, conn_id: ConnectionId, timeout_type: TimeoutType) -> bool {
        let duration = self.config.get(timeout_type);
        let deadline = Instant::now() + duration;

        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(&(conn_id, timeout_type)) {
            entry.deadline = deadline;
            entry.callback_id = self
                .next_callback_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// 남은 시간 조회
    pub fn remaining(&self, conn_id: ConnectionId, timeout_type: TimeoutType) -> Option<Duration> {
        let entries = self.entries.lock();
        entries.get(&(conn_id, timeout_type)).map(|entry| {
            let now = Instant::now();
            if entry.deadline > now {
                entry.deadline - now
            } else {
                Duration::ZERO
            }
        })
    }

    /// 타임아웃 만료 여부
    pub fn is_expired(&self, conn_id: ConnectionId, timeout_type: TimeoutType) -> bool {
        let entries = self.entries.lock();
        entries
            .get(&(conn_id, timeout_type))
            .map(|entry| Instant::now() >= entry.deadline)
            .unwrap_or(false)
    }

    /// 이벤트 수신기 획득 (한 번만 호출 가능)
    pub fn take_event_receiver(&self) -> Option<mpsc::Receiver<TimeoutEvent>> {
        self.event_rx.lock().take()
    }

    /// 타임아웃 체크 루프 시작
    pub async fn run(&self) {
        if self.running.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return; // 이미 실행 중
        }

        let check_interval = Duration::from_millis(100);

        loop {
            tokio::time::sleep(check_interval).await;

            let now = Instant::now();
            let mut expired = Vec::new();

            {
                let mut entries = self.entries.lock();
                entries.retain(|(conn_id, timeout_type), entry| {
                    if now >= entry.deadline {
                        expired.push(TimeoutEvent {
                            conn_id: *conn_id,
                            timeout_type: *timeout_type,
                            elapsed: now - (entry.deadline - self.config.get(*timeout_type)),
                        });
                        false // 제거
                    } else {
                        true // 유지
                    }
                });
            }

            // 만료 이벤트 발송
            for event in expired {
                if self.event_tx.send(event).await.is_err() {
                    // 수신자가 없음 - 계속 진행
                }
            }
        }
    }

    /// 활성 타임아웃 수
    pub fn active_count(&self) -> usize {
        self.entries.lock().len()
    }

    /// 설정 조회
    pub fn config(&self) -> &TimeoutConfig {
        &self.config
    }
}

/// 타임아웃 가드 (RAII)
///
/// 드롭 시 자동으로 타임아웃 취소
pub struct TimeoutGuard {
    manager: Arc<TimeoutManager>,
    conn_id: ConnectionId,
    timeout_type: TimeoutType,
    active: bool,
}

impl TimeoutGuard {
    /// 새 가드 생성
    pub fn new(
        manager: Arc<TimeoutManager>,
        conn_id: ConnectionId,
        timeout_type: TimeoutType,
    ) -> Self {
        manager.set_timeout(conn_id, timeout_type);
        Self {
            manager,
            conn_id,
            timeout_type,
            active: true,
        }
    }

    /// 타임아웃 리셋
    pub fn reset(&self) -> bool {
        if self.active {
            self.manager.reset_timeout(self.conn_id, self.timeout_type)
        } else {
            false
        }
    }

    /// 남은 시간
    pub fn remaining(&self) -> Option<Duration> {
        if self.active {
            self.manager.remaining(self.conn_id, self.timeout_type)
        } else {
            None
        }
    }

    /// 가드 비활성화 (드롭 시 취소 안 함)
    pub fn disarm(mut self) {
        self.active = false;
    }
}

impl Drop for TimeoutGuard {
    fn drop(&mut self) {
        if self.active {
            self.manager.cancel_timeout(self.conn_id, self.timeout_type);
        }
    }
}
