//! Session Timeout Management
//!
//! Phase 2 Step 5 M5.4: 타임아웃 관리
//!
//! 기능:
//! - 세션 유휴 타임아웃
//! - 트랜잭션 타임아웃
//! - 쿼리 타임아웃
//! - 타임아웃 이벤트 처리

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tokio::sync::Notify;
use tokio::time::{interval, timeout};

use super::server_session::ServerSessionId;

/// 세션 타임아웃 설정
#[derive(Debug, Clone)]
pub struct SessionTimeoutConfig {
    /// 연결 유휴 타임아웃 (세션 비활성 시 종료)
    pub idle_timeout: Duration,
    /// 세션 최대 수명
    pub max_session_lifetime: Duration,
    /// 기본 트랜잭션 타임아웃
    pub default_tx_timeout: Duration,
    /// 최대 트랜잭션 타임아웃
    pub max_tx_timeout: Duration,
    /// 기본 쿼리 타임아웃
    pub default_query_timeout: Duration,
    /// 최대 쿼리 타임아웃
    pub max_query_timeout: Duration,
    /// 타임아웃 체크 주기
    pub check_interval: Duration,
}

impl Default for SessionTimeoutConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(3600),          // 1시간
            max_session_lifetime: Duration::from_secs(86400), // 24시간
            default_tx_timeout: Duration::from_secs(30),
            max_tx_timeout: Duration::from_secs(300), // 5분
            default_query_timeout: Duration::from_secs(30),
            max_query_timeout: Duration::from_secs(120), // 2분
            check_interval: Duration::from_secs(10),
        }
    }
}

impl SessionTimeoutConfig {
    /// 빌더 생성
    pub fn builder() -> SessionTimeoutConfigBuilder {
        SessionTimeoutConfigBuilder::default()
    }

    /// 설정 검증
    pub fn validate(&self) -> Result<(), String> {
        if self.idle_timeout.is_zero() {
            return Err("idle_timeout must be > 0".into());
        }
        if self.default_tx_timeout > self.max_tx_timeout {
            return Err("default_tx_timeout must be <= max_tx_timeout".into());
        }
        if self.default_query_timeout > self.max_query_timeout {
            return Err("default_query_timeout must be <= max_query_timeout".into());
        }
        Ok(())
    }
}

/// 세션 타임아웃 설정 빌더
#[derive(Default)]
pub struct SessionTimeoutConfigBuilder {
    idle_timeout: Option<Duration>,
    max_session_lifetime: Option<Duration>,
    default_tx_timeout: Option<Duration>,
    max_tx_timeout: Option<Duration>,
    default_query_timeout: Option<Duration>,
    max_query_timeout: Option<Duration>,
    check_interval: Option<Duration>,
}

impl SessionTimeoutConfigBuilder {
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = Some(timeout);
        self
    }

    pub fn max_session_lifetime(mut self, lifetime: Duration) -> Self {
        self.max_session_lifetime = Some(lifetime);
        self
    }

    pub fn default_tx_timeout(mut self, timeout: Duration) -> Self {
        self.default_tx_timeout = Some(timeout);
        self
    }

    pub fn max_tx_timeout(mut self, timeout: Duration) -> Self {
        self.max_tx_timeout = Some(timeout);
        self
    }

    pub fn default_query_timeout(mut self, timeout: Duration) -> Self {
        self.default_query_timeout = Some(timeout);
        self
    }

    pub fn max_query_timeout(mut self, timeout: Duration) -> Self {
        self.max_query_timeout = Some(timeout);
        self
    }

    pub fn check_interval(mut self, interval: Duration) -> Self {
        self.check_interval = Some(interval);
        self
    }

    pub fn build(self) -> SessionTimeoutConfig {
        let default = SessionTimeoutConfig::default();
        SessionTimeoutConfig {
            idle_timeout: self.idle_timeout.unwrap_or(default.idle_timeout),
            max_session_lifetime: self
                .max_session_lifetime
                .unwrap_or(default.max_session_lifetime),
            default_tx_timeout: self
                .default_tx_timeout
                .unwrap_or(default.default_tx_timeout),
            max_tx_timeout: self.max_tx_timeout.unwrap_or(default.max_tx_timeout),
            default_query_timeout: self
                .default_query_timeout
                .unwrap_or(default.default_query_timeout),
            max_query_timeout: self.max_query_timeout.unwrap_or(default.max_query_timeout),
            check_interval: self.check_interval.unwrap_or(default.check_interval),
        }
    }
}

