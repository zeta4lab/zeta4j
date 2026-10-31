// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server Connection Pool
//!
//! Phase 2 Step 5 M5.1: 서버 연결 풀링
//!
//! 기능:
//! - 서버 측 연결 풀 관리
//! - 동적 풀 크기 조정 (AutoScaler)
//! - 유휴 연결 자동 정리 (CleanerTask)

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use tokio::sync::Notify;
use tokio::time::interval;

use crate::server::core::pool::{ConnectionPool, PoolConfig, PoolError, Poolable};

/// 서버 연결 풀 설정
#[derive(Debug, Clone)]
pub struct ServerPoolConfig {
    /// 기본 풀 설정
    pub pool: PoolConfig,
    /// 자동 스케일링 활성화
    pub auto_scaling: bool,
    /// 자동 스케일링 설정
    pub autoscaler: AutoScalerConfig,
    /// 자동 정리 활성화
    pub auto_cleanup: bool,
    /// 정리 주기
    pub cleanup_interval: Duration,
}

impl Default for ServerPoolConfig {
    fn default() -> Self {
        Self {
            pool: PoolConfig::default(),
            auto_scaling: true,
            autoscaler: AutoScalerConfig::default(),
            auto_cleanup: true,
            cleanup_interval: Duration::from_secs(60),
        }
    }
}

impl ServerPoolConfig {
    /// 빌더 생성
    pub fn builder() -> ServerPoolConfigBuilder {
        ServerPoolConfigBuilder::default()
    }
}

/// 서버 풀 설정 빌더
#[derive(Default)]
pub struct ServerPoolConfigBuilder {
    pool: Option<PoolConfig>,
    auto_scaling: Option<bool>,
    autoscaler: Option<AutoScalerConfig>,
    auto_cleanup: Option<bool>,
    cleanup_interval: Option<Duration>,
}

impl ServerPoolConfigBuilder {
    pub fn pool(mut self, config: PoolConfig) -> Self {
        self.pool = Some(config);
        self
    }

    pub fn auto_scaling(mut self, enabled: bool) -> Self {
        self.auto_scaling = Some(enabled);
        self
    }

    pub fn autoscaler(mut self, config: AutoScalerConfig) -> Self {
        self.autoscaler = Some(config);
        self
    }

    pub fn auto_cleanup(mut self, enabled: bool) -> Self {
        self.auto_cleanup = Some(enabled);
        self
    }

    pub fn cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = Some(interval);
        self
    }

    pub fn build(self) -> ServerPoolConfig {
        let default = ServerPoolConfig::default();
        ServerPoolConfig {
            pool: self.pool.unwrap_or(default.pool),
            auto_scaling: self.auto_scaling.unwrap_or(default.auto_scaling),
            autoscaler: self.autoscaler.unwrap_or(default.autoscaler),
            auto_cleanup: self.auto_cleanup.unwrap_or(default.auto_cleanup),
            cleanup_interval: self.cleanup_interval.unwrap_or(default.cleanup_interval),
        }
    }
}

/// 자동 스케일러 설정
#[derive(Debug, Clone)]
pub struct AutoScalerConfig {
    /// 스케일 업 임계값 (사용률 %)
    pub scale_up_threshold: f64,
    /// 스케일 다운 임계값 (사용률 %)
    pub scale_down_threshold: f64,
    /// 스케일 단위
    pub scale_step: usize,
    /// 최소 연결 수
    pub min_connections: usize,
    /// 최대 연결 수
    pub max_connections: usize,
    /// 평가 주기
    pub evaluation_interval: Duration,
    /// 쿨다운 기간 (스케일 후 대기 시간)
    pub cooldown_period: Duration,
    /// 메트릭 히스토리 크기
    pub history_size: usize,
}

impl Default for AutoScalerConfig {
    fn default() -> Self {
        Self {
            scale_up_threshold: 80.0,
            scale_down_threshold: 20.0,
            scale_step: 10,
            min_connections: 5,
            max_connections: 1000,
            evaluation_interval: Duration::from_secs(30),
            cooldown_period: Duration::from_secs(60),
            history_size: 10,
        }
    }
}

