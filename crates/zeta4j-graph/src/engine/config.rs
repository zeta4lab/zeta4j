// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GraphEngine 설정 및 통계 타입 정의

use crate::core::MemoryReporter;
use crate::storage::memory::limiter::MemoryLimiterConfig;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// 스토리지 유형
///
/// v1.9.1: Memory 타입 제거 - Disk만 지원
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StorageType {
    Disk,
}

/// 그래프 엔진 설정
#[derive(Debug, Clone)]
pub struct GraphEngineConfig {
    /// 노드 스토리지 초기 용량
    pub node_capacity: usize,

    /// 엣지 스토리지 초기 용량
    pub edge_capacity: usize,

    /// 배치 연산 크기
    pub batch_size: usize,

    /// 참조 무결성 검사 활성화
    pub enforce_referential_integrity: bool,

    /// 쿼리 캐시 크기
    pub query_cache_size: usize,

    /// 트랜잭션 지원 활성화
    pub enable_transaction_support: bool,

    /// WAL 파일 경로
    pub wal_path: String,

    /// 스토리지 유형
    pub storage_type: StorageType,

    /// 데이터 파일 경로 (Disk 모드용)
    pub data_path: String,

    /// 메모리 제한 설정 (None이면 무제한)
    pub memory_limit: Option<MemoryLimiterConfig>,

    /// 락 획득 타임아웃 (밀리초)
    pub lock_timeout_ms: u64,

    /// WAL 최대 세그먼트 크기 (바이트)
    pub wal_max_segment_size: u64,

    /// WAL 최대 세그먼트 수
    pub wal_max_segments: usize,

    /// WAL 플러시 간격 (레코드 수)
    pub wal_flush_interval: usize,

    /// MVCC 쓰기 세트 최대 보관 개수
    pub mvcc_max_write_sets: usize,

    /// MVCC 스냅샷 캐시 크기
    pub mvcc_snapshot_cache_size: usize,

    /// Compaction 최소 레코드 수 (이 수 이상일 때 압축 시작)
    pub compaction_min_records: usize,

    /// Compaction 최대 레코드 수 (한 번에 처리할 최대 수)
    pub compaction_max_records: usize,

    /// 인덱스 자동 생성 활성화
    pub index_auto_create: bool,

    /// 기본 인덱스 타입 (btree, hash)
    pub index_default_type: String,

    /// 버퍼 풀 크기 (페이지 수, 각 페이지 4KB)
    /// 기본값: 10000 (40MB)
    pub buffer_pool_size: usize,
}

impl Default for GraphEngineConfig {
    fn default() -> Self {
        // 기본 데이터 경로: ~/.zeta4j/databases
        let default_data_path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".zeta4j")
            .join("databases");
        let data_path_str = default_data_path.to_string_lossy().to_string();
        let wal_path_str = default_data_path
            .join("wal.log")
            .to_string_lossy()
            .to_string();

        Self {
            node_capacity: 10_000,
            edge_capacity: 50_000,
            batch_size: 1_000,
            enforce_referential_integrity: true,
            query_cache_size: 1_000,
            enable_transaction_support: true,
            wal_path: wal_path_str,
            storage_type: StorageType::Disk,
            data_path: data_path_str,
            memory_limit: None,
            lock_timeout_ms: 1000,                  // 기본값: 1초
            wal_max_segment_size: 64 * 1024 * 1024, // 64MB
            wal_max_segments: 10,
            wal_flush_interval: 100,
            mvcc_max_write_sets: 100,
            mvcc_snapshot_cache_size: 1000,
            compaction_min_records: 1000,
            compaction_max_records: 100000,
            index_auto_create: true,
            index_default_type: "btree".to_string(),
            buffer_pool_size: 10000, // 10000 pages = 40MB
        }
    }
}

impl GraphEngineConfig {
    /// 메모리 제한 설정 (MB 단위)
    pub fn with_memory_limit_mb(mut self, mb: usize) -> Self {
        self.memory_limit = Some(MemoryLimiterConfig::default().with_max_memory_mb(mb));
        self
    }

    /// 메모리 제한 설정 (GB 단위)
    pub fn with_memory_limit_gb(mut self, gb: usize) -> Self {
        self.memory_limit = Some(MemoryLimiterConfig::default().with_max_memory_gb(gb));
        self
    }

    /// 메모리 제한 설정 (상세)
    pub fn with_memory_limit(mut self, config: MemoryLimiterConfig) -> Self {
        self.memory_limit = Some(config);
        self
    }

    /// 메모리 제한 비활성화 (무제한)
    pub fn without_memory_limit(mut self) -> Self {
        self.memory_limit = None;
        self
    }

    /// 락 획득 타임아웃 설정 (밀리초)
    pub fn with_lock_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.lock_timeout_ms = timeout_ms;
        self
    }
}

/// 엔진 통계 (일반 버전 - 스냅샷용)
#[derive(Debug, Default, Clone)]
pub struct EngineStats {
    /// 노드 연산 통계
    pub node_operations: u64,

