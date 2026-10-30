//! 데이터베이스 통계 타입.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// 데이터베이스 통계
#[derive(Debug)]
pub struct DatabaseStatistics {
    /// 노드 수
    pub node_count: AtomicU64,
    /// 관계 수
    pub relationship_count: AtomicU64,
    /// 저장 크기 (바이트)
    pub store_size: AtomicU64,
    /// 총 트랜잭션 수
    pub total_tx_count: AtomicU64,
    /// 활성 트랜잭션 수
    pub active_tx_count: AtomicU32,
    /// 쿼리 수
    pub query_count: AtomicU64,
    /// 마지막 쿼리 시간 (Unix timestamp)
    pub last_query_time: AtomicU64,
}

impl DatabaseStatistics {
    /// 새 통계 객체 생성
    pub fn new() -> Self {
        Self {
            node_count: AtomicU64::new(0),
            relationship_count: AtomicU64::new(0),
            store_size: AtomicU64::new(0),
            total_tx_count: AtomicU64::new(0),
            active_tx_count: AtomicU32::new(0),
            query_count: AtomicU64::new(0),
            last_query_time: AtomicU64::new(0),
        }
    }

    /// 노드 수 증가
    pub fn increment_nodes(&self, count: u64) {
        self.node_count.fetch_add(count, Ordering::Relaxed);
    }

    /// 관계 수 증가
    pub fn increment_relationships(&self, count: u64) {
        self.relationship_count.fetch_add(count, Ordering::Relaxed);
    }

    /// 트랜잭션 시작
    pub fn start_transaction(&self) {
        self.active_tx_count.fetch_add(1, Ordering::Relaxed);
        self.total_tx_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 트랜잭션 종료
    pub fn end_transaction(&self) {
        self.active_tx_count.fetch_sub(1, Ordering::Relaxed);
    }

    /// 쿼리 실행 기록
    pub fn record_query(&self) {
        self.query_count.fetch_add(1, Ordering::Relaxed);
        let now = Utc::now().timestamp() as u64;
        self.last_query_time.store(now, Ordering::Relaxed);
    }

    /// 스냅샷 생성
    pub fn snapshot(&self) -> DatabaseStatisticsSnapshot {
        DatabaseStatisticsSnapshot {
            node_count: self.node_count.load(Ordering::Relaxed),
            relationship_count: self.relationship_count.load(Ordering::Relaxed),
            store_size: self.store_size.load(Ordering::Relaxed),
            total_tx_count: self.total_tx_count.load(Ordering::Relaxed),
            active_tx_count: self.active_tx_count.load(Ordering::Relaxed),
            query_count: self.query_count.load(Ordering::Relaxed),
            last_query_time: self.last_query_time.load(Ordering::Relaxed),
        }
    }
}

impl Default for DatabaseStatistics {
    fn default() -> Self {
        Self::new()
    }
}

/// 통계 스냅샷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseStatisticsSnapshot {
    pub node_count: u64,
    pub relationship_count: u64,
    pub store_size: u64,
    pub total_tx_count: u64,
    pub active_tx_count: u32,
    pub query_count: u64,
    pub last_query_time: u64,
}
