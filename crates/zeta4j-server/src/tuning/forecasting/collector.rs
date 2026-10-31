// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 메트릭 수집기
//!
//! 데이터베이스 워크로드 메트릭을 수집하고 관리합니다.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tokio::time::interval;

use super::storage::{StorageConfig, TimeSeriesStorage};
use zeta4j_graph::error::Result;

/// 수집할 메트릭 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MetricType {
    /// 초당 쿼리 수
    QueriesPerSecond,
    /// 평균 응답 시간 (ms)
    AvgResponseTime,
    /// P50 응답 시간 (ms)
    P50ResponseTime,
    /// P95 응답 시간 (ms)
    P95ResponseTime,
    /// P99 응답 시간 (ms)
    P99ResponseTime,
    /// 활성 연결 수
    ActiveConnections,
    /// 활성 트랜잭션 수
    ActiveTransactions,
    /// 캐시 히트율 (0.0 ~ 1.0)
    CacheHitRate,
    /// 인덱스 사용률 (0.0 ~ 1.0)
    IndexUsageRate,
    /// 디스크 읽기 (bytes/s)
    DiskReadBytes,
    /// 디스크 쓰기 (bytes/s)
    DiskWriteBytes,
    /// 메모리 사용량 (bytes)
    MemoryUsage,
    /// 힙 메모리 사용량 (bytes)
    HeapMemoryUsage,
    /// 노드 수
    NodeCount,
    /// 엣지 수
    EdgeCount,
    /// 락 대기 시간 (ms)
    LockWaitTime,
    /// 데드락 탐지 횟수
    DeadlockCount,
    /// WAL 쓰기량 (bytes/s)
    WalWriteBytes,
    /// 체크포인트 간격 (초)
    CheckpointInterval,
}

impl MetricType {
    /// 모든 메트릭 타입 반환
    pub fn all() -> &'static [MetricType] {
        &[
            MetricType::QueriesPerSecond,
            MetricType::AvgResponseTime,
            MetricType::P50ResponseTime,
            MetricType::P95ResponseTime,
            MetricType::P99ResponseTime,
            MetricType::ActiveConnections,
            MetricType::ActiveTransactions,
            MetricType::CacheHitRate,
            MetricType::IndexUsageRate,
            MetricType::DiskReadBytes,
            MetricType::DiskWriteBytes,
            MetricType::MemoryUsage,
            MetricType::HeapMemoryUsage,
            MetricType::NodeCount,
            MetricType::EdgeCount,
            MetricType::LockWaitTime,
            MetricType::DeadlockCount,
            MetricType::WalWriteBytes,
            MetricType::CheckpointInterval,
        ]
    }

    /// 메트릭 이름 반환
    pub fn name(&self) -> &'static str {
        match self {
            MetricType::QueriesPerSecond => "queries_per_second",
            MetricType::AvgResponseTime => "avg_response_time_ms",
            MetricType::P50ResponseTime => "p50_response_time_ms",
            MetricType::P95ResponseTime => "p95_response_time_ms",
            MetricType::P99ResponseTime => "p99_response_time_ms",
            MetricType::ActiveConnections => "active_connections",
            MetricType::ActiveTransactions => "active_transactions",
            MetricType::CacheHitRate => "cache_hit_rate",
            MetricType::IndexUsageRate => "index_usage_rate",
            MetricType::DiskReadBytes => "disk_read_bytes_per_sec",
            MetricType::DiskWriteBytes => "disk_write_bytes_per_sec",
            MetricType::MemoryUsage => "memory_usage_bytes",
            MetricType::HeapMemoryUsage => "heap_memory_usage_bytes",
            MetricType::NodeCount => "node_count",
            MetricType::EdgeCount => "edge_count",
            MetricType::LockWaitTime => "lock_wait_time_ms",
            MetricType::DeadlockCount => "deadlock_count",
            MetricType::WalWriteBytes => "wal_write_bytes_per_sec",
            MetricType::CheckpointInterval => "checkpoint_interval_sec",
        }
    }

    /// 메트릭 단위 반환
    pub fn unit(&self) -> &'static str {
        match self {
            MetricType::QueriesPerSecond => "qps",
            MetricType::AvgResponseTime
            | MetricType::P50ResponseTime
            | MetricType::P95ResponseTime
            | MetricType::P99ResponseTime
            | MetricType::LockWaitTime => "ms",
            MetricType::ActiveConnections
            | MetricType::ActiveTransactions
            | MetricType::NodeCount
            | MetricType::EdgeCount
            | MetricType::DeadlockCount => "count",
            MetricType::CacheHitRate | MetricType::IndexUsageRate => "ratio",
            MetricType::DiskReadBytes | MetricType::DiskWriteBytes | MetricType::WalWriteBytes => {
                "bytes/s"
            }
            MetricType::MemoryUsage | MetricType::HeapMemoryUsage => "bytes",
            MetricType::CheckpointInterval => "sec",
        }
    }

    /// 게이지 메트릭인지 확인 (순간 값)
    pub fn is_gauge(&self) -> bool {
        matches!(
            self,
            MetricType::ActiveConnections
                | MetricType::ActiveTransactions
                | MetricType::CacheHitRate
                | MetricType::IndexUsageRate
                | MetricType::MemoryUsage
                | MetricType::HeapMemoryUsage
                | MetricType::NodeCount
                | MetricType::EdgeCount
        )
    }

    /// 카운터 메트릭인지 확인 (누적 값)
    pub fn is_counter(&self) -> bool {
        matches!(self, MetricType::DeadlockCount)
    }
}