    /// 엣지 연산 통계
    pub edge_operations: u64,

    /// 쿼리 실행 횟수
    pub query_executions: u64,

    /// 배치 연산 횟수
    pub batch_operations: u64,

    /// 트랜잭션 횟수
    pub transaction_count: u64,

    /// 평균 쿼리 실행 시간 (나노초)
    pub avg_query_time_ns: u64,

    /// 메모리 사용량 (바이트)
    pub memory_usage: usize,
}

impl MemoryReporter for EngineStats {
    fn memory_usage(&self) -> usize {
        self.memory_usage
    }
}

/// 엔진 통계 (Lock-free 버전)
///
/// RwLock 대신 AtomicU64를 사용하여 락 경합 없이 통계를 업데이트합니다.
/// 조회 연산마다 락을 획득하던 병목을 제거하여 성능을 크게 개선합니다.
///
/// # 성능
/// - 기존: 매 노드 조회마다 RwLock.write() 호출 → 심각한 경합
/// - 개선: AtomicU64 fetch_add → 락 없이 O(1) 업데이트
#[derive(Debug)]
pub struct AtomicEngineStats {
    /// 노드 연산 통계
    pub node_operations: AtomicU64,

    /// 엣지 연산 통계
    pub edge_operations: AtomicU64,

    /// 쿼리 실행 횟수
    pub query_executions: AtomicU64,

    /// 배치 연산 횟수
    pub batch_operations: AtomicU64,

    /// 트랜잭션 횟수
    pub transaction_count: AtomicU64,

    /// 총 쿼리 실행 시간 (나노초) - 평균 계산용
    pub total_query_time_ns: AtomicU64,

    /// 메모리 사용량 (바이트)
    pub memory_usage: AtomicUsize,
}

impl Default for AtomicEngineStats {
    fn default() -> Self {
        Self {
            node_operations: AtomicU64::new(0),
            edge_operations: AtomicU64::new(0),
            query_executions: AtomicU64::new(0),
            batch_operations: AtomicU64::new(0),
            transaction_count: AtomicU64::new(0),
            total_query_time_ns: AtomicU64::new(0),
            memory_usage: AtomicUsize::new(0),
        }
    }
}

impl AtomicEngineStats {
    /// 새 AtomicEngineStats 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 쿼리 실행 통계 업데이트 (lock-free)
    #[inline]
    pub fn record_query(&self, elapsed_ns: u64) {
        self.query_executions.fetch_add(1, Ordering::Relaxed);
        self.total_query_time_ns
            .fetch_add(elapsed_ns, Ordering::Relaxed);
    }

    /// 노드 연산 카운트 증가 (lock-free)
    #[inline]
    pub fn record_node_operation(&self) {
        self.node_operations.fetch_add(1, Ordering::Relaxed);
    }

    /// 엣지 연산 카운트 증가 (lock-free)
    #[inline]
    pub fn record_edge_operation(&self) {
        self.edge_operations.fetch_add(1, Ordering::Relaxed);
    }

    /// 배치 연산 카운트 증가 (lock-free)
    #[inline]
    pub fn record_batch_operation(&self) {
        self.batch_operations.fetch_add(1, Ordering::Relaxed);
    }

    /// 트랜잭션 카운트 증가 (lock-free)
    #[inline]
    pub fn record_transaction(&self) {
        self.transaction_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 메모리 사용량 업데이트 (lock-free)
    #[inline]
    pub fn set_memory_usage(&self, bytes: usize) {
        self.memory_usage.store(bytes, Ordering::Relaxed);
    }

    /// 스냅샷 반환 (EngineStats로 변환)
    pub fn snapshot(&self) -> EngineStats {
        let query_executions = self.query_executions.load(Ordering::Relaxed);
        let total_time = self.total_query_time_ns.load(Ordering::Relaxed);
        let avg_query_time_ns = total_time.checked_div(query_executions).unwrap_or(0);

        EngineStats {
            node_operations: self.node_operations.load(Ordering::Relaxed),
            edge_operations: self.edge_operations.load(Ordering::Relaxed),
            query_executions,
            batch_operations: self.batch_operations.load(Ordering::Relaxed),
            transaction_count: self.transaction_count.load(Ordering::Relaxed),
            avg_query_time_ns,
            memory_usage: self.memory_usage.load(Ordering::Relaxed),
        }
    }

    /// 통계 리셋
    pub fn reset(&self) {
        self.node_operations.store(0, Ordering::Relaxed);
        self.edge_operations.store(0, Ordering::Relaxed);
        self.query_executions.store(0, Ordering::Relaxed);
        self.batch_operations.store(0, Ordering::Relaxed);
        self.transaction_count.store(0, Ordering::Relaxed);
        self.total_query_time_ns.store(0, Ordering::Relaxed);
        self.memory_usage.store(0, Ordering::Relaxed);
    }
}
