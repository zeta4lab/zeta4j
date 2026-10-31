// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 히스토리 기반 분석 모듈
//!
//! 시계열 데이터 저장 및 트렌드 분석 기능을 제공합니다.
//! - 다양한 메트릭의 시계열 저장
//! - 트렌드 분석 (상승/하락/안정)
//! - 이상치 탐지
//! - 단순 예측

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::monitor::{SystemStatus, TrendDirection};
use super::profiler::QueryStats;

/// 메트릭 유형
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricType {
    /// 메모리 사용률
    MemoryUsage,
    /// 버퍼 풀 히트율
    BufferPoolHitRatio,
    /// 트랜잭션 중단율
    TransactionAbortRate,
    /// 락 경합 수준
    LockContention,
    /// 평균 쿼리 시간 (마이크로초)
    AvgQueryTime,
    /// 느린 쿼리 비율
    SlowQueryRatio,
    /// 인덱스 사용율
    IndexUsageRatio,
    /// 노드 수
    NodeCount,
    /// 엣지 수
    EdgeCount,
    /// 사용자 정의 메트릭
    Custom(u32),
}

impl MetricType {
    /// 메트릭 이름
    pub fn name(&self) -> &'static str {
        match self {
            MetricType::MemoryUsage => "memory_usage",
            MetricType::BufferPoolHitRatio => "buffer_pool_hit_ratio",
            MetricType::TransactionAbortRate => "transaction_abort_rate",
            MetricType::LockContention => "lock_contention",
            MetricType::AvgQueryTime => "avg_query_time",
            MetricType::SlowQueryRatio => "slow_query_ratio",
            MetricType::IndexUsageRatio => "index_usage_ratio",
            MetricType::NodeCount => "node_count",
            MetricType::EdgeCount => "edge_count",
            MetricType::Custom(_) => "custom",
        }
    }

    /// 값이 높을수록 나쁜지 여부
    pub fn higher_is_worse(&self) -> bool {
        match self {
            MetricType::MemoryUsage => true,
            MetricType::BufferPoolHitRatio => false, // 높을수록 좋음
            MetricType::TransactionAbortRate => true,
            MetricType::LockContention => true,
            MetricType::AvgQueryTime => true,
            MetricType::SlowQueryRatio => true,
            MetricType::IndexUsageRatio => false, // 높을수록 좋음
            MetricType::NodeCount => false,
            MetricType::EdgeCount => false,
            MetricType::Custom(_) => true,
        }
    }
}

/// 메트릭 데이터 포인트
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    /// 타임스탬프 (Unix epoch seconds)
    pub timestamp: u64,
    /// 값
    pub value: f64,
}

impl MetricPoint {
    /// 새 메트릭 포인트 생성
    pub fn new(timestamp: u64, value: f64) -> Self {
        Self { timestamp, value }
    }

    /// 현재 시간으로 생성
    pub fn now(value: f64) -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            value,
        }
    }
}

/// 시계열 저장소 설정
#[derive(Debug, Clone)]
pub struct TimeSeriesConfig {
    /// 메트릭별 최대 데이터 포인트 수
    pub max_points_per_metric: usize,
    /// 데이터 보관 기간 (초, 0 = 무제한)
    pub retention_seconds: u64,
    /// 다운샘플링 활성화
    pub enable_downsampling: bool,
    /// 다운샘플링 간격 (초)
    pub downsample_interval: u64,
}

impl Default for TimeSeriesConfig {
    fn default() -> Self {
        Self {
            max_points_per_metric: 10000,
            retention_seconds: 86400 * 7, // 7일
            enable_downsampling: true,
            downsample_interval: 60, // 1분
        }
    }
}

impl TimeSeriesConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 테스트용 소규모 설정
    pub fn small() -> Self {
        Self {
            max_points_per_metric: 100,
            retention_seconds: 3600, // 1시간
            enable_downsampling: false,
            downsample_interval: 0,
        }
    }
}

/// 시계열 저장소
pub struct TimeSeriesStore {
    /// 메트릭별 데이터
    data: HashMap<MetricType, Vec<MetricPoint>>,
    /// 설정
    config: TimeSeriesConfig,
}