impl AutoScalerConfig {
    /// 빌더 생성
    pub fn builder() -> AutoScalerConfigBuilder {
        AutoScalerConfigBuilder::default()
    }

    /// 설정 검증
    pub fn validate(&self) -> Result<(), String> {
        if self.scale_up_threshold <= self.scale_down_threshold {
            return Err("scale_up_threshold must be > scale_down_threshold".into());
        }
        if self.min_connections > self.max_connections {
            return Err("min_connections must be <= max_connections".into());
        }
        if self.scale_step == 0 {
            return Err("scale_step must be > 0".into());
        }
        Ok(())
    }
}

/// 자동 스케일러 설정 빌더
#[derive(Default)]
pub struct AutoScalerConfigBuilder {
    scale_up_threshold: Option<f64>,
    scale_down_threshold: Option<f64>,
    scale_step: Option<usize>,
    min_connections: Option<usize>,
    max_connections: Option<usize>,
    evaluation_interval: Option<Duration>,
    cooldown_period: Option<Duration>,
    history_size: Option<usize>,
}

impl AutoScalerConfigBuilder {
    pub fn scale_up_threshold(mut self, threshold: f64) -> Self {
        self.scale_up_threshold = Some(threshold);
        self
    }

    pub fn scale_down_threshold(mut self, threshold: f64) -> Self {
        self.scale_down_threshold = Some(threshold);
        self
    }

    pub fn scale_step(mut self, step: usize) -> Self {
        self.scale_step = Some(step);
        self
    }

    pub fn min_connections(mut self, min: usize) -> Self {
        self.min_connections = Some(min);
        self
    }

    pub fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = Some(max);
        self
    }

    pub fn evaluation_interval(mut self, interval: Duration) -> Self {
        self.evaluation_interval = Some(interval);
        self
    }

    pub fn cooldown_period(mut self, period: Duration) -> Self {
        self.cooldown_period = Some(period);
        self
    }

    pub fn history_size(mut self, size: usize) -> Self {
        self.history_size = Some(size);
        self
    }

    pub fn build(self) -> AutoScalerConfig {
        let default = AutoScalerConfig::default();
        AutoScalerConfig {
            scale_up_threshold: self
                .scale_up_threshold
                .unwrap_or(default.scale_up_threshold),
            scale_down_threshold: self
                .scale_down_threshold
                .unwrap_or(default.scale_down_threshold),
            scale_step: self.scale_step.unwrap_or(default.scale_step),
            min_connections: self.min_connections.unwrap_or(default.min_connections),
            max_connections: self.max_connections.unwrap_or(default.max_connections),
            evaluation_interval: self
                .evaluation_interval
                .unwrap_or(default.evaluation_interval),
            cooldown_period: self.cooldown_period.unwrap_or(default.cooldown_period),
            history_size: self.history_size.unwrap_or(default.history_size),
        }
    }
}

/// 풀 메트릭 스냅샷
#[derive(Debug, Clone)]
pub struct PoolMetrics {
    /// 활성 연결 수
    pub active: usize,
    /// 유휴 연결 수
    pub idle: usize,
    /// 최대 연결 수
    pub max: usize,
    /// 사용률 (%)
    pub usage_percent: f64,
    /// 기록 시간
    pub timestamp: Instant,
}

/// 자동 스케일러
#[derive(Debug)]
pub struct AutoScaler {
    config: AutoScalerConfig,
    metrics_history: Mutex<VecDeque<PoolMetrics>>,
    current_max: AtomicUsize,
    last_scale_time: RwLock<Option<Instant>>,
    scale_up_count: AtomicU64,
    scale_down_count: AtomicU64,
}

impl AutoScaler {
    /// 새 AutoScaler 생성
    pub fn new(config: AutoScalerConfig) -> Self {
        let initial_max = config.min_connections.max(10);
        Self {
            config,
            metrics_history: Mutex::new(VecDeque::new()),
            current_max: AtomicUsize::new(initial_max),
            last_scale_time: RwLock::new(None),
            scale_up_count: AtomicU64::new(0),
            scale_down_count: AtomicU64::new(0),
        }
    }

