//! 시계열 저장소
//!
//! 메트릭 데이터를 효율적으로 저장하고 조회합니다.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;

use super::collector::{Aggregation, DataPoint, MetricType};

/// 보관 정책
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    /// 원본 데이터 보관 기간
    pub raw_retention: Duration,
    /// 다운샘플링 규칙
    pub downsample_rules: Vec<DownsampleRule>,
    /// 최대 데이터 포인트 수 (메모리 제한)
    pub max_points_per_metric: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            // 원본: 1일
            raw_retention: Duration::from_secs(24 * 60 * 60),
            downsample_rules: vec![
                // 1일 이후: 5분 단위로 집계, 7일 보관
                DownsampleRule {
                    after: Duration::from_secs(24 * 60 * 60),
                    interval: Duration::from_secs(5 * 60),
                    retention: Duration::from_secs(7 * 24 * 60 * 60),
                    aggregation: AggregationType::Avg,
                },
                // 7일 이후: 1시간 단위로 집계, 30일 보관
                DownsampleRule {
                    after: Duration::from_secs(7 * 24 * 60 * 60),
                    interval: Duration::from_secs(60 * 60),
                    retention: Duration::from_secs(30 * 24 * 60 * 60),
                    aggregation: AggregationType::Avg,
                },
            ],
            max_points_per_metric: 100_000,
        }
    }
}

/// 다운샘플링 규칙
#[derive(Debug, Clone)]
pub struct DownsampleRule {
    /// 이 시간 이후의 데이터에 적용
    pub after: Duration,
    /// 집계 간격
    pub interval: Duration,
    /// 다운샘플링된 데이터 보관 기간
    pub retention: Duration,
    /// 집계 방식
    pub aggregation: AggregationType,
}

/// 집계 유형
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AggregationType {
    Avg,
    Max,
    Min,
    Sum,
    First,
    Last,
}

/// 저장소 설정
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// 보관 정책
    pub retention: RetentionPolicy,
    /// 메모리 제한 (bytes)
    pub memory_limit: usize,
    /// 자동 정리 간격
    pub cleanup_interval: Duration,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            retention: RetentionPolicy::default(),
            memory_limit: 100 * 1024 * 1024,            // 100MB
            cleanup_interval: Duration::from_secs(300), // 5분
        }
    }
}

/// 시계열 데이터 저장소
pub struct TimeSeriesStorage {
    /// 설정
    config: StorageConfig,
    /// 메트릭별 원본 데이터 (시간순 정렬)
    raw_data: RwLock<HashMap<MetricType, VecDeque<DataPoint>>>,
    /// 다운샘플링된 데이터 (규칙 인덱스 -> 메트릭 -> 데이터)
    downsampled_data: RwLock<Vec<HashMap<MetricType, VecDeque<DataPoint>>>>,
    /// 마지막 정리 시간
    last_cleanup: RwLock<Option<std::time::Instant>>,
}

impl TimeSeriesStorage {
    /// 새 저장소 생성
    pub fn new(config: StorageConfig) -> Self {
        let rules_count = config.retention.downsample_rules.len();
        let downsampled = (0..rules_count).map(|_| HashMap::new()).collect();

        Self {
            config,
            raw_data: RwLock::new(HashMap::new()),
            downsampled_data: RwLock::new(downsampled),
            last_cleanup: RwLock::new(None),
        }
    }

    /// 데이터 포인트 삽입
    pub fn insert(&self, point: DataPoint) {
        let mut raw = self.raw_data.write();
        let queue = raw.entry(point.metric_type).or_default();

        // 시간순 삽입
        let pos = queue
            .iter()
            .rposition(|p| p.timestamp <= point.timestamp)
            .map(|i| i + 1)
            .unwrap_or(0);

        queue.insert(pos, point);

        // 최대 크기 제한
        while queue.len() > self.config.retention.max_points_per_metric {
            queue.pop_front();
        }
    }

    /// 데이터 조회
    pub fn query(
        &self,
        metric: MetricType,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        aggregation: Aggregation,
    ) -> Vec<DataPoint> {
        // 먼저 원본 데이터에서 조회
        let raw = self.raw_data.read();
        let mut results = Vec::new();

        if let Some(queue) = raw.get(&metric) {
            for point in queue.iter() {
                if point.timestamp >= start && point.timestamp <= end {
                    results.push(point.clone());
                }
            }
        }

        // 다운샘플링된 데이터에서 추가 조회 (오래된 범위)
        let downsampled = self.downsampled_data.read();
        for level_data in downsampled.iter() {
            if let Some(queue) = level_data.get(&metric) {
                for point in queue.iter() {
                    if point.timestamp >= start && point.timestamp <= end {
                        // 원본에 없는 범위만 추가
                        if !results.iter().any(|p| p.timestamp == point.timestamp) {
                            results.push(point.clone());
                        }
                    }
                }
            }
        }

        // 시간순 정렬
        results.sort_by_key(|a| a.timestamp);

        // 집계 적용
        match aggregation {
            Aggregation::None => results,
            _ => self.aggregate_points(&results, aggregation, metric),
        }
    }