impl TimeSeriesStore {
    /// 새 저장소 생성
    pub fn new(config: TimeSeriesConfig) -> Self {
        Self {
            data: HashMap::new(),
            config,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(TimeSeriesConfig::default())
    }

    /// 메트릭 기록
    pub fn record(&mut self, metric_type: MetricType, point: MetricPoint) {
        let points = self.data.entry(metric_type).or_default();

        // 다운샘플링 적용
        if self.config.enable_downsampling
            && let Some(last) = points.last()
            && point.timestamp.saturating_sub(last.timestamp) < self.config.downsample_interval
        {
            // 마지막 포인트 업데이트 (평균)
            let averaged_value = (last.value + point.value) / 2.0;
            if let Some(last) = points.last_mut() {
                last.value = averaged_value;
            }
            return;
        }

        points.push(point);

        // 최대 포인트 수 제한
        if points.len() > self.config.max_points_per_metric {
            points.remove(0);
        }
    }

    /// 현재 값으로 메트릭 기록
    pub fn record_now(&mut self, metric_type: MetricType, value: f64) {
        self.record(metric_type, MetricPoint::now(value));
    }

    /// SystemStatus에서 메트릭 추출 및 기록
    pub fn record_system_status(&mut self, status: &SystemStatus) {
        let ts = status.timestamp;

        self.record(
            MetricType::MemoryUsage,
            MetricPoint::new(ts, status.memory_usage_ratio()),
        );
        self.record(
            MetricType::BufferPoolHitRatio,
            MetricPoint::new(ts, status.buffer_pool.hit_ratio),
        );
        self.record(
            MetricType::TransactionAbortRate,
            MetricPoint::new(ts, status.transactions.abort_ratio()),
        );
        self.record(
            MetricType::LockContention,
            MetricPoint::new(ts, status.locks.contention_level()),
        );
        self.record(
            MetricType::NodeCount,
            MetricPoint::new(ts, status.node_count as f64),
        );
        self.record(
            MetricType::EdgeCount,
            MetricPoint::new(ts, status.edge_count as f64),
        );
    }

    /// QueryStats에서 메트릭 추출 및 기록
    pub fn record_query_stats(&mut self, stats: &QueryStats) {
        self.record_now(MetricType::AvgQueryTime, stats.avg_time_us as f64);
        self.record_now(MetricType::IndexUsageRatio, stats.index_usage_ratio);
        if stats.total_queries > 0 {
            let slow_ratio = stats.slow_queries as f64 / stats.total_queries as f64;
            self.record_now(MetricType::SlowQueryRatio, slow_ratio);
        }
    }

    /// 메트릭 데이터 조회
    pub fn get(&self, metric_type: MetricType) -> Option<&[MetricPoint]> {
        self.data.get(&metric_type).map(|v| v.as_slice())
    }

    /// 시간 범위로 필터링된 데이터 조회
    pub fn get_range(&self, metric_type: MetricType, start: u64, end: u64) -> Vec<&MetricPoint> {
        self.data
            .get(&metric_type)
            .map(|points| {
                points
                    .iter()
                    .filter(|p| p.timestamp >= start && p.timestamp <= end)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 최근 N개 데이터 조회
    pub fn get_recent(&self, metric_type: MetricType, n: usize) -> Vec<&MetricPoint> {
        self.data
            .get(&metric_type)
            .map(|points| points.iter().rev().take(n).collect())
            .unwrap_or_default()
    }

    /// 현재 값 조회
    pub fn current_value(&self, metric_type: MetricType) -> Option<f64> {
        self.data
            .get(&metric_type)
            .and_then(|points| points.last())
            .map(|p| p.value)
    }

    /// 저장된 메트릭 유형 목록
    pub fn metric_types(&self) -> Vec<MetricType> {
        self.data.keys().copied().collect()
    }

    /// 메트릭별 포인트 수
    pub fn point_count(&self, metric_type: MetricType) -> usize {
        self.data.get(&metric_type).map(|v| v.len()).unwrap_or(0)
    }

    /// 전체 포인트 수
    pub fn total_points(&self) -> usize {
        self.data.values().map(|v| v.len()).sum()
    }

    /// 오래된 데이터 정리
    pub fn cleanup(&mut self) {
        if self.config.retention_seconds == 0 {
            return;
        }

        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(self.config.retention_seconds);

        for points in self.data.values_mut() {
            points.retain(|p| p.timestamp >= cutoff);
        }
    }

    /// 모든 데이터 클리어
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// 설정 조회
    pub fn config(&self) -> &TimeSeriesConfig {
        &self.config
    }
}

/// 트렌드 분석 결과
#[derive(Debug, Clone)]
pub struct TrendAnalysis {
    /// 메트릭 유형
    pub metric_type: MetricType,
    /// 트렌드 방향
    pub direction: TrendDirection,
    /// 변화율 (% / 시간 단위)
    pub change_rate: f64,
    /// 평균 값
    pub average: f64,
    /// 최소 값
    pub min: f64,
    /// 최대 값
    pub max: f64,
    /// 표준 편차
    pub std_dev: f64,
    /// 분석에 사용된 데이터 포인트 수
    pub sample_count: usize,
}

impl TrendAnalysis {
    /// 변동성 레벨 (표준편차 / 평균)
    pub fn volatility(&self) -> f64 {
        if self.average == 0.0 {
            0.0
        } else {
            self.std_dev / self.average
        }
    }
}

/// 이상치 탐지 결과
#[derive(Debug, Clone)]
pub struct AnomalyResult {
    /// 메트릭 유형
    pub metric_type: MetricType,
    /// 탐지된 이상치 포인트
    pub anomalies: Vec<MetricPoint>,
    /// 심각도 (0.0 ~ 1.0)
    pub severity: f64,
    /// 설명
    pub description: String,
}

/// 예측 결과
#[derive(Debug, Clone)]
pub struct ForecastResult {
    /// 메트릭 유형
    pub metric_type: MetricType,
    /// 예측 값
    pub predicted_value: f64,
    /// 예측 시점 (타임스탬프)
    pub predicted_at: u64,
    /// 신뢰도 (0.0 ~ 1.0)
    pub confidence: f64,
    /// 예측 범위 (하한, 상한)
    pub range: (f64, f64),
}

/// 히스토리 분석기 설정
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// 트렌드 분석용 최소 샘플 수
    pub min_samples_for_trend: usize,
    /// 이상치 탐지 Z-Score 임계값
    pub anomaly_z_threshold: f64,
    /// 트렌드 변화율 임계값
    pub trend_threshold: f64,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            min_samples_for_trend: 5,
            anomaly_z_threshold: 2.5,
            trend_threshold: 0.05,
        }
    }
}

impl AnalyzerConfig {}

/// 히스토리 분석기
pub struct HistoryAnalyzer {
    /// 시계열 저장소
    store: TimeSeriesStore,
    /// 분석기 설정
    config: AnalyzerConfig,
}

impl HistoryAnalyzer {
    /// 새 분석기 생성
    pub fn new(store_config: TimeSeriesConfig, analyzer_config: AnalyzerConfig) -> Self {
        Self {
            store: TimeSeriesStore::new(store_config),
            config: analyzer_config,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(TimeSeriesConfig::default(), AnalyzerConfig::default())
    }

    /// 시계열 저장소 참조
    pub fn store(&self) -> &TimeSeriesStore {
        &self.store
    }

    /// 시계열 저장소 가변 참조
    pub fn store_mut(&mut self) -> &mut TimeSeriesStore {
        &mut self.store
    }

    /// SystemStatus 기록
    pub fn record_system_status(&mut self, status: &SystemStatus) {
        self.store.record_system_status(status);
    }

    /// QueryStats 기록
    pub fn record_query_stats(&mut self, stats: &QueryStats) {
        self.store.record_query_stats(stats);
    }

    /// 트렌드 분석
    pub fn analyze_trend(&self, metric_type: MetricType) -> Option<TrendAnalysis> {
        let points = self.store.get(metric_type)?;

        if points.len() < self.config.min_samples_for_trend {
            return None;
        }

        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let n = values.len();

        // 기본 통계
        let average = values.iter().sum::<f64>() / n as f64;
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        let variance = values.iter().map(|v| (v - average).powi(2)).sum::<f64>() / n as f64;
        let std_dev = variance.sqrt();

        // 트렌드 분석 (선형 회귀의 기울기)
        let x_mean = (n - 1) as f64 / 2.0;
        let y_mean = average;

        let mut numerator = 0.0;
        let mut denominator = 0.0;

        for (i, v) in values.iter().enumerate() {
            let x = i as f64;
            numerator += (x - x_mean) * (v - y_mean);
            denominator += (x - x_mean).powi(2);
        }

        let slope = if denominator != 0.0 {
            numerator / denominator
        } else {
            0.0
        };

        // 변화율 (평균 대비)
        let change_rate = if average != 0.0 { slope / average } else { 0.0 };

        // 트렌드 방향 결정
        let direction = if change_rate > self.config.trend_threshold {
            if metric_type.higher_is_worse() {
                TrendDirection::Degrading
            } else {
                TrendDirection::Improving
            }
        } else if change_rate < -self.config.trend_threshold {
            if metric_type.higher_is_worse() {
                TrendDirection::Improving
            } else {
                TrendDirection::Degrading
            }
        } else {
            TrendDirection::Stable
        };

        Some(TrendAnalysis {
            metric_type,
            direction,
            change_rate,
            average,
            min,
            max,
            std_dev,
            sample_count: n,
        })
    }

    /// 모든 메트릭 트렌드 분석
    pub fn analyze_all_trends(&self) -> Vec<TrendAnalysis> {
        self.store
            .metric_types()
            .iter()
            .filter_map(|&mt| self.analyze_trend(mt))
            .collect()
    }

    /// 이상치 탐지
    pub fn detect_anomalies(&self, metric_type: MetricType) -> Option<AnomalyResult> {
        let points = self.store.get(metric_type)?;

        if points.len() < self.config.min_samples_for_trend {
            return None;
        }

        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let n = values.len();

        let mean = values.iter().sum::<f64>() / n as f64;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return Some(AnomalyResult {
                metric_type,
                anomalies: vec![],
                severity: 0.0,
                description: "No variation in data".to_string(),
            });
        }

        // Z-Score 기반 이상치 탐지
        let anomalies: Vec<MetricPoint> = points
            .iter()
            .filter(|p| {
                let z_score = (p.value - mean).abs() / std_dev;
                z_score > self.config.anomaly_z_threshold
            })
            .cloned()
            .collect();

        let severity = if anomalies.is_empty() {
            0.0
        } else {
            // 이상치 비율과 최대 Z-Score로 심각도 계산
            let max_z = anomalies
                .iter()
                .map(|p| (p.value - mean).abs() / std_dev)
                .fold(0.0, f64::max);

            let ratio = anomalies.len() as f64 / n as f64;
            ((max_z / self.config.anomaly_z_threshold - 1.0).min(1.0) * 0.5 + ratio * 0.5).min(1.0)
        };

        let description = if anomalies.is_empty() {
            "No anomalies detected".to_string()
        } else {
            format!(
                "Found {} anomalies ({}% of data)",
                anomalies.len(),
                (anomalies.len() as f64 / n as f64 * 100.0) as usize
            )
        };

        Some(AnomalyResult {
            metric_type,
            anomalies,
            severity,
            description,
        })
    }