/// 타임아웃 이벤트
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeoutEvent {
    /// 세션 유휴 타임아웃
    SessionIdle {
        session_id: ServerSessionId,
        idle_duration: Duration,
    },
    /// 세션 수명 만료
    SessionExpired {
        session_id: ServerSessionId,
        lifetime: Duration,
    },
    /// 트랜잭션 타임아웃
    TransactionTimeout {
        session_id: ServerSessionId,
        tx_id: u64,
        duration: Duration,
    },
    /// 쿼리 타임아웃
    QueryTimeout {
        session_id: ServerSessionId,
        query_id: i64,
        duration: Duration,
    },
}

impl std::fmt::Display for TimeoutEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeoutEvent::SessionIdle {
                session_id,
                idle_duration,
            } => {
                write!(
                    f,
                    "Session {} idle timeout after {:?}",
                    session_id, idle_duration
                )
            }
            TimeoutEvent::SessionExpired {
                session_id,
                lifetime,
            } => {
                write!(f, "Session {} expired after {:?}", session_id, lifetime)
            }
            TimeoutEvent::TransactionTimeout {
                session_id,
                tx_id,
                duration,
            } => {
                write!(
                    f,
                    "Transaction {} in session {} timeout after {:?}",
                    tx_id, session_id, duration
                )
            }
            TimeoutEvent::QueryTimeout {
                session_id,
                query_id,
                duration,
            } => {
                write!(
                    f,
                    "Query {} in session {} timeout after {:?}",
                    query_id, session_id, duration
                )
            }
        }
    }
}

/// 타임아웃 이벤트 핸들러
pub type TimeoutEventHandler = Arc<dyn Fn(TimeoutEvent) + Send + Sync>;

/// 개별 타이머
#[derive(Debug)]
struct Timer {
    /// 시작 시간
    started_at: Instant,
    /// 타임아웃 시간
    timeout: Duration,
    /// 취소됨
    cancelled: bool,
}

impl Timer {
    fn new(timeout: Duration) -> Self {
        Self {
            started_at: Instant::now(),
            timeout,
            cancelled: false,
        }
    }

    fn is_expired(&self) -> bool {
        !self.cancelled && self.started_at.elapsed() > self.timeout
    }

    fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    fn cancel(&mut self) {
        self.cancelled = true;
    }

    fn reset(&mut self) {
        self.started_at = Instant::now();
        self.cancelled = false;
    }
}

/// 세션 타이머
#[derive(Debug)]
pub struct SessionTimers {
    /// 세션 유휴 타이머
    idle_timer: Timer,
    /// 세션 생성 시간 (수명 체크용)
    created_at: Instant,
    /// 트랜잭션 타이머 (tx_id -> Timer)
    tx_timer: Option<(u64, Timer)>,
    /// 쿼리 타이머 (query_id -> Timer)
    query_timer: Option<(i64, Timer)>,
}

impl SessionTimers {
    fn new(idle_timeout: Duration) -> Self {
        Self {
            idle_timer: Timer::new(idle_timeout),
            created_at: Instant::now(),
            tx_timer: None,
            query_timer: None,
        }
    }

    /// 세션 수명
    fn lifetime(&self) -> Duration {
        self.created_at.elapsed()
    }
}