    /// 메트릭 기록
    pub fn record_metrics(&self, metrics: PoolMetrics) {
        let mut history = self.metrics_history.lock();
        history.push_back(metrics);
        if history.len() > self.config.history_size {
            history.pop_front();
        }
    }

    /// 평균 사용률 계산
    pub fn avg_usage(&self) -> f64 {
        let history = self.metrics_history.lock();
        if history.is_empty() {
            return 0.0;
        }
        let sum: f64 = history.iter().map(|m| m.usage_percent).sum();
        sum / history.len() as f64
    }

    /// 스케일링 결정
    pub fn evaluate(&self) -> ScaleDecision {
        // 쿨다운 체크
        if let Some(last_time) = *self.last_scale_time.read()
            && last_time.elapsed() < self.config.cooldown_period
        {
            return ScaleDecision::Hold;
        }

        let avg_usage = self.avg_usage();
        let current_max = self.current_max.load(Ordering::Acquire);

        if avg_usage >= self.config.scale_up_threshold {
            let new_max = (current_max + self.config.scale_step).min(self.config.max_connections);
            if new_max > current_max {
                return ScaleDecision::ScaleUp { new_max };
            }
        } else if avg_usage <= self.config.scale_down_threshold {
            let new_max = current_max
                .saturating_sub(self.config.scale_step)
                .max(self.config.min_connections);
            if new_max < current_max {
                return ScaleDecision::ScaleDown { new_max };
            }
        }

        ScaleDecision::Hold
    }

    /// 스케일 적용
    pub fn apply_scale(&self, new_max: usize, is_up: bool) {
        self.current_max.store(new_max, Ordering::Release);
        *self.last_scale_time.write() = Some(Instant::now());

        if is_up {
            self.scale_up_count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.scale_down_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 현재 최대 연결 수
    pub fn current_max(&self) -> usize {
        self.current_max.load(Ordering::Acquire)
    }

    /// 스케일 업 횟수
    pub fn scale_up_count(&self) -> u64 {
        self.scale_up_count.load(Ordering::Relaxed)
    }

    /// 스케일 다운 횟수
    pub fn scale_down_count(&self) -> u64 {
        self.scale_down_count.load(Ordering::Relaxed)
    }

    /// 설정 조회
    pub fn config(&self) -> &AutoScalerConfig {
        &self.config
    }
}

/// 스케일 결정
#[derive(Debug, Clone, PartialEq)]
pub enum ScaleDecision {
    /// 유지
    Hold,
    /// 스케일 업
    ScaleUp { new_max: usize },
    /// 스케일 다운
    ScaleDown { new_max: usize },
}

/// 유휴 연결 정리 태스크
pub struct CleanerTask {
    interval: Duration,
    running: AtomicBool,
    shutdown_notify: Arc<Notify>,
    cleaned_count: AtomicU64,
}

impl CleanerTask {
    /// 새 CleanerTask 생성
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            running: AtomicBool::new(false),
            shutdown_notify: Arc::new(Notify::new()),
            cleaned_count: AtomicU64::new(0),
        }
    }

    /// 실행 중인지 확인
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// 정리된 연결 수
    pub fn cleaned_count(&self) -> u64 {
        self.cleaned_count.load(Ordering::Relaxed)
    }

    /// 정리 횟수 증가
    pub fn record_cleanup(&self, count: usize) {
        self.cleaned_count
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// 시작
    pub fn start(&self) {
        self.running.store(true, Ordering::Release);
    }

    /// 종료 신호
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.shutdown_notify.notify_waiters();
    }

    /// 종료 대기
    pub async fn wait_shutdown(&self) {
        self.shutdown_notify.notified().await;
    }

    /// 정리 주기
    pub fn interval(&self) -> Duration {
        self.interval
    }
}

impl std::fmt::Debug for CleanerTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CleanerTask")
            .field("interval", &self.interval)
            .field("running", &self.running.load(Ordering::Relaxed))
            .field("cleaned_count", &self.cleaned_count.load(Ordering::Relaxed))
            .finish()
    }
}