    /// 모든 메트릭 이상치 탐지
    pub fn detect_all_anomalies(&self) -> Vec<AnomalyResult> {
        self.store
            .metric_types()
            .iter()
            .filter_map(|&mt| self.detect_anomalies(mt))
            .filter(|r| !r.anomalies.is_empty())
            .collect()
    }

    /// 단순 예측 (선형 외삽)
    pub fn forecast(&self, metric_type: MetricType, seconds_ahead: u64) -> Option<ForecastResult> {
        let points = self.store.get(metric_type)?;

        if points.len() < self.config.min_samples_for_trend {
            return None;
        }

        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let n = values.len();

        // 선형 회귀
        let x_mean = (n - 1) as f64 / 2.0;
        let y_mean = values.iter().sum::<f64>() / n as f64;

        let mut numerator = 0.0;
        let mut denominator = 0.0;

        for (i, v) in values.iter().enumerate() {
            let x = i as f64;
            numerator += (x - x_mean) * (v - y_mean);
            denominator += (x - x_mean).powi(2);
        }

        let slope = if denominator != 0.0 {
            numerator / denominator
        } else {
            0.0
        };

        let intercept = y_mean - slope * x_mean;

        // 예측
        let last_ts = points.last()?.timestamp;
        let predicted_at = last_ts + seconds_ahead;

        // 시간 간격 추정
        let avg_interval = if n > 1 {
            (points.last()?.timestamp - points.first()?.timestamp) as f64 / (n - 1) as f64
        } else {
            1.0
        };

        let future_x = n as f64 + (seconds_ahead as f64 / avg_interval.max(1.0));
        let predicted_value = slope * future_x + intercept;

        // 예측 신뢰도 (R² 기반)
        let ss_res: f64 = values
            .iter()
            .enumerate()
            .map(|(i, v)| (v - (slope * i as f64 + intercept)).powi(2))
            .sum();
        let ss_tot: f64 = values.iter().map(|v| (v - y_mean).powi(2)).sum();

        let r_squared = if ss_tot != 0.0 {
            1.0 - ss_res / ss_tot
        } else {
            0.0
        };

        // 거리에 따른 신뢰도 감소
        let distance_factor = 1.0 / (1.0 + (seconds_ahead as f64 / 3600.0).sqrt());
        let confidence = (r_squared * distance_factor).clamp(0.0, 1.0);

        // 예측 범위
        let variance = values.iter().map(|v| (v - y_mean).powi(2)).sum::<f64>() / n as f64;
        let std_dev = variance.sqrt();
        let margin = std_dev * (1.0 + (seconds_ahead as f64 / 3600.0).sqrt());

        Some(ForecastResult {
            metric_type,
            predicted_value,
            predicted_at,
            confidence,
            range: (predicted_value - margin, predicted_value + margin),
        })
    }