/// 시계열 데이터 포인트
#[derive(Debug, Clone)]
pub struct DataPoint {
    /// 타임스탬프
    pub timestamp: DateTime<Utc>,
    /// 메트릭 타입
    pub metric_type: MetricType,
    /// 값
    pub value: f64,
    /// 레이블 (선택적 메타데이터)
    pub labels: HashMap<String, String>,
}

impl DataPoint {
    /// 새 데이터 포인트 생성
    pub fn new(metric_type: MetricType, value: f64) -> Self {
        Self {
            timestamp: Utc::now(),
            metric_type,
            value,
            labels: HashMap::new(),
        }
    }

    /// 타임스탬프 지정하여 생성
    pub fn with_timestamp(metric_type: MetricType, value: f64, timestamp: DateTime<Utc>) -> Self {
        Self {
            timestamp,
            metric_type,
            value,
            labels: HashMap::new(),
        }
    }

    /// 레이블 추가
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// 여러 레이블 추가
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels.extend(labels);
        self
    }
}

/// 집계 방식
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Aggregation {
    /// 집계 없음 (원본 데이터)
    None,
    /// 평균
    Avg(Duration),
    /// 최대값
    Max(Duration),
    /// 최소값
    Min(Duration),
    /// 합계
    Sum(Duration),
    /// 카운트
    Count(Duration),
    /// 첫 번째 값
    First(Duration),
    /// 마지막 값
    Last(Duration),
}

impl Aggregation {
    /// 집계 윈도우 크기 반환
    pub fn window(&self) -> Option<Duration> {
        match self {
            Aggregation::None => None,
            Aggregation::Avg(d)
            | Aggregation::Max(d)
            | Aggregation::Min(d)
            | Aggregation::Sum(d)
            | Aggregation::Count(d)
            | Aggregation::First(d)
            | Aggregation::Last(d) => Some(*d),
        }
    }
}

/// 샘플링 설정
#[derive(Debug, Clone)]
pub struct SamplingConfig {
    /// 샘플링 비율 (0.0 ~ 1.0, 1.0 = 모든 데이터)
    pub rate: f64,
    /// 최소 샘플 간격
    pub min_interval: Duration,
    /// 값 변화 임계값 (변화가 이 값 이상일 때만 기록)
    pub change_threshold: f64,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            rate: 1.0,
            min_interval: Duration::from_secs(1),
            change_threshold: 0.0,
        }
    }
}

