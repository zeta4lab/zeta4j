// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! M9.2: Connection Pool
//!
//! 연결 풀링

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::Semaphore;

use super::bolt::{BoltClient, to_bolt_auth};
use super::driver::{AuthToken, ServerAddress};
use super::error::{DriverError, DriverResult};

// ============================================================================
// PoolConfig - 풀 설정
// ============================================================================

/// 연결 풀 설정
///
/// 연결 풀의 동작을 제어하는 설정입니다.
///
/// # 필드
///
/// | 필드 | 기본값 | 설명 |
/// |------|--------|------|
/// | `max_size` | 100 | 최대 연결 수 |
/// | `min_idle` | 1 | 최소 유휴 연결 수 |
/// | `warmup_on_init` | false | 초기화 시 워밍업 여부 |
/// | `warmup_size` | 0 | 워밍업 연결 수 (0이면 min_idle 사용) |
/// | `max_lifetime` | 1시간 | 연결 최대 수명 |
/// | `idle_timeout` | 5분 | 유휴 타임아웃 |
/// | `connection_timeout` | 30초 | 연결 타임아웃 |
/// | `validation_timeout` | 5초 | 검증 타임아웃 |
///
/// # 예시
///
/// ```rust,ignore
/// use zeta4j_server::driver::PoolConfig;
/// use std::time::Duration;
///
/// let config = PoolConfig {
///     max_size: 50,
///     min_idle: 5,
///     warmup_on_init: true,
///     warmup_size: 10, // 시작 시 10개 연결 미리 생성
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// 최대 연결 수
    pub max_size: usize,
    /// 최소 유휴 연결 수
    pub min_idle: usize,
    /// 초기화 시 워밍업 수행 여부
    pub warmup_on_init: bool,
    /// 워밍업 시 생성할 연결 수 (0이면 min_idle 사용)
    pub warmup_size: usize,
    /// 연결 최대 수명
    pub max_lifetime: Duration,
    /// 유휴 타임아웃
    pub idle_timeout: Duration,
    /// 연결 타임아웃
    pub connection_timeout: Duration,
    /// 검증 타임아웃
    pub validation_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 100,
            min_idle: 1,
            warmup_on_init: false,
            warmup_size: 0,
            max_lifetime: Duration::from_secs(3600),
            idle_timeout: Duration::from_secs(300),
            connection_timeout: Duration::from_secs(30),
            validation_timeout: Duration::from_secs(5),
        }
    }
}

impl PoolConfig {
    /// 빌더 패턴으로 풀 설정 생성
    pub fn builder() -> PoolConfigBuilder {
        PoolConfigBuilder::default()
    }
}

/// 풀 설정 빌더
#[derive(Debug, Clone, Default)]
pub struct PoolConfigBuilder {
    config: PoolConfig,
}

impl PoolConfigBuilder {
    /// 최대 연결 수 설정
    pub fn max_size(mut self, size: usize) -> Self {
        self.config.max_size = size;
        self
    }

    /// 최소 유휴 연결 수 설정
    pub fn min_idle(mut self, size: usize) -> Self {
        self.config.min_idle = size;
        self
    }

    /// 워밍업 활성화 (min_idle 개수만큼)
    pub fn with_warmup(mut self) -> Self {
        self.config.warmup_on_init = true;
        self
    }

    /// 워밍업 활성화 (지정 개수)
    pub fn with_warmup_size(mut self, size: usize) -> Self {
        self.config.warmup_on_init = true;
        self.config.warmup_size = size;
        self
    }

    /// 연결 최대 수명 설정
    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.config.max_lifetime = duration;
        self
    }

    /// 유휴 타임아웃 설정
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.config.idle_timeout = duration;
        self
    }

    /// 연결 타임아웃 설정
    pub fn connection_timeout(mut self, duration: Duration) -> Self {
        self.config.connection_timeout = duration;
        self
    }

    /// 검증 타임아웃 설정
    pub fn validation_timeout(mut self, duration: Duration) -> Self {
        self.config.validation_timeout = duration;
        self
    }

    /// 설정 빌드
    pub fn build(self) -> PoolConfig {
        self.config
    }
}

// ============================================================================
// ConnectionState - 연결 상태
// ============================================================================

/// 연결 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// 연결됨
    Connected,
    /// 유휴 상태
    Idle,
    /// 사용 중
    InUse,
    /// 닫힘
    Closed,
    /// 오류
    Failed,
}

// ============================================================================
// PooledConnection - 풀링된 연결
// ============================================================================

/// 풀링된 연결
pub struct PooledConnection {
    /// 연결 ID
    id: u64,
    /// 서버 주소
    address: ServerAddress,
    /// 생성 시간
    created_at: Instant,
    /// 마지막 사용 시간
    last_used: Instant,
    /// 상태
    state: ConnectionState,
    /// 연결 풀 참조
    pool: Option<Arc<ConnectionPool>>,
    /// Bolt 클라이언트 (실제 연결)
    pub(crate) bolt_client: Option<BoltClient>,
}