    /// 종합 분석 리포트 생성
    pub fn generate_report(&self) -> HistoryReport {
        let trends = self.analyze_all_trends();
        let anomalies = self.detect_all_anomalies();

        // 악화 중인 메트릭 수
        let degrading_count = trends
            .iter()
            .filter(|t| t.direction == TrendDirection::Degrading)
            .count();

        // 이상치가 있는 메트릭 수 (건강 점수 계산에서 사용)
        let _anomaly_count = anomalies.len();

        // 전체 건강 점수 (0 ~ 100)
        let health_score = {
            let degrading_penalty = degrading_count as f64 * 10.0;
            let anomaly_penalty = anomalies.iter().map(|a| a.severity * 15.0).sum::<f64>();
            (100.0 - degrading_penalty - anomaly_penalty).max(0.0) as u32
        };

        HistoryReport {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            trends,
            anomalies,
            health_score,
            total_metrics: self.store.metric_types().len(),
            total_data_points: self.store.total_points(),
        }
    }

    /// 설정 조회
    pub fn config(&self) -> &AnalyzerConfig {
        &self.config
    }
}

/// 히스토리 분석 리포트
#[derive(Debug, Clone)]
pub struct HistoryReport {
    /// 리포트 생성 시간
    pub timestamp: u64,
    /// 트렌드 분석 결과
    pub trends: Vec<TrendAnalysis>,
    /// 이상치 탐지 결과
    pub anomalies: Vec<AnomalyResult>,
    /// 전체 건강 점수 (0 ~ 100)
    pub health_score: u32,
    /// 분석된 메트릭 수
    pub total_metrics: usize,
    /// 총 데이터 포인트 수
    pub total_data_points: usize,
}

impl HistoryReport {
    /// 악화 중인 메트릭 목록
    pub fn degrading_metrics(&self) -> Vec<&TrendAnalysis> {
        self.trends
            .iter()
            .filter(|t| t.direction == TrendDirection::Degrading)
            .collect()
    }

    /// 개선 중인 메트릭 목록
    pub fn improving_metrics(&self) -> Vec<&TrendAnalysis> {
        self.trends
            .iter()
            .filter(|t| t.direction == TrendDirection::Improving)
            .collect()
    }

    /// 건강 상태 요약
    pub fn health_summary(&self) -> &'static str {
        match self.health_score {
            90..=100 => "Excellent",
            70..=89 => "Good",
            50..=69 => "Fair",
            30..=49 => "Poor",
            _ => "Critical",
        }
    }
}
