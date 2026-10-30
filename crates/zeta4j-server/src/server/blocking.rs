//! Blocking Pool Manager
//!
//! CPU 바운드 작업을 위한 스레드 풀 관리자.
//! SystemPool을 통해 Rayon 기반 CPU 바운드 알고리즘 작업을 실행합니다.
//!
//! # 개요
//!
//! - **SystemPool**: CPU 바운드 알고리즘 작업용 (기본 300초 타임아웃)
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_server::server::blocking::{BlockingPoolManager, BlockingPoolConfig, PoolType};
//!
//! let config = BlockingPoolConfig::default();
//! let manager = BlockingPoolManager::new(config)?;
//!
//! // 알고리즘 실행
//! let result = manager.spawn(PoolType::System, || {
//!     compute_pagerank()
//! }).await?;
//! ```

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};

use prometheus::{CounterVec, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry};
use rayon::ThreadPoolBuilder;
use tokio::sync::{Semaphore, oneshot};
use tracing::{debug, error, instrument, warn};

// ============================================================================
// Pool Type
// ============================================================================

/// 풀 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolType {
    /// 시스템 풀 (CPU 바운드 알고리즘, 300초 타임아웃)
    System,
}

impl PoolType {
    /// 메트릭 라벨용 문자열
    pub fn as_label(&self) -> &'static str {
        match self {
            PoolType::System => "system",
        }
    }
}

impl fmt::Display for PoolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_label())
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Blocking 작업 에러
#[derive(Debug)]
pub enum BlockingError<E = String> {
    /// 작업이 패닉 발생
    Panic(String),
    /// 작업이 취소됨
    Cancelled,
    /// 작업 타임아웃
    Timeout,
    /// 작업 실행 에러
    Task(E),
    /// 런타임 에러
    Runtime(String),
}

impl<E: fmt::Display> fmt::Display for BlockingError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Panic(msg) => write!(f, "Task panicked: {}", msg),
            Self::Cancelled => write!(f, "Task was cancelled"),
            Self::Timeout => write!(f, "Task timed out"),
            Self::Task(e) => write!(f, "Task error: {}", e),
            Self::Runtime(msg) => write!(f, "Runtime error: {}", msg),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for BlockingError<E> {}

// ============================================================================
// Configuration
// ============================================================================

/// 개별 풀 설정
#[derive(Debug, Clone)]
pub struct SinglePoolConfig {
    /// 스레드 수 (0 = CPU 코어 수 자동 감지)
    pub num_threads: usize,
    /// 작업 타임아웃
    pub timeout: Duration,
    /// 최대 동시 작업 수
    pub max_concurrent_tasks: usize,
    /// 스레드 이름 접두사
    pub thread_name_prefix: String,
}

impl Default for SinglePoolConfig {
    fn default() -> Self {
        Self {
            num_threads: 0,
            timeout: Duration::from_secs(30),
            max_concurrent_tasks: num_cpus::get() * 4,
            thread_name_prefix: "zeta4j-blocking".to_string(),
        }
    }
}

/// 블로킹 풀 전체 설정
#[derive(Debug, Clone)]
pub struct BlockingPoolConfig {
    /// 시스템 풀 설정
    pub system_pool: SinglePoolConfig,
    /// 메트릭 활성화
    pub metrics_enabled: bool,
}

impl Default for BlockingPoolConfig {
    fn default() -> Self {
        Self {
            system_pool: SinglePoolConfig {
                num_threads: num_cpus::get().max(2) / 2,
                timeout: Duration::from_secs(300),
                max_concurrent_tasks: num_cpus::get() * 2,
                thread_name_prefix: "zeta4j-system".to_string(),
            },
            metrics_enabled: true,
        }
    }
}

impl BlockingPoolConfig {
    /// 빌더 생성
    pub fn builder() -> BlockingPoolConfigBuilder {
        BlockingPoolConfigBuilder::default()
    }
}

/// 설정 빌더
#[derive(Default)]
pub struct BlockingPoolConfigBuilder {
    config: BlockingPoolConfig,
}

impl BlockingPoolConfigBuilder {
    /// 시스템 풀 스레드 수 설정
    pub fn system_threads(mut self, n: usize) -> Self {
        self.config.system_pool.num_threads = n;
        self
    }

    /// 시스템 풀 타임아웃 설정
    pub fn system_timeout(mut self, timeout: Duration) -> Self {
        self.config.system_pool.timeout = timeout;
        self
    }

    /// 시스템 풀 최대 동시 작업 수 설정
    pub fn system_max_concurrent(mut self, n: usize) -> Self {
        self.config.system_pool.max_concurrent_tasks = n;
        self
    }

    /// 메트릭 활성화 설정
    pub fn metrics_enabled(mut self, enabled: bool) -> Self {
        self.config.metrics_enabled = enabled;
        self
    }

    /// 설정 빌드
    pub fn build(self) -> BlockingPoolConfig {
        self.config
    }
}

// ============================================================================
// Metrics
// ============================================================================