    /// 최근 N개 데이터 조회
    pub fn query_last(&self, metric: MetricType, count: usize) -> Vec<DataPoint> {
        let raw = self.raw_data.read();

        if let Some(queue) = raw.get(&metric) {
            let skip = queue.len().saturating_sub(count);
            queue.iter().skip(skip).cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// 최신 데이터 포인트 조회
    pub fn get_latest(&self, metric: MetricType) -> Option<DataPoint> {
        let raw = self.raw_data.read();
        raw.get(&metric).and_then(|q| q.back().cloned())
    }

    /// 보관 정책 적용 (오래된 데이터 정리 + 다운샘플링)
    pub fn apply_retention(&self) {
        let now = std::time::Instant::now();

        // 너무 자주 실행 방지
        {
            let last = self.last_cleanup.read();
            if let Some(last_time) = *last
                && now.duration_since(last_time) < self.config.cleanup_interval
            {
                return;
            }
        }

        *self.last_cleanup.write() = Some(now);

        let utc_now = Utc::now();

        // 1. 원본 데이터 정리 및 다운샘플링
        self.downsample_and_cleanup(utc_now);

        // 2. 다운샘플링된 데이터 정리
        self.cleanup_downsampled(utc_now);
    }

    /// 다운샘플링 및 원본 정리
    fn downsample_and_cleanup(&self, now: DateTime<Utc>) {
        let raw_cutoff = now
            - chrono::Duration::from_std(self.config.retention.raw_retention)
                .unwrap_or_else(|_| chrono::Duration::days(1));

        let mut raw = self.raw_data.write();
        let mut downsampled = self.downsampled_data.write();

        for (metric, queue) in raw.iter_mut() {
            // 다운샘플링할 포인트 수집
            let old_points: Vec<_> = queue
                .iter()
                .filter(|p| p.timestamp < raw_cutoff)
                .cloned()
                .collect();

            if old_points.is_empty() {
                continue;
            }

            // 다운샘플링 수행
            for (rule_idx, rule) in self.config.retention.downsample_rules.iter().enumerate() {
                let downsampled_points =
                    self.downsample_points(&old_points, rule.interval, rule.aggregation, *metric);

                if let Some(level_data) = downsampled.get_mut(rule_idx) {
                    let ds_queue = level_data.entry(*metric).or_insert_with(VecDeque::new);
                    for point in downsampled_points {
                        ds_queue.push_back(point);
                    }
                }
            }

            // 원본에서 오래된 데이터 제거
            queue.retain(|p| p.timestamp >= raw_cutoff);
        }
    }

    /// 다운샘플링된 데이터 정리
    fn cleanup_downsampled(&self, now: DateTime<Utc>) {
        let mut downsampled = self.downsampled_data.write();

        for (rule_idx, rule) in self.config.retention.downsample_rules.iter().enumerate() {
            let cutoff = now
                - chrono::Duration::from_std(rule.retention)
                    .unwrap_or_else(|_| chrono::Duration::days(30));

            if let Some(level_data) = downsampled.get_mut(rule_idx) {
                for queue in level_data.values_mut() {
                    queue.retain(|p| p.timestamp >= cutoff);
                }
            }
        }
    }

    /// 포인트 다운샘플링
    fn downsample_points(
        &self,
        points: &[DataPoint],
        interval: Duration,
        agg_type: AggregationType,
        metric: MetricType,
    ) -> Vec<DataPoint> {
        if points.is_empty() {
            return Vec::new();
        }

        // 시간 버킷으로 그룹화
        let interval_secs = interval.as_secs() as i64;
        let mut buckets: BTreeMap<i64, Vec<f64>> = BTreeMap::new();

        for point in points {
            let bucket_key = point.timestamp.timestamp() / interval_secs * interval_secs;
            buckets.entry(bucket_key).or_default().push(point.value);
        }

        // 각 버킷 집계
        buckets
            .into_iter()
            .map(|(bucket_ts, values)| {
                let agg_value = self.aggregate_values(&values, agg_type);
                let timestamp = DateTime::from_timestamp(bucket_ts, 0).unwrap_or_else(Utc::now);

                DataPoint::with_timestamp(metric, agg_value, timestamp)
            })
            .collect()
    }

    /// 값 집계
    fn aggregate_values(&self, values: &[f64], agg_type: AggregationType) -> f64 {
        if values.is_empty() {
            return 0.0;
        }

        match agg_type {
            AggregationType::Avg => values.iter().sum::<f64>() / values.len() as f64,
            AggregationType::Max => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            AggregationType::Min => values.iter().cloned().fold(f64::INFINITY, f64::min),
            AggregationType::Sum => values.iter().sum(),
            AggregationType::First => values[0],
            AggregationType::Last => values[values.len() - 1],
        }
    }

    /// 포인트 집계 (쿼리용)
    fn aggregate_points(
        &self,
        points: &[DataPoint],
        aggregation: Aggregation,
        metric: MetricType,
    ) -> Vec<DataPoint> {
        let window = match aggregation.window() {
            Some(w) => w,
            None => return points.to_vec(),
        };

        let agg_type = match aggregation {
            Aggregation::Avg(_) => AggregationType::Avg,
            Aggregation::Max(_) => AggregationType::Max,
            Aggregation::Min(_) => AggregationType::Min,
            Aggregation::Sum(_) => AggregationType::Sum,
            Aggregation::Count(_) => {
                // Count는 특별 처리
                let interval_secs = window.as_secs() as i64;
                let mut buckets: BTreeMap<i64, usize> = BTreeMap::new();

                for point in points {
                    let bucket_key = point.timestamp.timestamp() / interval_secs * interval_secs;
                    *buckets.entry(bucket_key).or_insert(0) += 1;
                }

                return buckets
                    .into_iter()
                    .map(|(bucket_ts, count)| {
                        let timestamp =
                            DateTime::from_timestamp(bucket_ts, 0).unwrap_or_else(Utc::now);
                        DataPoint::with_timestamp(metric, count as f64, timestamp)
                    })
                    .collect();
            }
            Aggregation::First(_) => AggregationType::First,
            Aggregation::Last(_) => AggregationType::Last,
            Aggregation::None => return points.to_vec(),
        };

        self.downsample_points(points, window, agg_type, metric)
    }

    /// 특정 메트릭 데이터 전체 삭제
    pub fn clear_metric(&self, metric: MetricType) {
        self.raw_data.write().remove(&metric);

        let mut downsampled = self.downsampled_data.write();
        for level_data in downsampled.iter_mut() {
            level_data.remove(&metric);
        }
    }

    /// 모든 데이터 삭제
    pub fn clear(&self) {
        self.raw_data.write().clear();

        let mut downsampled = self.downsampled_data.write();
        for level_data in downsampled.iter_mut() {
            level_data.clear();
        }
    }

    /// 통계 정보
    pub fn stats(&self) -> StorageStats {
        let raw = self.raw_data.read();
        let downsampled = self.downsampled_data.read();

        let mut total_points = 0;
        let mut oldest: Option<DateTime<Utc>> = None;
        let mut newest: Option<DateTime<Utc>> = None;

        for queue in raw.values() {
            total_points += queue.len();
            if let Some(first) = queue.front()
                && oldest.is_none_or(|timestamp| first.timestamp < timestamp)
            {
                oldest = Some(first.timestamp);
            }
            if let Some(last) = queue.back()
                && newest.is_none_or(|timestamp| last.timestamp > timestamp)
            {
                newest = Some(last.timestamp);
            }
        }

        for level_data in downsampled.iter() {
            for queue in level_data.values() {
                total_points += queue.len();
            }
        }

        // 메모리 추정 (DataPoint 크기 * 포인트 수)
        let point_size = std::mem::size_of::<DataPoint>() + 64; // 레이블 등 추가 메모리
        let memory_bytes = total_points * point_size;

        StorageStats {
            total_points,
            memory_bytes,
            metrics_count: raw.len(),
            oldest_timestamp: oldest,
            newest_timestamp: newest,
        }
    }

    /// 원본 데이터 포인트 수
    pub fn raw_count(&self, metric: MetricType) -> usize {
        self.raw_data
            .read()
            .get(&metric)
            .map(|q| q.len())
            .unwrap_or(0)
    }

    /// 다운샘플링된 데이터 포인트 수
    pub fn downsampled_count(&self, metric: MetricType) -> usize {
        let downsampled = self.downsampled_data.read();
        downsampled
            .iter()
            .map(|level| level.get(&metric).map(|q| q.len()).unwrap_or(0))
            .sum()
    }
}

/// 저장소 통계
#[derive(Debug, Clone)]
pub struct StorageStats {
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
