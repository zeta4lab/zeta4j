//! Resource Isolation Module (M6.2)
//!
//! 리소스 격리 및 제한

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::database::DatabaseId;

// ============================================================================
// Resource Limits
// ============================================================================

/// 리소스 제한
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// 최대 스토리지 (바이트)
    pub max_storage: u64,
    /// CPU 가중치 (1-100)
    pub cpu_weight: u8,
    /// 최대 연결 수
    pub max_connections: u32,
    /// 최대 트랜잭션 수
    pub max_transactions: u32,
    /// 최대 쿼리 수/분
    pub max_queries_per_minute: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_storage: 10 * 1024 * 1024 * 1024, // 10GB
            cpu_weight: 50,
            max_connections: 100,
            max_transactions: 1000,
            max_queries_per_minute: 10000,
        }
    }
}

/// 리소스 제한 빌더
#[derive(Debug, Default)]
pub struct ResourceLimitsBuilder {
    limits: ResourceLimits,
}

impl ResourceLimitsBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_storage(mut self, bytes: u64) -> Self {
        self.limits.max_storage = bytes;
        self
    }

    pub fn max_storage_gb(mut self, gb: u64) -> Self {
        self.limits.max_storage = gb * 1024 * 1024 * 1024;
        self
    }

    pub fn cpu_weight(mut self, weight: u8) -> Self {
        self.limits.cpu_weight = weight.clamp(1, 100);
        self
    }

    pub fn max_connections(mut self, count: u32) -> Self {
        self.limits.max_connections = count;
        self
    }

    pub fn max_transactions(mut self, count: u32) -> Self {
        self.limits.max_transactions = count;
        self
    }

    pub fn max_queries_per_minute(mut self, count: u32) -> Self {
        self.limits.max_queries_per_minute = count;
        self
    }

    pub fn build(self) -> ResourceLimits {
        self.limits
    }
}

// ============================================================================
// Resource Usage
// ============================================================================

/// 리소스 사용량
#[derive(Debug)]
pub struct ResourceUsage {
    /// 현재 스토리지 사용량
    pub current_storage: AtomicU64,
    /// 현재 연결 수
    pub current_connections: AtomicU32,
    /// 현재 트랜잭션 수
    pub current_transactions: AtomicU32,
    /// 분당 쿼리 카운터
    query_counter: AtomicU64,
    /// 쿼리 카운터 시작 시간
    query_window_start: RwLock<Instant>,
    /// 마지막 업데이트
    last_updated: RwLock<Instant>,
}

impl ResourceUsage {
    /// 새 사용량 객체 생성
    pub fn new() -> Self {
        Self {
            current_storage: AtomicU64::new(0),
            current_connections: AtomicU32::new(0),
            current_transactions: AtomicU32::new(0),
            query_counter: AtomicU64::new(0),
            query_window_start: RwLock::new(Instant::now()),
            last_updated: RwLock::new(Instant::now()),
        }
    }

    /// 연결 추가
    pub fn add_connection(&self) {
        self.current_connections.fetch_add(1, Ordering::SeqCst);
        self.touch();
    }

    /// 연결 제거
    pub fn remove_connection(&self) {
        self.current_connections.fetch_sub(1, Ordering::SeqCst);
        self.touch();
    }

    /// 트랜잭션 시작
    pub fn start_transaction(&self) {
        self.current_transactions.fetch_add(1, Ordering::SeqCst);
        self.touch();
    }

    /// 트랜잭션 종료
    pub fn end_transaction(&self) {
        self.current_transactions.fetch_sub(1, Ordering::SeqCst);
        self.touch();
    }

    /// 쿼리 기록
    pub fn record_query(&self) {
        // 1분 윈도우 체크
        let window_start = *self.query_window_start.read();
        if window_start.elapsed() >= Duration::from_secs(60) {
            // 새 윈도우 시작
            let mut ws = self.query_window_start.write();
            *ws = Instant::now();
            self.query_counter.store(1, Ordering::SeqCst);
        } else {
            self.query_counter.fetch_add(1, Ordering::SeqCst);
        }
        self.touch();
    }

    /// 분당 쿼리 수
    pub fn queries_per_minute(&self) -> u32 {
        let window_start = *self.query_window_start.read();
        if window_start.elapsed() >= Duration::from_secs(60) {
            0
        } else {
            self.query_counter.load(Ordering::SeqCst) as u32
        }
    }