/// 세션 타임아웃 관리자
///
/// 모든 세션의 타임아웃을 중앙에서 관리
pub struct SessionTimeoutManager {
    config: SessionTimeoutConfig,
    timers: Arc<RwLock<HashMap<ServerSessionId, SessionTimers>>>,
    handlers: Arc<RwLock<Vec<TimeoutEventHandler>>>,
    running: AtomicBool,
    shutdown_notify: Arc<Notify>,
    events_processed: AtomicU64,
    timeouts_triggered: AtomicU64,
}

impl SessionTimeoutManager {
    /// 새 타임아웃 관리자 생성
    pub fn new(config: SessionTimeoutConfig) -> Self {
        Self {
            config,
            timers: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(Vec::new())),
            running: AtomicBool::new(false),
            shutdown_notify: Arc::new(Notify::new()),
            events_processed: AtomicU64::new(0),
            timeouts_triggered: AtomicU64::new(0),
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(SessionTimeoutConfig::default())
    }

    /// 세션 등록
    pub fn register_session(&self, session_id: ServerSessionId) {
        let timers = SessionTimers::new(self.config.idle_timeout);
        self.timers.write().insert(session_id, timers);
    }

    /// 세션 해제
    pub fn unregister_session(&self, session_id: ServerSessionId) {
        self.timers.write().remove(&session_id);
    }

    /// 유휴 타이머 리셋 (세션 활동 시 호출)
    pub fn reset_idle_timer(&self, session_id: ServerSessionId) {
        if let Some(timers) = self.timers.write().get_mut(&session_id) {
            timers.idle_timer.reset();
        }
    }

    /// 트랜잭션 타이머 시작
    pub fn start_tx_timer(
        &self,
        session_id: ServerSessionId,
        tx_id: u64,
        timeout: Option<Duration>,
    ) {
        let actual_timeout = timeout
            .map(|t| t.min(self.config.max_tx_timeout))
            .unwrap_or(self.config.default_tx_timeout);

        if let Some(timers) = self.timers.write().get_mut(&session_id) {
            timers.tx_timer = Some((tx_id, Timer::new(actual_timeout)));
        }
    }

    /// 트랜잭션 타이머 취소
    pub fn cancel_tx_timer(&self, session_id: ServerSessionId) {
        if let Some(timers) = self.timers.write().get_mut(&session_id) {
            if let Some((_, ref mut timer)) = timers.tx_timer {
                timer.cancel();
            }
            timers.tx_timer = None;
        }
    }

    /// 쿼리 타이머 시작
    pub fn start_query_timer(
        &self,
        session_id: ServerSessionId,
        query_id: i64,
        timeout: Option<Duration>,
    ) {
        let actual_timeout = timeout
            .map(|t| t.min(self.config.max_query_timeout))
            .unwrap_or(self.config.default_query_timeout);

        if let Some(timers) = self.timers.write().get_mut(&session_id) {
            timers.query_timer = Some((query_id, Timer::new(actual_timeout)));
        }
    }

    /// 쿼리 타이머 취소
    pub fn cancel_query_timer(&self, session_id: ServerSessionId) {
        if let Some(timers) = self.timers.write().get_mut(&session_id) {
            if let Some((_, ref mut timer)) = timers.query_timer {
                timer.cancel();
            }
            timers.query_timer = None;
        }
    }

    /// 타임아웃 이벤트 핸들러 등록
    pub fn on_timeout(&self, handler: TimeoutEventHandler) {
        self.handlers.write().push(handler);
    }

    /// 이벤트 발송
    fn emit_event(&self, event: TimeoutEvent) {
        self.events_processed.fetch_add(1, Ordering::Relaxed);
        self.timeouts_triggered.fetch_add(1, Ordering::Relaxed);

        let handlers = self.handlers.read();
        for handler in handlers.iter() {
            handler(event.clone());
        }
    }