/// 수집기 설정
#[derive(Debug, Clone)]
pub struct CollectorConfig {
    /// 수집 간격
    pub collection_interval: Duration,
    /// 활성화할 메트릭
    pub enabled_metrics: Vec<MetricType>,
    /// 샘플링 설정
    pub sampling: SamplingConfig,
    /// 저장소 설정
    pub storage: StorageConfig,
    /// 버퍼 크기
    pub buffer_size: usize,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            collection_interval: Duration::from_secs(60),
            enabled_metrics: MetricType::all().to_vec(),
            sampling: SamplingConfig::default(),
            storage: StorageConfig::default(),
            buffer_size: 1000,
        }
    }
}

/// 메트릭 소스 트레이트
pub trait MetricSource: Send + Sync {
    /// 현재 메트릭 값 수집
    fn collect(&self) -> Vec<DataPoint>;

    /// 지원하는 메트릭 타입
    fn supported_metrics(&self) -> Vec<MetricType>;
}

/// 메트릭 수집기
pub struct MetricCollector {
    /// 설정
    config: CollectorConfig,
    /// 저장소
    storage: Arc<TimeSeriesStorage>,
    /// 메트릭 소스
    sources: RwLock<Vec<Arc<dyn MetricSource>>>,
    /// 마지막 수집 값 (변화 감지용)
    last_values: RwLock<HashMap<MetricType, f64>>,
    /// 실행 상태
    running: Arc<RwLock<bool>>,
    /// 중지 신호 송신자
    stop_tx: RwLock<Option<mpsc::Sender<()>>>,
}

impl MetricCollector {
    /// 새 수집기 생성
    pub fn new(config: CollectorConfig) -> Self {
        let storage = Arc::new(TimeSeriesStorage::new(config.storage.clone()));

        Self {
            config,
            storage,
            sources: RwLock::new(Vec::new()),
            last_values: RwLock::new(HashMap::new()),
            running: Arc::new(RwLock::new(false)),
            stop_tx: RwLock::new(None),
        }
    }

    /// 메트릭 소스 등록
    pub fn register_source(&self, source: Arc<dyn MetricSource>) {
        self.sources.write().push(source);
    }