/// 블로킹 풀 Prometheus 메트릭
pub struct BlockingMetrics {
    /// 작업 소요 시간
    pub task_duration: HistogramVec,
    /// 총 작업 수
    pub tasks_total: CounterVec,
    /// 현재 실행 중인 작업 수
    pub tasks_in_flight: GaugeVec,
    /// 타임아웃 횟수
    pub timeouts_total: CounterVec,
    /// 패닉 횟수
    pub panics_total: CounterVec,
    /// 대기열 대기 시간
    pub queue_wait_time: HistogramVec,
    /// 풀 포화도 (0-1)
    pub pool_saturation: GaugeVec,
}

impl BlockingMetrics {
    /// 새 메트릭 인스턴스 생성 및 레지스트리에 등록
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let task_duration = HistogramVec::new(
            HistogramOpts::new(
                "zeta4j_blocking_task_duration_seconds",
                "Duration of blocking tasks",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0,
            ]),
            &["pool", "status"],
        )?;

        let tasks_total = CounterVec::new(
            Opts::new(
                "zeta4j_blocking_tasks_total",
                "Total blocking tasks executed",
            ),
            &["pool", "status"],
        )?;

        let tasks_in_flight = GaugeVec::new(
            Opts::new(
                "zeta4j_blocking_tasks_in_flight",
                "Currently executing blocking tasks",
            ),
            &["pool"],
        )?;

        let timeouts_total = CounterVec::new(
            Opts::new(
                "zeta4j_blocking_timeouts_total",
                "Total blocking task timeouts",
            ),
            &["pool"],
        )?;

        let panics_total = CounterVec::new(
            Opts::new("zeta4j_blocking_panics_total", "Total blocking task panics"),
            &["pool"],
        )?;

        let queue_wait_time = HistogramVec::new(
            HistogramOpts::new(
                "zeta4j_blocking_queue_wait_seconds",
                "Time waiting for pool slot",
            )
            .buckets(vec![0.0001, 0.001, 0.01, 0.1, 1.0, 10.0]),
            &["pool"],
        )?;

        let pool_saturation = GaugeVec::new(
            Opts::new(
                "zeta4j_blocking_pool_saturation",
                "Pool saturation ratio (0-1)",
            ),
            &["pool"],
        )?;

        // 레지스트리에 등록
        registry.register(Box::new(task_duration.clone()))?;
        registry.register(Box::new(tasks_total.clone()))?;
        registry.register(Box::new(tasks_in_flight.clone()))?;
        registry.register(Box::new(timeouts_total.clone()))?;
        registry.register(Box::new(panics_total.clone()))?;
        registry.register(Box::new(queue_wait_time.clone()))?;
        registry.register(Box::new(pool_saturation.clone()))?;

        Ok(Self {
            task_duration,
            tasks_total,
            tasks_in_flight,
            timeouts_total,
            panics_total,
            queue_wait_time,
            pool_saturation,
        })
    }

    /// 기본 레지스트리로 생성
    pub fn with_default_registry() -> Result<Self, prometheus::Error> {
        Self::new(prometheus::default_registry())
    }
}

// ============================================================================
// Pool Manager
// ============================================================================

/// 블로킹 풀 관리자
pub struct BlockingPoolManager {
    system_pool: rayon::ThreadPool,
    config: BlockingPoolConfig,
    system_semaphore: Arc<Semaphore>,
    metrics: Option<Arc<BlockingMetrics>>,
}

impl BlockingPoolManager {
    /// 새 풀 매니저 생성
    pub fn new(config: BlockingPoolConfig) -> Result<Self, BlockingPoolError> {
        let system_threads = if config.system_pool.num_threads == 0 {
            num_cpus::get().max(2) / 2
        } else {
            config.system_pool.num_threads
        };

        let system_prefix = config.system_pool.thread_name_prefix.clone();
        let system_pool = ThreadPoolBuilder::new()
            .num_threads(system_threads)
            .thread_name(move |i| format!("{}-{}", system_prefix, i))
            .build()
            .map_err(|e| BlockingPoolError::PoolCreation(e.to_string()))?;

        let system_semaphore = Arc::new(Semaphore::new(config.system_pool.max_concurrent_tasks));

        debug!(
            system_threads = system_threads,
            system_max_concurrent = config.system_pool.max_concurrent_tasks,
            "Blocking pool manager initialized"
        );

        Ok(Self {
            system_pool,
            config,
            system_semaphore,
            metrics: None,
        })
    }

    /// 메트릭 연결
    pub fn with_metrics(mut self, metrics: Arc<BlockingMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// 풀 타입에 따른 작업 실행
    #[instrument(skip(self, f), fields(pool = %pool_type))]
    pub async fn spawn<F, T, E>(&self, pool_type: PoolType, f: F) -> Result<T, BlockingError<E>>
    where
        F: FnOnce() -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        let timeout = match pool_type {
            PoolType::System => self.config.system_pool.timeout,
        };

        self.spawn_with_timeout(pool_type, timeout, f).await
    }

    /// 커스텀 타임아웃으로 작업 실행
    pub async fn spawn_with_timeout<F, T, E>(
        &self,
        pool_type: PoolType,
        timeout: Duration,
        f: F,
    ) -> Result<T, BlockingError<E>>
    where
        F: FnOnce() -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        let pool_label = pool_type.as_label();
        let start = Instant::now();