/// 서버 풀 통계
#[derive(Debug, Clone)]
pub struct ServerPoolStats {
    /// 총 획득 수
    pub acquired: u64,
    /// 총 반환 수
    pub returned: u64,
    /// 총 생성 수
    pub created: u64,
    /// 총 폐기 수
    pub destroyed: u64,
    /// 타임아웃 수
    pub timeouts: u64,
    /// 스케일 업 횟수
    pub scale_ups: u64,
    /// 스케일 다운 횟수
    pub scale_downs: u64,
    /// 정리된 연결 수
    pub cleaned: u64,
}

/// 서버 풀 상태
#[derive(Debug, Clone)]
pub struct ServerPoolStatus {
    /// 활성 연결 수
    pub active: usize,
    /// 유휴 연결 수
    pub idle: usize,
    /// 현재 최대 연결 수
    pub current_max: usize,
    /// 설정된 최대 연결 수
    pub config_max: usize,
    /// 풀 닫힘 여부
    pub closed: bool,
    /// 자동 스케일링 활성화 여부
    pub auto_scaling_enabled: bool,
    /// 자동 정리 활성화 여부
    pub auto_cleanup_enabled: bool,
}

/// 서버 연결 풀
///
/// 기존 ConnectionPool을 래핑하여 서버 전용 기능 추가:
/// - 동적 스케일링
/// - 백그라운드 정리 태스크
/// - 확장된 메트릭
pub struct ServerConnectionPool<T: Poolable> {
    config: ServerPoolConfig,
    inner_pool: ConnectionPool<T>,
    autoscaler: Option<Arc<AutoScaler>>,
    cleaner: Option<Arc<CleanerTask>>,
    closed: AtomicBool,
}

impl<T: Poolable> ServerConnectionPool<T> {
    /// 새 서버 연결 풀 생성
    pub fn new(config: ServerPoolConfig) -> Result<Self, PoolError> {
        let inner_pool = ConnectionPool::new(config.pool.clone())?;

        let autoscaler = if config.auto_scaling {
            Some(Arc::new(AutoScaler::new(config.autoscaler.clone())))
        } else {
            None
        };

        let cleaner = if config.auto_cleanup {
            Some(Arc::new(CleanerTask::new(config.cleanup_interval)))
        } else {
            None
        };

        Ok(Self {
            config,
            inner_pool,
            autoscaler,
            cleaner,
            closed: AtomicBool::new(false),
        })
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Result<Self, PoolError> {
        Self::new(ServerPoolConfig::default())
    }

    /// 연결 획득
    pub async fn acquire<F, Fut>(
        &self,
        create_fn: F,
    ) -> Result<crate::server::core::pool::PooledConnection<T>, PoolError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(PoolError::Closed);
        }

        let result = self.inner_pool.acquire(create_fn).await;

        // 메트릭 기록 (스케일러 활성화 시)
        if let Some(ref scaler) = self.autoscaler {
            let metrics = PoolMetrics {
                active: self.inner_pool.active_count(),
                idle: self.inner_pool.idle_count(),
                max: self.inner_pool.max_connections(),
                usage_percent: self.inner_pool.usage_percent(),
                timestamp: Instant::now(),
            };
            scaler.record_metrics(metrics);
        }