    /// 마지막 업데이트 시간 갱신
    fn touch(&self) {
        let mut last = self.last_updated.write();
        *last = Instant::now();
    }

    /// 스냅샷 생성
    pub fn snapshot(&self) -> ResourceUsageSnapshot {
        ResourceUsageSnapshot {
            current_storage: self.current_storage.load(Ordering::SeqCst),
            current_connections: self.current_connections.load(Ordering::SeqCst),
            current_transactions: self.current_transactions.load(Ordering::SeqCst),
            queries_per_minute: self.queries_per_minute(),
            last_updated: *self.last_updated.read(),
        }
    }
}

impl Default for ResourceUsage {
    fn default() -> Self {
        Self::new()
    }
}

/// 리소스 사용량 스냅샷
#[derive(Debug, Clone)]
pub struct ResourceUsageSnapshot {
    pub current_storage: u64,
    pub current_connections: u32,
    pub current_transactions: u32,
    pub queries_per_minute: u32,
    pub last_updated: Instant,
}

// ============================================================================
// Resource Type
// ============================================================================

/// 리소스 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Storage,
    Connections,
    Transactions,
    QueriesPerMinute,
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceType::Storage => write!(f, "storage"),
            ResourceType::Connections => write!(f, "connections"),
            ResourceType::Transactions => write!(f, "transactions"),
            ResourceType::QueriesPerMinute => write!(f, "queries_per_minute"),
        }
    }
}

// ============================================================================
// Resource Action
// ============================================================================

/// 리소스 제한 초과 시 액션
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceAction {
    /// 허용
    Allow,
    /// 거부
    Deny,
    /// 일정 시간 후 재시도
    Throttle(Duration),
    /// 큐에 추가
    Queue,
}

// ============================================================================
// Resource Monitor
// ============================================================================

/// 리소스 모니터
pub struct ResourceMonitor {
    /// 데이터베이스별 리소스 사용량
    usage: RwLock<HashMap<DatabaseId, Arc<ResourceUsage>>>,
    /// 모니터링 주기
    interval: Duration,
    /// 종료 신호
    shutdown_tx: Option<watch::Sender<bool>>,
}

impl ResourceMonitor {
    /// 새 모니터 생성
    pub fn new(interval: Duration) -> Self {
        Self {
            usage: RwLock::new(HashMap::new()),
            interval,
            shutdown_tx: None,
        }
    }

    /// 데이터베이스 등록
    pub fn register(&self, db_id: DatabaseId) -> Arc<ResourceUsage> {
        let mut usage = self.usage.write();
        let resource_usage = Arc::new(ResourceUsage::new());
        usage.insert(db_id, resource_usage.clone());
        resource_usage
    }

    /// 데이터베이스 해제
    pub fn unregister(&self, db_id: &DatabaseId) {
        let mut usage = self.usage.write();
        usage.remove(db_id);
    }

    /// 사용량 조회
    pub fn get_usage(&self, db_id: &DatabaseId) -> Option<Arc<ResourceUsage>> {
        let usage = self.usage.read();
        usage.get(db_id).cloned()
    }

    /// 모든 사용량 조회
    pub fn get_all_usage(&self) -> HashMap<DatabaseId, ResourceUsageSnapshot> {
        let usage = self.usage.read();
        usage.iter().map(|(id, u)| (*id, u.snapshot())).collect()
    }

    /// 모니터링 주기
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// 백그라운드 모니터링 시작
    pub fn start_background_monitoring(&mut self) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        self.shutdown_tx = Some(tx);
        rx
    }

    /// 모니터링 종료
    pub fn stop(&self) {
        if let Some(ref tx) = self.shutdown_tx {
            let _ = tx.send(true);
        }
    }
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new(Duration::from_secs(10))
    }
}

// ============================================================================
// Resource Enforcer
// ============================================================================

/// 리소스 제한 적용기
pub struct ResourceEnforcer {
    /// 데이터베이스별 리소스 제한
    limits: RwLock<HashMap<DatabaseId, ResourceLimits>>,
    /// 리소스 모니터
    monitor: Arc<ResourceMonitor>,
}

impl ResourceEnforcer {
    /// 새 제한 적용기 생성
    pub fn new(monitor: Arc<ResourceMonitor>) -> Self {
        Self {
            limits: RwLock::new(HashMap::new()),
            monitor,
        }
    }

    /// 리소스 제한 설정
    pub fn set_limits(&self, db_id: DatabaseId, limits: ResourceLimits) {
        let mut l = self.limits.write();
        l.insert(db_id, limits);
    }