    /// 수집 시작 (백그라운드 태스크)
    pub async fn start(&self) -> Result<()> {
        if *self.running.read() {
            return Ok(());
        }

        *self.running.write() = true;

        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        *self.stop_tx.write() = Some(stop_tx);

        let storage = Arc::clone(&self.storage);
        let sources = {
            let sources = self.sources.read();
            sources.clone()
        };
        let interval_duration = self.config.collection_interval;
        let running = Arc::clone(&self.running);
        let sampling = self.config.sampling.clone();
        let enabled_metrics: std::collections::HashSet<MetricType> =
            self.config.enabled_metrics.iter().copied().collect();

        tokio::spawn(async move {
            let mut ticker = interval(interval_duration);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if !*running.read() {
                            break;
                        }

                        // 모든 소스에서 메트릭 수집
                        for source in &sources {
                            let points = source.collect();
                            for point in points {
                                // 활성화된 메트릭만 처리
                                if !enabled_metrics.contains(&point.metric_type) {
                                    continue;
                                }

                                // 샘플링 적용
                                if sampling.rate < 1.0 {
                                    let r: f64 = rand::random();
                                    if r > sampling.rate {
                                        continue;
                                    }
                                }

                                // 저장
                                storage.insert(point);
                            }
                        }

                        // 보관 정책 적용
                        storage.apply_retention();
                    }
                    _ = stop_rx.recv() => {
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// 수집 중지
    pub async fn stop(&self) {
        *self.running.write() = false;

        // Take sender while holding lock, then send outside lock
        let stop_tx = self.stop_tx.write().take();
        if let Some(tx) = stop_tx {
            let _ = tx.send(()).await;
        }
    }

    /// 실행 중인지 확인
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// 단일 수집 수행 (동기)
    pub fn collect_once(&self) {
        let sources = self.sources.read();
        let enabled_metrics: std::collections::HashSet<MetricType> =
            self.config.enabled_metrics.iter().copied().collect();

        for source in sources.iter() {
            let points = source.collect();
            for point in points {
                if enabled_metrics.contains(&point.metric_type) {
                    self.storage.insert(point);
                }
            }
        }
    }

    /// 수동으로 데이터 포인트 추가
    pub fn insert(&self, point: DataPoint) {
        self.storage.insert(point);
    }

    /// 특정 메트릭 조회
    pub fn query(
        &self,
        metric: MetricType,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        aggregation: Aggregation,
    ) -> Vec<DataPoint> {
        self.storage.query(metric, start, end, aggregation)
    }

    /// 최근 N개 데이터 포인트 조회
    pub fn query_last(&self, metric: MetricType, count: usize) -> Vec<DataPoint> {
        self.storage.query_last(metric, count)
    }

    /// 특정 메트릭의 최신 값 조회
    pub fn get_latest(&self, metric: MetricType) -> Option<DataPoint> {
        self.storage.get_latest(metric)
    }

    /// 저장소 참조 반환
    pub fn storage(&self) -> &TimeSeriesStorage {
        &self.storage
    }

    /// 통계 정보 반환
    pub fn stats(&self) -> CollectorStats {
        let sources_count = self.sources.read().len();
        let storage_stats = self.storage.stats();

        CollectorStats {
            sources_count,
            total_points: storage_stats.total_points,
            memory_bytes: storage_stats.memory_bytes,
            metrics_count: storage_stats.metrics_count,
            oldest_timestamp: storage_stats.oldest_timestamp,
            newest_timestamp: storage_stats.newest_timestamp,
        }
    }
}

/// 수집기 통계
#[derive(Debug, Clone)]
pub struct CollectorStats {
    /// 등록된 소스 수
    pub sources_count: usize,
    /// 총 데이터 포인트 수
    pub total_points: usize,
    /// 메모리 사용량 (추정)
    pub memory_bytes: usize,
    /// 메트릭 종류 수
    pub metrics_count: usize,
    /// 가장 오래된 데이터 시간
    pub oldest_timestamp: Option<DateTime<Utc>>,
    /// 가장 최근 데이터 시간
    pub newest_timestamp: Option<DateTime<Utc>>,
}

/// 기본 시스템 메트릭 소스
pub struct SystemMetricSource {
    /// 마지막 수집 시간
    last_collection: RwLock<Option<std::time::Instant>>,
}

impl SystemMetricSource {
    pub fn new() -> Self {
        Self {
            last_collection: RwLock::new(None),
        }
    }
}

impl Default for SystemMetricSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for SystemMetricSource {
    #[allow(unused_mut)]
    fn collect(&self) -> Vec<DataPoint> {
        #[allow(unused_variables)]
        let now = Utc::now();
        let mut points = Vec::new();

        // 메모리 사용량 (추정)
        // 실제 구현에서는 시스템 API 사용
        #[cfg(target_os = "linux")]
        {
            if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
                // 간단한 파싱
                for line in meminfo.lines() {
                    if line.starts_with("MemAvailable:")
                        && let Some(value) = line.split_whitespace().nth(1)
                        && let Ok(kb) = value.parse::<u64>()
                    {
                        points.push(DataPoint::with_timestamp(
                            MetricType::MemoryUsage,
                            (kb * 1024) as f64,
                            now,
                        ));
                    }
                }
            }
        }

        *self.last_collection.write() = Some(std::time::Instant::now());
        points
    }

    fn supported_metrics(&self) -> Vec<MetricType> {
        vec![MetricType::MemoryUsage]
    }
}
