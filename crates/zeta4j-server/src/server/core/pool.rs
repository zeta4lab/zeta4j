// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Connection Pool
//!
//! Phase 2 Step 1 M1.3: 연결 풀링
//!
//! 서버 측 연결 풀로 연결 재사용 및 효율적인 리소스 관리를 제공합니다.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// 연결 풀 설정
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// 최소 연결 수 (warm pool)
    pub min_connections: usize,
    /// 최대 연결 수
    pub max_connections: usize,
    /// 연결 획득 대기 타임아웃
    pub acquire_timeout: Duration,
    /// 유휴 연결 타임아웃 (이후 정리)
    pub idle_timeout: Duration,
    /// 연결 최대 수명 (이후 재생성)
    pub max_lifetime: Duration,
    /// 연결 유효성 검사 간격
    pub validation_interval: Duration,
    /// FIFO vs LIFO (기본: LIFO - 최근 사용 연결 우선)
    pub fifo: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 0,
            max_connections: 100,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(600),  // 10분
            max_lifetime: Duration::from_secs(3600), // 1시간
            validation_interval: Duration::from_secs(30),
            fifo: false, // LIFO 기본
        }
    }
}

impl PoolConfig {
    /// 빌더 생성
    pub fn builder() -> PoolConfigBuilder {
        PoolConfigBuilder::default()
    }

    /// 설정 검증
    pub fn validate(&self) -> Result<(), PoolError> {
        if self.max_connections == 0 {
            return Err(PoolError::Config("max_connections must be > 0".into()));
        }
        if self.min_connections > self.max_connections {
            return Err(PoolError::Config(
                "min_connections must be <= max_connections".into(),
            ));
        }
        if self.acquire_timeout.is_zero() {
            return Err(PoolError::Config("acquire_timeout must be > 0".into()));
        }
        Ok(())
    }
}

/// 풀 설정 빌더
#[derive(Default)]
pub struct PoolConfigBuilder {
    min_connections: Option<usize>,
    max_connections: Option<usize>,
    acquire_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    max_lifetime: Option<Duration>,
    validation_interval: Option<Duration>,
    fifo: Option<bool>,
}

impl PoolConfigBuilder {
    pub fn min_connections(mut self, n: usize) -> Self {
        self.min_connections = Some(n);
        self
    }

    pub fn max_connections(mut self, n: usize) -> Self {
        self.max_connections = Some(n);
        self
    }

    pub fn acquire_timeout(mut self, d: Duration) -> Self {
        self.acquire_timeout = Some(d);
        self
    }

    pub fn idle_timeout(mut self, d: Duration) -> Self {
        self.idle_timeout = Some(d);
        self
    }

    pub fn max_lifetime(mut self, d: Duration) -> Self {
        self.max_lifetime = Some(d);
        self
    }

    pub fn validation_interval(mut self, d: Duration) -> Self {
        self.validation_interval = Some(d);
        self
    }

    pub fn fifo(mut self, fifo: bool) -> Self {
        self.fifo = Some(fifo);
        self
    }

    pub fn build(self) -> PoolConfig {
        let default = PoolConfig::default();
        PoolConfig {
            min_connections: self.min_connections.unwrap_or(default.min_connections),
            max_connections: self.max_connections.unwrap_or(default.max_connections),
            acquire_timeout: self.acquire_timeout.unwrap_or(default.acquire_timeout),
            idle_timeout: self.idle_timeout.unwrap_or(default.idle_timeout),
            max_lifetime: self.max_lifetime.unwrap_or(default.max_lifetime),
            validation_interval: self
                .validation_interval
                .unwrap_or(default.validation_interval),
            fifo: self.fifo.unwrap_or(default.fifo),
        }
    }
}

/// 풀 에러
#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    #[error("Pool configuration error: {0}")]
    Config(String),

    #[error("Connection acquisition timeout")]
    AcquireTimeout,

    #[error("Pool is closed")]
    Closed,

    #[error("Connection validation failed: {0}")]
    ValidationFailed(String),

    #[error("Pool exhausted (max connections reached)")]
    Exhausted,

    #[error("Connection creation failed: {0}")]
    CreateFailed(String),
}

/// 풀링 가능한 연결 트레이트
pub trait Poolable: Send + Sync + 'static {
    /// 연결 유효성 검사
    fn is_valid(&self) -> bool;

    /// 연결 리셋 (재사용 전 정리)
    fn reset(&mut self);

    /// 연결 종료
    fn close(&mut self);
}

/// 풀링된 연결 래퍼
pub struct PooledConnection<T: Poolable> {
    /// 내부 연결
    conn: Option<T>,
    /// 풀 참조 (반환용)
    pool: Arc<ConnectionPoolInner<T>>,
    /// 세마포어 퍼밋
    _permit: OwnedSemaphorePermit,
    /// 생성 시간
    created_at: Instant,
    /// 마지막 사용 시간
    last_used: Instant,
}