    /// 리소스 제한 조회
    pub fn get_limits(&self, db_id: &DatabaseId) -> Option<ResourceLimits> {
        let limits = self.limits.read();
        limits.get(db_id).cloned()
    }

    /// 리소스 제한 해제
    pub fn remove_limits(&self, db_id: &DatabaseId) {
        let mut limits = self.limits.write();
        limits.remove(db_id);
    }

    /// 연결 허용 여부
    pub fn can_accept_connection(&self, db_id: &DatabaseId) -> bool {
        let limits = self.limits.read();
        let Some(limit) = limits.get(db_id) else {
            return true;
        };

        let Some(usage) = self.monitor.get_usage(db_id) else {
            return true;
        };

        let current = usage.current_connections.load(Ordering::SeqCst);
        current < limit.max_connections
    }

    /// 트랜잭션 시작 가능 여부
    pub fn can_start_transaction(&self, db_id: &DatabaseId) -> bool {
        let limits = self.limits.read();
        let Some(limit) = limits.get(db_id) else {
            return true;
        };

        let Some(usage) = self.monitor.get_usage(db_id) else {
            return true;
        };

        let current = usage.current_transactions.load(Ordering::SeqCst);
        current < limit.max_transactions
    }

    /// 쿼리 실행 가능 여부
    pub fn can_execute_query(&self, db_id: &DatabaseId) -> bool {
        let limits = self.limits.read();
        let Some(limit) = limits.get(db_id) else {
            return true;
        };

        let Some(usage) = self.monitor.get_usage(db_id) else {
            return true;
        };

        let qpm = usage.queries_per_minute();
        qpm < limit.max_queries_per_minute
    }

    /// 리소스 타입별 체크
    pub fn check_resource(&self, db_id: &DatabaseId, resource: ResourceType) -> ResourceAction {
        let limits = self.limits.read();
        let Some(limit) = limits.get(db_id) else {
            return ResourceAction::Allow;
        };

        let Some(usage) = self.monitor.get_usage(db_id) else {
            return ResourceAction::Allow;
        };

        match resource {
            ResourceType::Storage => {
                let current = usage.current_storage.load(Ordering::SeqCst);
                if current >= limit.max_storage {
                    ResourceAction::Deny
                } else {
                    ResourceAction::Allow
                }
            }
            ResourceType::Connections => {
                let current = usage.current_connections.load(Ordering::SeqCst);
                if current >= limit.max_connections {
                    ResourceAction::Queue
                } else {
                    ResourceAction::Allow
                }
            }
            ResourceType::Transactions => {
                let current = usage.current_transactions.load(Ordering::SeqCst);
                if current >= limit.max_transactions {
                    ResourceAction::Queue
                } else {
                    ResourceAction::Allow
                }
            }
            ResourceType::QueriesPerMinute => {
                let qpm = usage.queries_per_minute();
                if qpm >= limit.max_queries_per_minute {
                    // 다음 분까지 대기
                    ResourceAction::Throttle(Duration::from_secs(60))
                } else if qpm as f64 >= limit.max_queries_per_minute as f64 * 0.9 {
                    ResourceAction::Throttle(Duration::from_millis(10))
                } else {
                    ResourceAction::Allow
                }
            }
        }
    }

    /// 리소스 초과 처리
    pub fn handle_limit_exceeded(
        &self,
        db_id: &DatabaseId,
        resource: ResourceType,
    ) -> ResourceAction {
        self.check_resource(db_id, resource)
    }

    /// 사용률 계산
    pub fn get_utilization(&self, db_id: &DatabaseId) -> Option<ResourceUtilization> {
        let limits = self.limits.read();
        let limit = limits.get(db_id)?;
        let usage = self.monitor.get_usage(db_id)?;
        let snapshot = usage.snapshot();

        Some(ResourceUtilization {
            storage: snapshot.current_storage as f64 / limit.max_storage as f64,
            connections: snapshot.current_connections as f64 / limit.max_connections as f64,
            transactions: snapshot.current_transactions as f64 / limit.max_transactions as f64,
            queries_per_minute: snapshot.queries_per_minute as f64
                / limit.max_queries_per_minute as f64,
        })
    }
}

/// 리소스 사용률
#[derive(Debug, Clone)]
pub struct ResourceUtilization {
    pub storage: f64,
    pub connections: f64,
    pub transactions: f64,
    pub queries_per_minute: f64,
}