    /// 타임아웃 체크 (단일 실행)
    pub fn check_timeouts(&self) -> Vec<TimeoutEvent> {
        let mut events = Vec::new();
        let mut expired_sessions = Vec::new();

        {
            let timers = self.timers.read();

            for (session_id, session_timers) in timers.iter() {
                // 세션 수명 체크
                if session_timers.lifetime() > self.config.max_session_lifetime {
                    events.push(TimeoutEvent::SessionExpired {
                        session_id: *session_id,
                        lifetime: session_timers.lifetime(),
                    });
                    expired_sessions.push(*session_id);
                    continue;
                }

                // 유휴 타임아웃 체크
                if session_timers.idle_timer.is_expired() {
                    events.push(TimeoutEvent::SessionIdle {
                        session_id: *session_id,
                        idle_duration: session_timers.idle_timer.elapsed(),
                    });
                    expired_sessions.push(*session_id);
                    continue;
                }

                // 트랜잭션 타임아웃 체크
                if let Some((tx_id, ref timer)) = session_timers.tx_timer
                    && timer.is_expired()
                {
                    events.push(TimeoutEvent::TransactionTimeout {
                        session_id: *session_id,
                        tx_id,
                        duration: timer.elapsed(),
                    });
                }

                // 쿼리 타임아웃 체크
                if let Some((query_id, ref timer)) = session_timers.query_timer
                    && timer.is_expired()
                {
                    events.push(TimeoutEvent::QueryTimeout {
                        session_id: *session_id,
                        query_id,
                        duration: timer.elapsed(),
                    });
                }
            }
        }

        // 만료된 세션 제거
        {
            let mut timers = self.timers.write();
            for session_id in expired_sessions {
                timers.remove(&session_id);
            }
        }

        // 이벤트 발송
        for event in &events {
            self.emit_event(event.clone());
        }

        events
    }

    /// 백그라운드 체커 실행
    pub async fn run(&self) {
        self.running.store(true, Ordering::Release);

        let mut ticker = interval(self.config.check_interval);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if !self.running.load(Ordering::Acquire) {
                        break;
                    }
                    self.check_timeouts();
                }
                _ = self.shutdown_notify.notified() => {
                    break;
                }
            }
        }

        self.running.store(false, Ordering::Release);
    }

    /// 종료
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Release);
        self.shutdown_notify.notify_waiters();
    }

    /// 실행 중인지 확인
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// 등록된 세션 수
    pub fn session_count(&self) -> usize {
        self.timers.read().len()
    }

    /// 처리된 이벤트 수
    pub fn events_processed(&self) -> u64 {
        self.events_processed.load(Ordering::Relaxed)
    }

    /// 트리거된 타임아웃 수
    pub fn timeouts_triggered(&self) -> u64 {
        self.timeouts_triggered.load(Ordering::Relaxed)
    }

    /// 설정 조회
    pub fn config(&self) -> &SessionTimeoutConfig {
        &self.config
    }
}

impl std::fmt::Debug for SessionTimeoutManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionTimeoutManager")
            .field("config", &self.config)
            .field("session_count", &self.session_count())
            .field("running", &self.running.load(Ordering::Relaxed))
            .field(
                "events_processed",
                &self.events_processed.load(Ordering::Relaxed),
            )
            .field(
                "timeouts_triggered",
                &self.timeouts_triggered.load(Ordering::Relaxed),
            )
            .finish()
    }
}

/// 작업 타임아웃 래퍼
///
/// Future에 타임아웃을 적용하는 유틸리티
pub async fn with_timeout<F, T>(duration: Duration, fut: F) -> Result<T, TimeoutError>
where
    F: std::future::Future<Output = T>,
{
    timeout(duration, fut)
        .await
        .map_err(|_| TimeoutError::Elapsed(duration))
}

/// 타임아웃 에러
#[derive(Debug, thiserror::Error)]
pub enum TimeoutError {
    #[error("Operation timed out after {0:?}")]
    Elapsed(Duration),
}