impl<T: Poolable> PooledConnection<T> {
    /// 내부 연결 참조
    pub fn get(&self) -> &T {
        let Some(conn) = self.conn.as_ref() else {
            std::process::abort();
        };
        conn
    }

    /// 내부 연결 가변 참조
    pub fn get_mut(&mut self) -> &mut T {
        let Some(conn) = self.conn.as_mut() else {
            std::process::abort();
        };
        conn
    }

    /// 연결 수명
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// 유휴 시간
    pub fn idle_time(&self) -> Duration {
        self.last_used.elapsed()
    }

    /// 마지막 사용 시간 갱신
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
    }
}

impl<T: Poolable> Drop for PooledConnection<T> {
    fn drop(&mut self) {
        if let Some(mut conn) = self.conn.take() {
            // 활성 연결 수 감소
            self.pool.active_count.fetch_sub(1, Ordering::Release);

            // 풀이 닫혔거나 연결이 무효하면 그냥 드롭
            if self.pool.closed.load(Ordering::Acquire) || !conn.is_valid() {
                conn.close();
                self.pool.stats.destroyed.fetch_add(1, Ordering::Relaxed);
                return;
            }

            // 연결 최대 수명 체크
            if self.created_at.elapsed() > self.pool.config.max_lifetime {
                conn.close();
                self.pool.stats.destroyed.fetch_add(1, Ordering::Relaxed);
                return;
            }

            // 연결 리셋 후 풀에 반환
            conn.reset();

            let idle_conn = IdleConnection {
                conn,
                created_at: self.created_at,
                returned_at: Instant::now(),
            };

            let mut idle = self.pool.idle.lock();
            if self.pool.config.fifo {
                idle.push_back(idle_conn);
            } else {
                idle.push_front(idle_conn); // LIFO
            }

            self.pool.stats.returned.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl<T: Poolable> std::ops::Deref for PooledConnection<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T: Poolable> std::ops::DerefMut for PooledConnection<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

/// 유휴 연결
struct IdleConnection<T> {
    conn: T,
    created_at: Instant,
    returned_at: Instant,
}

/// 풀 통계
#[derive(Debug, Default)]
pub struct PoolStats {
    /// 총 획득 수
    pub acquired: AtomicU64,
    /// 총 반환 수
    pub returned: AtomicU64,
    /// 총 생성 수
    pub created: AtomicU64,
    /// 총 폐기 수
    pub destroyed: AtomicU64,
    /// 타임아웃 수
    pub timeouts: AtomicU64,
    /// 유효성 검사 실패 수
    pub validation_failures: AtomicU64,
}

impl PoolStats {
    /// 스냅샷 생성
    pub fn snapshot(&self) -> PoolStatsSnapshot {
        PoolStatsSnapshot {
            acquired: self.acquired.load(Ordering::Relaxed),
            returned: self.returned.load(Ordering::Relaxed),
            created: self.created.load(Ordering::Relaxed),
            destroyed: self.destroyed.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
            validation_failures: self.validation_failures.load(Ordering::Relaxed),
        }
    }
}

/// 풀 통계 스냅샷
#[derive(Debug, Clone)]
pub struct PoolStatsSnapshot {
    pub acquired: u64,
    pub returned: u64,
    pub created: u64,
    pub destroyed: u64,
    pub timeouts: u64,
    pub validation_failures: u64,
}

/// 연결 풀 내부
struct ConnectionPoolInner<T: Poolable> {
    config: PoolConfig,
    idle: Mutex<VecDeque<IdleConnection<T>>>,
    semaphore: Arc<Semaphore>,
    stats: PoolStats,
    closed: std::sync::atomic::AtomicBool,
    active_count: AtomicUsize,
}

/// 연결 풀
///
/// 서버 측 연결 풀로, 연결 재사용 및 리소스 관리를 제공합니다.
///
/// ## 기능
/// - 최소/최대 연결 수 설정
/// - 유휴 연결 타임아웃
/// - 연결 최대 수명
/// - FIFO/LIFO 선택
/// - 연결 유효성 검사
pub struct ConnectionPool<T: Poolable> {
    inner: Arc<ConnectionPoolInner<T>>,
}

impl<T: Poolable> ConnectionPool<T> {
    /// 새 연결 풀 생성
    pub fn new(config: PoolConfig) -> Result<Self, PoolError> {
        config.validate()?;

        let semaphore = Arc::new(Semaphore::new(config.max_connections));

        let inner = Arc::new(ConnectionPoolInner {
            config,
            idle: Mutex::new(VecDeque::new()),
            semaphore,
            stats: PoolStats::default(),
            closed: std::sync::atomic::AtomicBool::new(false),
            active_count: AtomicUsize::new(0),
        });

        Ok(Self { inner })
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Result<Self, PoolError> {
        Self::new(PoolConfig::default())
    }

    /// 연결 획득 (기존 유휴 연결 또는 콜백으로 새 연결 생성)
    pub async fn acquire<F, Fut>(&self, create_fn: F) -> Result<PooledConnection<T>, PoolError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(PoolError::Closed);
        }

        // 세마포어로 동시 연결 수 제한
        let permit = match tokio::time::timeout(
            self.inner.config.acquire_timeout,
            Arc::clone(&self.inner.semaphore).acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return Err(PoolError::Closed),
            Err(_) => {
                self.inner.stats.timeouts.fetch_add(1, Ordering::Relaxed);
                return Err(PoolError::AcquireTimeout);
            }
        };

        // 유휴 연결 시도
        let idle_conn = self.try_get_idle();

        let (conn, created_at) = if let Some(idle) = idle_conn {
            // 유휴 연결 재사용
            (idle.conn, idle.created_at)
        } else {
            // 새 연결 생성
            let conn = create_fn().await.map_err(PoolError::CreateFailed)?;
            self.inner.stats.created.fetch_add(1, Ordering::Relaxed);
            (conn, Instant::now())
        };

        self.inner.active_count.fetch_add(1, Ordering::AcqRel);
        self.inner.stats.acquired.fetch_add(1, Ordering::Relaxed);

        Ok(PooledConnection {
            conn: Some(conn),
            pool: Arc::clone(&self.inner),
            _permit: permit,
            created_at,
            last_used: Instant::now(),
        })
    }

    /// 유휴 연결 시도
    fn try_get_idle(&self) -> Option<IdleConnection<T>> {
        let mut idle = self.inner.idle.lock();
        let now = Instant::now();

        // FIFO/LIFO 모두 front에서 꺼냄 (push_front로 넣었으므로)
        while let Some(conn) = idle.pop_front() {
            // 유휴 타임아웃 체크
            if now.duration_since(conn.returned_at) > self.inner.config.idle_timeout {
                self.inner.stats.destroyed.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            // 최대 수명 체크
            if now.duration_since(conn.created_at) > self.inner.config.max_lifetime {
                self.inner.stats.destroyed.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            // 유효성 체크
            if !conn.conn.is_valid() {
                self.inner
                    .stats
                    .validation_failures
                    .fetch_add(1, Ordering::Relaxed);
                self.inner.stats.destroyed.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            return Some(conn);
        }

        None
    }

    /// 현재 유휴 연결 수
    pub fn idle_count(&self) -> usize {
        self.inner.idle.lock().len()
    }

    /// 현재 활성 연결 수
    pub fn active_count(&self) -> usize {
        self.inner.active_count.load(Ordering::Acquire)
    }

    /// 총 연결 수 (활성 + 유휴)
    pub fn total_count(&self) -> usize {
        self.active_count() + self.idle_count()
    }

    /// 최대 연결 수
    pub fn max_connections(&self) -> usize {
        self.inner.config.max_connections
    }

    /// 사용률 (백분율)
    pub fn usage_percent(&self) -> f64 {
        let active = self.active_count() as f64;
        let max = self.inner.config.max_connections as f64;
        (active / max) * 100.0
    }

    /// 풀 상태
    pub fn status(&self) -> PoolStatus {
        PoolStatus {
            active: self.active_count(),
            idle: self.idle_count(),
            max: self.inner.config.max_connections,
            closed: self.inner.closed.load(Ordering::Acquire),
        }
    }

    /// 통계
    pub fn stats(&self) -> PoolStatsSnapshot {
        self.inner.stats.snapshot()
    }

    /// 유휴 연결 정리 (만료된 연결 제거)
    pub fn cleanup_idle(&self) {
        let mut idle = self.inner.idle.lock();
        let now = Instant::now();
        let config = &self.inner.config;

        idle.retain(|conn| {
            let idle_expired = now.duration_since(conn.returned_at) > config.idle_timeout;
            let lifetime_expired = now.duration_since(conn.created_at) > config.max_lifetime;

            if idle_expired || lifetime_expired {
                self.inner.stats.destroyed.fetch_add(1, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
    }

    /// 풀 닫기
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);

        // 모든 유휴 연결 정리
        let mut idle = self.inner.idle.lock();
        for mut conn in idle.drain(..) {
            conn.conn.close();
            self.inner.stats.destroyed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 풀이 닫혔는지 확인
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// 설정 조회
    pub fn config(&self) -> &PoolConfig {
        &self.inner.config
    }
}

impl<T: Poolable> Clone for ConnectionPool<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// 풀 상태
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub active: usize,
    pub idle: usize,
    pub max: usize,
    pub closed: bool,
}

impl std::fmt::Display for PoolStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pool {{ active: {}, idle: {}, max: {}, closed: {} }}",
            self.active, self.idle, self.max, self.closed
        )
    }
}