impl PooledConnection {
    /// 새 연결 생성
    #[expect(dead_code)]
    pub(crate) fn new(id: u64, address: ServerAddress) -> Self {
        let now = Instant::now();
        Self {
            id,
            address,
            created_at: now,
            last_used: now,
            state: ConnectionState::Connected,
            pool: None,
            bolt_client: None,
        }
    }

    /// Bolt 클라이언트와 함께 연결 생성
    pub(crate) fn with_bolt_client(id: u64, address: ServerAddress, client: BoltClient) -> Self {
        let now = Instant::now();
        Self {
            id,
            address,
            created_at: now,
            last_used: now,
            state: ConnectionState::Connected,
            pool: None,
            bolt_client: Some(client),
        }
    }

    /// Bolt 클라이언트 참조 (가변)
    pub fn bolt_client_mut(&mut self) -> Option<&mut BoltClient> {
        self.bolt_client.as_mut()
    }

    /// Bolt 클라이언트 참조
    pub fn bolt_client(&self) -> Option<&BoltClient> {
        self.bolt_client.as_ref()
    }

    /// 연결 ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// 서버 주소
    pub fn address(&self) -> &ServerAddress {
        &self.address
    }

    /// 생성 시간
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// 마지막 사용 시간
    pub fn last_used(&self) -> Instant {
        self.last_used
    }

    /// 연결 상태
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// 유효성 확인
    pub fn is_valid(&self, config: &PoolConfig) -> bool {
        if self.state == ConnectionState::Closed || self.state == ConnectionState::Failed {
            return false;
        }

        // 최대 수명 체크
        if self.created_at.elapsed() > config.max_lifetime {
            return false;
        }

        // 유휴 타임아웃 체크
        if self.state == ConnectionState::Idle && self.last_used.elapsed() > config.idle_timeout {
            return false;
        }

        true
    }

    /// 사용으로 표시
    pub fn mark_in_use(&mut self) {
        self.state = ConnectionState::InUse;
        self.last_used = Instant::now();
    }

    /// 유휴로 표시
    pub fn mark_idle(&mut self) {
        self.state = ConnectionState::Idle;
        self.last_used = Instant::now();
    }

    /// 닫힘으로 표시
    pub fn mark_closed(&mut self) {
        self.state = ConnectionState::Closed;
    }

    /// 실패로 표시
    pub fn mark_failed(&mut self) {
        self.state = ConnectionState::Failed;
    }

    /// 연결 닫기
    pub async fn close(&mut self) -> DriverResult<()> {
        self.mark_closed();
        // BoltClient 닫기
        if let Some(ref mut client) = self.bolt_client {
            let _ = client.close().await;
        }
        self.bolt_client = None;
        Ok(())
    }

    /// 풀 설정
    pub fn set_pool(&mut self, pool: Arc<ConnectionPool>) {
        self.pool = Some(pool);
    }

    /// 풀로 반환
    pub fn return_to_pool(mut self) {
        if let Some(pool) = self.pool.take() {
            self.mark_idle();
            pool.return_connection(self);
        }
    }
}

impl std::fmt::Debug for PooledConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledConnection")
            .field("id", &self.id)
            .field("address", &self.address)
            .field("state", &self.state)
            .field("age", &self.created_at.elapsed())
            .finish()
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        // 풀로 반환되지 않은 연결은 닫음
        if self.pool.is_some() && self.state != ConnectionState::Closed {
            // 비동기 닫기가 불가능하므로 상태만 변경
            self.mark_closed();
        }
    }
}

// ============================================================================
// PoolMetrics - 풀 메트릭
// ============================================================================

/// 풀 메트릭
#[derive(Debug, Clone, Default)]
pub struct PoolMetrics {
    /// 현재 크기
    pub size: usize,
    /// 유휴 연결 수
    pub idle: usize,
    /// 사용 중인 연결 수
    pub in_use: usize,
    /// 총 획득 횟수
    pub total_acquisitions: u64,
    /// 총 생성 횟수
    pub total_created: u64,
    /// 총 닫힌 연결 수
    pub total_closed: u64,
    /// 총 타임아웃 횟수
    pub total_timeouts: u64,
}

// ============================================================================
// ConnectionPool - 연결 풀
// ============================================================================