        // 세마포어 및 풀 선택
        let (semaphore, pool) = match pool_type {
            PoolType::System => (&self.system_semaphore, &self.system_pool),
        };

        // 대기열 대기 시간 측정
        let wait_start = Instant::now();
        let _permit = semaphore
            .acquire()
            .await
            .map_err(|_| BlockingError::Runtime("Semaphore closed".to_string()))?;

        // 메트릭 기록
        if let Some(ref metrics) = self.metrics {
            metrics
                .queue_wait_time
                .with_label_values(&[pool_label])
                .observe(wait_start.elapsed().as_secs_f64());
            metrics
                .tasks_in_flight
                .with_label_values(&[pool_label])
                .inc();

            // 포화도 업데이트
            let max = match pool_type {
                PoolType::System => self.config.system_pool.max_concurrent_tasks,
            };
            let current = max - semaphore.available_permits();
            metrics
                .pool_saturation
                .with_label_values(&[pool_label])
                .set(current as f64 / max as f64);
        }

        // 결과 채널 생성
        let (tx, rx) = oneshot::channel();

        // 풀에서 실행
        pool.spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(f));
            let _ = tx.send(result);
        });

        // 타임아웃과 함께 대기
        let result = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(result))) => {
                // 성공
                if let Some(ref metrics) = self.metrics {
                    let duration = start.elapsed().as_secs_f64();
                    metrics
                        .task_duration
                        .with_label_values(&[pool_label, "success"])
                        .observe(duration);
                    metrics
                        .tasks_total
                        .with_label_values(&[pool_label, "success"])
                        .inc();
                }
                result.map_err(BlockingError::Task)
            }
            Ok(Ok(Err(panic))) => {
                // 패닉 발생
                if let Some(ref metrics) = self.metrics {
                    metrics.panics_total.with_label_values(&[pool_label]).inc();
                    metrics
                        .tasks_total
                        .with_label_values(&[pool_label, "panic"])
                        .inc();
                }
                let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };
                error!(pool = pool_label, panic = %msg, "Task panicked");
                Err(BlockingError::Panic(msg))
            }
            Ok(Err(_)) => {
                // 채널 닫힘
                Err(BlockingError::Cancelled)
            }
            Err(_) => {
                // 타임아웃
                if let Some(ref metrics) = self.metrics {
                    metrics
                        .timeouts_total
                        .with_label_values(&[pool_label])
                        .inc();
                    metrics
                        .tasks_total
                        .with_label_values(&[pool_label, "timeout"])
                        .inc();
                }
                warn!(
                    pool = pool_label,
                    timeout_secs = timeout.as_secs(),
                    "Task timed out"
                );
                Err(BlockingError::Timeout)
            }
        };

        // in-flight 감소
        if let Some(ref metrics) = self.metrics {
            metrics
                .tasks_in_flight
                .with_label_values(&[pool_label])
                .dec();
        }

        result
    }

    /// 알고리즘 작업 실행 (편의 메서드)
    pub async fn spawn_algorithm<F, T, E>(
        &self,
        algorithm_name: &str,
        f: F,
    ) -> Result<T, BlockingError<E>>
    where
        F: FnOnce() -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: fmt::Display + Send + 'static,
    {
        debug!(algorithm = algorithm_name, "Spawning algorithm task");
        let start = Instant::now();

        let result = self.spawn(PoolType::System, f).await;

        match &result {
            Ok(_) => {
                debug!(algorithm = algorithm_name, elapsed = ?start.elapsed(), "Algorithm completed")
            }
            Err(e) => error!(algorithm = algorithm_name, error = %e, "Algorithm failed"),
        }

        result
    }

    /// 현재 풀 통계
    pub fn stats(&self) -> BlockingPoolStats {
        BlockingPoolStats {
            system_available_permits: self.system_semaphore.available_permits(),
            system_max_permits: self.config.system_pool.max_concurrent_tasks,
        }
    }
}

// ============================================================================
// Supporting Types
// ============================================================================

/// 풀 관리자 에러
#[derive(Debug)]
pub enum BlockingPoolError {
    /// 풀 생성 실패
    PoolCreation(String),
}

impl fmt::Display for BlockingPoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockingPoolError::PoolCreation(msg) => {
                write!(f, "Failed to create thread pool: {}", msg)
            }
        }
    }
}

impl std::error::Error for BlockingPoolError {}

/// 풀 통계 스냅샷
#[derive(Debug, Clone)]
pub struct BlockingPoolStats {
    /// 시스템 풀 사용 가능 슬롯
    pub system_available_permits: usize,
    /// 시스템 풀 최대 슬롯
    pub system_max_permits: usize,
}

impl BlockingPoolStats {
    /// 시스템 풀 사용 중 슬롯
    pub fn system_in_use(&self) -> usize {
        self.system_max_permits - self.system_available_permits
    }

    /// 시스템 풀 포화도
    pub fn system_saturation(&self) -> f64 {
        self.system_in_use() as f64 / self.system_max_permits as f64
    }
}