        result
    }

    /// 스케일링 평가 및 적용
    pub fn evaluate_and_scale(&self) -> Option<ScaleDecision> {
        let scaler = self.autoscaler.as_ref()?;
        let decision = scaler.evaluate();

        match &decision {
            ScaleDecision::ScaleUp { new_max } => {
                scaler.apply_scale(*new_max, true);
            }
            ScaleDecision::ScaleDown { new_max } => {
                scaler.apply_scale(*new_max, false);
            }
            ScaleDecision::Hold => {}
        }

        Some(decision)
    }

    /// 유휴 연결 정리
    pub fn cleanup_idle(&self) -> usize {
        self.inner_pool.cleanup_idle();
        let count = self.inner_pool.stats().destroyed;

        if let Some(ref cleaner) = self.cleaner {
            cleaner.record_cleanup(1);
        }

        count as usize
    }

    /// 백그라운드 태스크 시작
    ///
    /// `shutdown_rx`는 ShutdownController.watch()로 얻은 수신자입니다.
    pub async fn run_background_tasks(
        self: Arc<Self>,
        shutdown_rx: tokio::sync::watch::Receiver<crate::server::core::shutdown::ShutdownState>,
    ) {
        let pool = Arc::clone(&self);

        // 자동 스케일링 태스크
        if let Some(ref scaler) = self.autoscaler {
            let pool_clone = Arc::clone(&pool);
            let mut shutdown_watch = shutdown_rx.clone();
            let interval_duration = scaler.config().evaluation_interval;

            tokio::spawn(async move {
                let mut ticker = interval(interval_duration);
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            pool_clone.evaluate_and_scale();
                        }
                        _ = shutdown_watch.changed() => {
                            break;
                        }
                    }
                }
            });
        }

        // 자동 정리 태스크
        if let Some(ref cleaner) = self.cleaner {
            let pool_clone = Arc::clone(&pool);
            let mut shutdown_watch = shutdown_rx.clone();
            let interval_duration = cleaner.interval();

            cleaner.start();

            tokio::spawn(async move {
                let mut ticker = interval(interval_duration);
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            pool_clone.cleanup_idle();
                        }
                        _ = shutdown_watch.changed() => {
                            if let Some(ref c) = pool_clone.cleaner {
                                c.stop();
                            }
                            break;
                        }
                    }
                }
            });
        }
    }

    /// 풀 상태
    pub fn status(&self) -> ServerPoolStatus {
        let inner_status = self.inner_pool.status();
        ServerPoolStatus {
            active: inner_status.active,
            idle: inner_status.idle,
            current_max: self
                .autoscaler
                .as_ref()
                .map(|s| s.current_max())
                .unwrap_or(inner_status.max),
            config_max: inner_status.max,
            closed: self.closed.load(Ordering::Acquire),
            auto_scaling_enabled: self.autoscaler.is_some(),
            auto_cleanup_enabled: self.cleaner.is_some(),
        }
    }

    /// 풀 통계
    pub fn stats(&self) -> ServerPoolStats {
        let inner_stats = self.inner_pool.stats();
        ServerPoolStats {
            acquired: inner_stats.acquired,
            returned: inner_stats.returned,
            created: inner_stats.created,
            destroyed: inner_stats.destroyed,
            timeouts: inner_stats.timeouts,
            scale_ups: self
                .autoscaler
                .as_ref()
                .map(|s| s.scale_up_count())
                .unwrap_or(0),
            scale_downs: self
                .autoscaler
                .as_ref()
                .map(|s| s.scale_down_count())
                .unwrap_or(0),
            cleaned: self
                .cleaner
                .as_ref()
                .map(|c| c.cleaned_count())
                .unwrap_or(0),
        }
    }

    /// 풀 닫기
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.inner_pool.close();

        if let Some(ref cleaner) = self.cleaner {
            cleaner.stop();
        }
    }

    /// 풀이 닫혔는지 확인
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// 설정 조회
    pub fn config(&self) -> &ServerPoolConfig {
        &self.config
    }

    /// 활성 연결 수
    pub fn active_count(&self) -> usize {
        self.inner_pool.active_count()
    }

    /// 유휴 연결 수
    pub fn idle_count(&self) -> usize {
        self.inner_pool.idle_count()
    }

    /// 총 연결 수
    pub fn total_count(&self) -> usize {
        self.inner_pool.total_count()
    }

    /// 사용률 (%)
    pub fn usage_percent(&self) -> f64 {
        self.inner_pool.usage_percent()
    }
}

impl<T: Poolable> Clone for ServerConnectionPool<T> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            inner_pool: self.inner_pool.clone(),
            autoscaler: self.autoscaler.clone(),
            cleaner: self.cleaner.clone(),
            closed: AtomicBool::new(self.closed.load(Ordering::Acquire)),
        }
    }
}