/// 연결 풀
pub struct ConnectionPool {
    /// 서버 주소
    address: ServerAddress,
    /// 풀 설정
    config: PoolConfig,
    /// 인증 토큰
    auth: AuthToken,
    /// 유휴 연결들
    idle_connections: Mutex<VecDeque<PooledConnection>>,
    /// 세마포어 (연결 수 제한)
    semaphore: Arc<Semaphore>,
    /// 현재 크기
    size: AtomicUsize,
    /// 사용 중인 연결 수
    in_use: AtomicUsize,
    /// 총 생성 횟수
    total_created: AtomicU64,
    /// 총 획득 횟수
    total_acquisitions: AtomicU64,
    /// 총 닫힌 횟수
    total_closed: AtomicU64,
    /// 다음 연결 ID
    next_id: AtomicU64,
    /// 열린 상태
    open: parking_lot::RwLock<bool>,
}

impl ConnectionPool {
    /// 새 연결 풀 생성
    pub fn new(address: ServerAddress, config: PoolConfig) -> Self {
        Self::with_auth(address, config, AuthToken::None)
    }

    /// 인증 토큰과 함께 연결 풀 생성
    pub fn with_auth(address: ServerAddress, config: PoolConfig, auth: AuthToken) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_size));

        Self {
            address,
            config,
            auth,
            idle_connections: Mutex::new(VecDeque::new()),
            semaphore,
            size: AtomicUsize::new(0),
            in_use: AtomicUsize::new(0),
            total_created: AtomicU64::new(0),
            total_acquisitions: AtomicU64::new(0),
            total_closed: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
            open: parking_lot::RwLock::new(true),
        }
    }

    /// 연결 획득
    pub async fn acquire(&self) -> DriverResult<PooledConnection> {
        if !*self.open.read() {
            return Err(DriverError::pool("Pool is closed"));
        }

        // 먼저 유휴 연결 확인
        if let Some(conn) = self.get_idle_connection() {
            self.total_acquisitions.fetch_add(1, Ordering::Relaxed);
            self.in_use.fetch_add(1, Ordering::Relaxed);
            return Ok(conn);
        }

        // 세마포어 획득 (타임아웃)
        let permit = tokio::time::timeout(
            self.config.connection_timeout,
            self.semaphore.clone().acquire_owned(),
        )
        .await
        .map_err(|_| DriverError::timeout("Connection acquisition timeout"))?
        .map_err(|_| DriverError::pool("Pool semaphore closed"))?;

        // 다시 유휴 연결 확인
        if let Some(conn) = self.get_idle_connection() {
            drop(permit);
            self.total_acquisitions.fetch_add(1, Ordering::Relaxed);
            self.in_use.fetch_add(1, Ordering::Relaxed);
            return Ok(conn);
        }

        // 새 연결 생성
        let conn = self.create_connection().await?;
        drop(permit);

        self.total_acquisitions.fetch_add(1, Ordering::Relaxed);
        self.in_use.fetch_add(1, Ordering::Relaxed);

        Ok(conn)
    }

    /// 유휴 연결 가져오기
    fn get_idle_connection(&self) -> Option<PooledConnection> {
        let mut idle = self.idle_connections.lock();

        while let Some(mut conn) = idle.pop_front() {
            if conn.is_valid(&self.config) {
                conn.mark_in_use();
                return Some(conn);
            } else {
                // 유효하지 않은 연결 닫기
                conn.mark_closed();
                self.size.fetch_sub(1, Ordering::Relaxed);
                self.total_closed.fetch_add(1, Ordering::Relaxed);
            }
        }

        None
    }

    /// 새 연결 생성
    async fn create_connection(&self) -> DriverResult<PooledConnection> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let address_str = self.address.to_socket_addr();

        // 실제 Bolt 연결 생성
        let mut client = BoltClient::connect(&address_str)
            .await
            .map_err(|e| DriverError::connection(format!("Failed to connect: {}", e)))?;

        // 인증
        let bolt_auth = to_bolt_auth(&self.auth);
        client
            .hello(Some(bolt_auth))
            .await
            .map_err(|e| DriverError::authentication(format!("Authentication failed: {}", e)))?;

        // PooledConnection 생성
        let mut conn = PooledConnection::with_bolt_client(id, self.address.clone(), client);
        conn.mark_in_use();
        self.size.fetch_add(1, Ordering::Relaxed);
        self.total_created.fetch_add(1, Ordering::Relaxed);

        Ok(conn)
    }

    /// 연결 반환
    pub fn return_connection(&self, mut conn: PooledConnection) {
        if !*self.open.read() {
            conn.mark_closed();
            self.size.fetch_sub(1, Ordering::Relaxed);
            self.total_closed.fetch_add(1, Ordering::Relaxed);
            return;
        }

        self.in_use.fetch_sub(1, Ordering::Relaxed);

        if conn.is_valid(&self.config) {
            conn.mark_idle();
            let mut idle = self.idle_connections.lock();
            idle.push_back(conn);
        } else {
            conn.mark_closed();
            self.size.fetch_sub(1, Ordering::Relaxed);
            self.total_closed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 풀 닫기
    pub async fn close(&self) -> DriverResult<()> {
        *self.open.write() = false;

        // Drain connections while holding lock, then close outside lock
        let connections_to_close: Vec<_> = {
            let mut idle = self.idle_connections.lock();
            idle.drain(..).collect()
        }; // Guard dropped before await

        for mut conn in connections_to_close {
            conn.close().await?;
            self.size.fetch_sub(1, Ordering::Relaxed);
            self.total_closed.fetch_add(1, Ordering::Relaxed);
        }

        Ok(())
    }

    /// 연결 확인
    pub async fn verify_connectivity(&self) -> DriverResult<()> {
        let mut conn = self.acquire().await?;
        // RESET 메시지로 연결 확인
        if let Some(ref mut client) = conn.bolt_client {
            client.reset().await.map_err(|e| {
                DriverError::connection(format!("Connection verification failed: {}", e))
            })?;
        }
        conn.return_to_pool();
        Ok(())
    }

    /// 연결 풀 워밍업
    ///
    /// 지정된 수의 연결을 미리 생성하여 유휴 상태로 유지합니다.
    /// 첫 번째 요청의 지연 시간을 줄이는 데 유용합니다.
    ///
    /// # 인자
    ///
    /// - `count`: 생성할 연결 수 (0이면 config의 설정 사용)
    ///
    /// # 반환
    ///
    /// - `Ok(usize)`: 성공적으로 생성된 연결 수
    /// - `Err`: 워밍업 중 오류 발생 (부분 성공 가능)
    ///
    /// # 예시
    ///
    /// ```rust,ignore
    /// let pool = ConnectionPool::with_auth(address, config, auth);
    ///
    /// // 5개 연결 미리 생성
    /// let warmed = pool.warmup(5).await?;
    /// println!("Warmed up {} connections", warmed);
    ///
    /// // config 설정 사용 (warmup_size 또는 min_idle)
    /// let warmed = pool.warmup(0).await?;
    /// ```
    pub async fn warmup(&self, count: usize) -> DriverResult<usize> {
        if !*self.open.read() {
            return Err(DriverError::pool("Pool is closed"));
        }

        // 워밍업 개수 결정
        let target = if count > 0 {
            count
        } else if self.config.warmup_size > 0 {
            self.config.warmup_size
        } else {
            self.config.min_idle
        };

        // max_size 초과 방지
        let target = target.min(self.config.max_size);

        // 이미 충분한 연결이 있으면 스킵
        let current_idle = self.idle_count();
        if current_idle >= target {
            return Ok(0);
        }

        let to_create = target - current_idle;
        let mut created = 0;

        for _ in 0..to_create {
            match self.create_connection().await {
                Ok(mut conn) => {
                    conn.mark_idle();
                    self.idle_connections.lock().push_back(conn);
                    created += 1;
                }
                Err(e) => {
                    // 부분 성공 시 경고만 로그
                    tracing::warn!("Warmup connection failed: {}", e);
                    // 첫 번째 연결도 실패하면 에러 반환
                    if created == 0 {
                        return Err(e);
                    }
                    break;
                }
            }
        }

        Ok(created)
    }

    /// 설정에 따른 자동 워밍업
    ///
    /// `warmup_on_init`이 true인 경우에만 워밍업을 수행합니다.
    ///
    /// # 반환
    ///
    /// - `Ok(Some(n))`: n개 연결 워밍업 완료
    /// - `Ok(None)`: 워밍업 비활성화됨
    /// - `Err`: 워밍업 실패
    pub async fn warmup_if_enabled(&self) -> DriverResult<Option<usize>> {
        if !self.config.warmup_on_init {
            return Ok(None);
        }

        let count = self.warmup(0).await?;
        Ok(Some(count))
    }

    /// 메트릭 조회
    pub fn metrics(&self) -> PoolMetrics {
        let idle = self.idle_connections.lock().len();

        PoolMetrics {
            size: self.size.load(Ordering::Relaxed),
            idle,
            in_use: self.in_use.load(Ordering::Relaxed),
            total_acquisitions: self.total_acquisitions.load(Ordering::Relaxed),
            total_created: self.total_created.load(Ordering::Relaxed),
            total_closed: self.total_closed.load(Ordering::Relaxed),
            total_timeouts: 0,
        }
    }

    /// 풀 크기
    pub fn size(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    /// 유휴 연결 수
    pub fn idle_count(&self) -> usize {
        self.idle_connections.lock().len()
    }

    /// 사용 중인 연결 수
    pub fn in_use_count(&self) -> usize {
        self.in_use.load(Ordering::Relaxed)
    }
}

impl std::fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionPool")
            .field("address", &self.address)
            .field("size", &self.size())
            .field("idle", &self.idle_count())
            .field("in_use", &self.in_use_count())
            .finish()
    }
}
