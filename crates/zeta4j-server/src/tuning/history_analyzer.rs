//! 튜닝 히스토리 분석 모듈
//!
//! 장기 트렌드 분석과 주기적 패턴 감지를 제공합니다.
//!
//! # 기능
//!
//! - 튜닝 메트릭 시계열 저장
//! - 장기 트렌드 분석 (상승/하락/안정)
//! - 주기적 패턴 감지 (일간/주간)
//! - 예측 기반 권장사항
//!
//! # 사용 예시
//!
//! ```ignore
//! use zeta4j_server::tuning::{TuningHistoryAnalyzer, TuningMetricType};
//!
//! let mut analyzer = TuningHistoryAnalyzer::with_defaults();
//!
//! // 메트릭 기록
//! analyzer.record(TuningMetricType::QueryFrequency, 1000.0);
//! analyzer.record(TuningMetricType::IndexHitRatio, 0.95);
//!
//! // 트렌드 분석
//! let trend = analyzer.analyze_trend(TuningMetricType::QueryFrequency);
//! println!("트렌드: {:?}", trend.direction);
//!
//! // 주기적 패턴 감지
//! let pattern = analyzer.detect_periodic_pattern(TuningMetricType::QueryFrequency);
//! println!("피크 시간: {:?}", pattern.peak_hours);
//! ```

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// 튜닝 메트릭 유형
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TuningMetricType {
    /// 쿼리 빈도 (queries/sec)
    QueryFrequency,
    /// 인덱스 히트율
    IndexHitRatio,
    /// 버퍼 풀 히트율
    BufferPoolHitRatio,
    /// 평균 쿼리 시간 (ms)
    AvgQueryTime,
    /// 인덱스 추천 수
    IndexRecommendationCount,
    /// 메모리 사용률
    MemoryUsage,
    /// 락 경합 수준
    LockContention,
    /// 시스템 상태 점수
    HealthScore,
    /// 사용자 정의 메트릭
    Custom(u32),
}

impl TuningMetricType {
    /// 메트릭 이름
    pub fn name(&self) -> &'static str {
        match self {
            TuningMetricType::QueryFrequency => "query_frequency",
            TuningMetricType::IndexHitRatio => "index_hit_ratio",
            TuningMetricType::BufferPoolHitRatio => "buffer_pool_hit_ratio",
            TuningMetricType::AvgQueryTime => "avg_query_time",
            TuningMetricType::IndexRecommendationCount => "index_recommendation_count",
            TuningMetricType::MemoryUsage => "memory_usage",
            TuningMetricType::LockContention => "lock_contention",
            TuningMetricType::HealthScore => "health_score",
            TuningMetricType::Custom(_) => "custom",
        }
    }

    /// 값이 높을수록 나쁜지 여부
    pub fn higher_is_worse(&self) -> bool {
        match self {
            TuningMetricType::QueryFrequency => false,
            TuningMetricType::IndexHitRatio => false,
            TuningMetricType::BufferPoolHitRatio => false,
            TuningMetricType::AvgQueryTime => true,
            TuningMetricType::IndexRecommendationCount => true,
            TuningMetricType::MemoryUsage => true,
            TuningMetricType::LockContention => true,
            TuningMetricType::HealthScore => false,
            TuningMetricType::Custom(_) => true,
        }
    }
}

/// 메트릭 데이터 포인트
#[derive(Debug, Clone)]
pub struct TuningMetricPoint {
    /// 타임스탬프 (Unix epoch seconds)
    pub timestamp: u64,
    /// 값
    pub value: f64,
}

impl TuningMetricPoint {
    /// 새 메트릭 포인트 생성
    pub fn new(timestamp: u64, value: f64) -> Self {
        Self { timestamp, value }
    }

    /// 현재 시간으로 생성
    pub fn now(value: f64) -> Self {
        Self {
            timestamp: current_timestamp(),
            value,
        }
    }
}

/// 트렌드 방향
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendDirection {
    /// 상승
    Rising,
    /// 하락
    Falling,
    /// 안정
    Stable,
    /// 불규칙
    Irregular,
}

impl TrendDirection {
    /// 트렌드 이름
    pub fn name(&self) -> &'static str {
        match self {
            TrendDirection::Rising => "rising",
            TrendDirection::Falling => "falling",
            TrendDirection::Stable => "stable",
            TrendDirection::Irregular => "irregular",
        }
    }
}

/// 트렌드 분석 결과
#[derive(Debug, Clone)]
pub struct TrendAnalysis {
    /// 메트릭 타입
    pub metric_type: TuningMetricType,
    /// 트렌드 방향
    pub direction: TrendDirection,
    /// 변화율 (percent per hour)
    pub change_rate: f64,
    /// 이동 평균
    pub moving_average: f64,
    /// 최근 값
    pub current_value: f64,
    /// 분석 기간 (초)
    pub period_seconds: u64,
    /// 분석에 사용된 데이터 포인트 수
    pub data_points: usize,
    /// 신뢰도 (0.0 ~ 1.0)
    pub confidence: f64,
}

impl TrendAnalysis {
    /// 요약 문자열
    pub fn summary(&self) -> String {
        format!(
            "{}: {} (change rate: {:.2}%/h, moving avg: {:.2}, confidence: {:.1}%)",
            self.metric_type.name(),
            self.direction.name(),
            self.change_rate,
            self.moving_average,
            self.confidence * 100.0
        )
    }
}

/// 주기적 패턴 분석 결과
#[derive(Debug, Clone)]
pub struct PeriodicPattern {
    /// 메트릭 타입
    pub metric_type: TuningMetricType,
    /// 주기성 감지 여부
    pub has_periodicity: bool,
    /// 주기 유형
    pub period_type: PeriodType,
    /// 피크 시간대 (0-23시)
    pub peak_hours: Vec<u8>,
    /// 저점 시간대 (0-23시)
    pub low_hours: Vec<u8>,
    /// 피크 요일 (0=일요일 ~ 6=토요일)
    pub peak_days: Vec<u8>,
    /// 시간대별 평균 값
    pub hourly_averages: [f64; 24],
    /// 요일별 평균 값
    pub daily_averages: [f64; 7],
    /// 분석에 사용된 데이터 포인트 수
    pub data_points: usize,
}

impl PeriodicPattern {
    /// 빈 패턴 생성
    pub fn empty(metric_type: TuningMetricType) -> Self {
        Self {
            metric_type,
            has_periodicity: false,
            period_type: PeriodType::None,
            peak_hours: Vec::new(),
            low_hours: Vec::new(),
            peak_days: Vec::new(),
            hourly_averages: [0.0; 24],
            daily_averages: [0.0; 7],
            data_points: 0,
        }
    }

    /// 요약 문자열
    pub fn summary(&self) -> String {
        if !self.has_periodicity {
            return format!("{}: no periodic pattern", self.metric_type.name());
        }

        let peak_hours_str: Vec<String> =
            self.peak_hours.iter().map(|h| format!("{}h", h)).collect();

        format!(
            "{}: {:?} pattern, peak hours: {}",
            self.metric_type.name(),
            self.period_type,
            peak_hours_str.join(", ")
        )
    }
}

/// 주기 유형
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodType {
    /// 없음
    None,
    /// 일간 (시간대별)
    Daily,
    /// 주간 (요일별)
    Weekly,
    /// 복합 (일간 + 주간)
    Combined,
}

/// 예측 결과
#[derive(Debug, Clone)]
pub struct Prediction {
    /// 메트릭 타입
    pub metric_type: TuningMetricType,
    /// 예측 시간 (현재로부터 초)
    pub horizon_seconds: u64,
    /// 예측 값
    pub predicted_value: f64,
    /// 하한 (95% 신뢰구간)
    pub lower_bound: f64,
    /// 상한 (95% 신뢰구간)
    pub upper_bound: f64,
    /// 신뢰도
    pub confidence: f64,
}

impl Prediction {
    /// 요약 문자열
    pub fn summary(&self) -> String {
        format!(
            "{}: prediction after {} hours {:.2} ({:.2} ~ {:.2}), confidence: {:.1}%",
            self.metric_type.name(),
            self.horizon_seconds / 3600,
            self.predicted_value,
            self.lower_bound,
            self.upper_bound,
            self.confidence * 100.0
        )
    }
}

/// 선제적 튜닝 권장사항
#[derive(Debug, Clone)]
pub struct ProactiveRecommendation {
    /// 권장사항 유형
    pub recommendation_type: RecommendationType,
    /// 관련 메트릭
    pub related_metric: TuningMetricType,
    /// 설명
    pub description: String,
    /// 예상 영향
    pub expected_impact: String,
    /// 우선순위 (1=높음, 5=낮음)
    pub priority: u8,
    /// 권장 실행 시간 (Unix timestamp, 0=즉시)
    pub suggested_time: u64,
}

impl ProactiveRecommendation {
    /// 요약 문자열
    pub fn summary(&self) -> String {
        format!(
            "[P{}] {:?}: {}",
            self.priority, self.recommendation_type, self.description
        )
    }
}

/// 권장사항 유형
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecommendationType {
    /// 인덱스 생성
    CreateIndex,
    /// 인덱스 삭제
    DropIndex,
    /// 버퍼 풀 크기 조정
    AdjustBufferPool,
    /// 통계 갱신
    RefreshStats,
    /// 유지보수 작업
    Maintenance,
    /// 경고
    Warning,
}

/// 히스토리 분석기 설정
#[derive(Debug, Clone)]
pub struct HistoryAnalyzerConfig {
    /// 메트릭별 최대 데이터 포인트 수
    pub max_points_per_metric: usize,
    /// 데이터 보관 기간 (초)
    pub retention_seconds: u64,
    /// 트렌드 분석 최소 데이터 포인트 수
    pub min_trend_points: usize,
    /// 주기 분석 최소 일수
    pub min_period_days: usize,
    /// 트렌드 변화 임계값 (percent per hour)
    pub trend_threshold: f64,
    /// 피크 감지 임계값 (평균 대비 배수)
    pub peak_threshold: f64,
}

impl Default for HistoryAnalyzerConfig {
    fn default() -> Self {
        Self {
            max_points_per_metric: 10000,
            retention_seconds: 86400 * 7, // 7일
            min_trend_points: 10,
            min_period_days: 3,
            trend_threshold: 1.0, // 1% per hour
            peak_threshold: 1.5,  // 평균의 1.5배
        }
    }
}

impl HistoryAnalyzerConfig {}

/// 튜닝 히스토리 분석기
pub struct TuningHistoryAnalyzer {
    /// 설정
    config: HistoryAnalyzerConfig,
    /// 메트릭별 시계열 데이터
    data: HashMap<TuningMetricType, Vec<TuningMetricPoint>>,
    /// 마지막 정리 시간
    last_cleanup: u64,
}

impl TuningHistoryAnalyzer {
    /// 새 분석기 생성
    pub fn new(config: HistoryAnalyzerConfig) -> Self {
        Self {
            config,
            data: HashMap::new(),
            last_cleanup: current_timestamp(),
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(HistoryAnalyzerConfig::default())
    }

    /// 메트릭 기록
    pub fn record(&mut self, metric_type: TuningMetricType, value: f64) {
        self.record_at(metric_type, value, current_timestamp());
    }

    /// 특정 시간에 메트릭 기록
    pub fn record_at(&mut self, metric_type: TuningMetricType, value: f64, timestamp: u64) {
        let points = self.data.entry(metric_type).or_default();
        points.push(TuningMetricPoint::new(timestamp, value));

        // 최대 포인트 수 초과 시 오래된 데이터 제거
        if points.len() > self.config.max_points_per_metric {
            points.remove(0);
        }

        // 주기적 정리
        if current_timestamp() - self.last_cleanup > 3600 {
            self.cleanup();
        }
    }

    /// 오래된 데이터 정리
    pub fn cleanup(&mut self) {
        let cutoff = current_timestamp() - self.config.retention_seconds;

        for points in self.data.values_mut() {
            points.retain(|p| p.timestamp >= cutoff);
        }

        self.last_cleanup = current_timestamp();
    }

    /// 메트릭 데이터 포인트 수 반환
    pub fn point_count(&self, metric_type: TuningMetricType) -> usize {
        self.data.get(&metric_type).map(|v| v.len()).unwrap_or(0)
    }

    /// 트렌드 분석
    pub fn analyze_trend(&self, metric_type: TuningMetricType) -> TrendAnalysis {
        self.analyze_trend_period(metric_type, 3600) // 기본 1시간
    }

    /// 특정 기간 트렌드 분석
    pub fn analyze_trend_period(
        &self,
        metric_type: TuningMetricType,
        period_seconds: u64,
    ) -> TrendAnalysis {
        let points = self.get_points_in_period(metric_type, period_seconds);

        if points.len() < self.config.min_trend_points {
            return TrendAnalysis {
                metric_type,
                direction: TrendDirection::Irregular,
                change_rate: 0.0,
                moving_average: points.last().map(|p| p.value).unwrap_or(0.0),
                current_value: points.last().map(|p| p.value).unwrap_or(0.0),
                period_seconds,
                data_points: points.len(),
                confidence: 0.0,
            };
        }

        // 선형 회귀로 트렌드 계산
        let (slope, _intercept, r_squared) = self.linear_regression(&points);

        // 변화율 계산 (percent per hour)
        let avg = self.calculate_average(&points);
        let change_rate = if avg.abs() > 0.001 {
            (slope * 3600.0 / avg) * 100.0
        } else {
            0.0
        };

        // 트렌드 방향 결정
        let direction = if r_squared < 0.3 {
            TrendDirection::Irregular
        } else if change_rate > self.config.trend_threshold {
            TrendDirection::Rising
        } else if change_rate < -self.config.trend_threshold {
            TrendDirection::Falling
        } else {
            TrendDirection::Stable
        };

        // 이동 평균 계산 (최근 10개 포인트)
        let moving_average = self.calculate_moving_average(&points, 10);

        TrendAnalysis {
            metric_type,
            direction,
            change_rate,
            moving_average,
            current_value: points.last().map(|p| p.value).unwrap_or(0.0),
            period_seconds,
            data_points: points.len(),
            confidence: r_squared.clamp(0.0, 1.0),
        }
    }

    /// 주기적 패턴 감지
    pub fn detect_periodic_pattern(&self, metric_type: TuningMetricType) -> PeriodicPattern {
        let points = match self.data.get(&metric_type) {
            Some(p) if p.len() >= 24 => p,
            _ => return PeriodicPattern::empty(metric_type),
        };

        // 시간대별, 요일별 데이터 수집
        let mut hourly_sums = [0.0f64; 24];
        let mut hourly_counts = [0usize; 24];
        let mut daily_sums = [0.0f64; 7];
        let mut daily_counts = [0usize; 7];

        for point in points {
            let (hour, day) = self.timestamp_to_hour_day(point.timestamp);
            hourly_sums[hour] += point.value;
            hourly_counts[hour] += 1;
            daily_sums[day] += point.value;
            daily_counts[day] += 1;
        }

        // 평균 계산
        let mut hourly_averages = [0.0f64; 24];
        for i in 0..24 {
            if hourly_counts[i] > 0 {
                hourly_averages[i] = hourly_sums[i] / hourly_counts[i] as f64;
            }
        }

        let mut daily_averages = [0.0f64; 7];
        for i in 0..7 {
            if daily_counts[i] > 0 {
                daily_averages[i] = daily_sums[i] / daily_counts[i] as f64;
            }
        }

        // 전체 평균
        let overall_avg = self.calculate_average(points);

        // 피크 시간대 및 저점 감지
        let peak_threshold = overall_avg * self.config.peak_threshold;
        let low_threshold = overall_avg / self.config.peak_threshold;

        let peak_hours: Vec<u8> = hourly_averages
            .iter()
            .enumerate()
            .filter(|(_, avg)| **avg > peak_threshold)
            .map(|(i, _)| i as u8)
            .collect();

        let low_hours: Vec<u8> = hourly_averages
            .iter()
            .enumerate()
            .filter(|(_, avg)| **avg < low_threshold && **avg > 0.0)
            .map(|(i, _)| i as u8)
            .collect();

        let peak_days: Vec<u8> = daily_averages
            .iter()
            .enumerate()
            .filter(|(_, avg)| **avg > peak_threshold)
            .map(|(i, _)| i as u8)
            .collect();

        // 주기성 판단
        let hourly_variance = self.calculate_variance(&hourly_averages);
        let daily_variance = self.calculate_variance(&daily_averages);

        let has_daily_pattern = hourly_variance > overall_avg * 0.1 && !peak_hours.is_empty();
        let has_weekly_pattern = daily_variance > overall_avg * 0.1 && !peak_days.is_empty();

        let (has_periodicity, period_type) = match (has_daily_pattern, has_weekly_pattern) {
            (true, true) => (true, PeriodType::Combined),
            (true, false) => (true, PeriodType::Daily),
            (false, true) => (true, PeriodType::Weekly),
            (false, false) => (false, PeriodType::None),
        };

        PeriodicPattern {
            metric_type,
            has_periodicity,
            period_type,
            peak_hours,
            low_hours,
            peak_days,
            hourly_averages,
            daily_averages,
            data_points: points.len(),
        }
    }

    /// 예측 생성
    pub fn predict(&self, metric_type: TuningMetricType, horizon_seconds: u64) -> Prediction {
        let trend = self.analyze_trend_period(metric_type, 3600 * 6); // 6시간 트렌드

        if trend.confidence < 0.3 || trend.data_points < self.config.min_trend_points {
            return Prediction {
                metric_type,
                horizon_seconds,
                predicted_value: trend.current_value,
                lower_bound: trend.current_value * 0.8,
                upper_bound: trend.current_value * 1.2,
                confidence: 0.0,
            };
        }

        // 선형 추세 기반 예측
        let hours = horizon_seconds as f64 / 3600.0;
        let change = trend.change_rate / 100.0 * trend.current_value * hours;
        let predicted_value = trend.current_value + change;

        // 신뢰구간 (표준편차 기반)
        let points = self.get_points_in_period(metric_type, 3600 * 6);
        let std_dev = self.calculate_std_dev(&points);
        let margin = std_dev * 1.96 * (1.0 + hours * 0.1); // 시간에 따라 증가

        // 신뢰도는 예측 기간에 반비례
        let confidence = (trend.confidence * (1.0 - hours / 24.0)).max(0.1);

        Prediction {
            metric_type,
            horizon_seconds,
            predicted_value,
            lower_bound: predicted_value - margin,
            upper_bound: predicted_value + margin,
            confidence,
        }
    }

    /// 선제적 권장사항 생성
    pub fn generate_recommendations(&self) -> Vec<ProactiveRecommendation> {
        let mut recommendations = Vec::new();

        // 각 메트릭별 분석
        for &metric_type in &[
            TuningMetricType::QueryFrequency,
            TuningMetricType::IndexHitRatio,
            TuningMetricType::BufferPoolHitRatio,
            TuningMetricType::AvgQueryTime,
            TuningMetricType::MemoryUsage,
        ] {
            if let Some(rec) = self.analyze_metric_for_recommendation(metric_type) {
                recommendations.push(rec);
            }
        }

        // 주기적 패턴 기반 권장사항
        if let Some(rec) = self.analyze_periodic_recommendation() {
            recommendations.push(rec);
        }

        // 우선순위로 정렬
        recommendations.sort_by_key(|r| r.priority);
        recommendations
    }

    /// 메트릭별 권장사항 분석
    fn analyze_metric_for_recommendation(
        &self,
        metric_type: TuningMetricType,
    ) -> Option<ProactiveRecommendation> {
        let trend = self.analyze_trend(metric_type);

        if trend.confidence < 0.5 || trend.data_points < self.config.min_trend_points {
            return None;
        }

        // 악화 추세 감지
        let is_worsening = if metric_type.higher_is_worse() {
            trend.direction == TrendDirection::Rising
        } else {
            trend.direction == TrendDirection::Falling
        };

        if !is_worsening {
            return None;
        }

        // 메트릭별 권장사항 생성
        match metric_type {
            TuningMetricType::IndexHitRatio => Some(ProactiveRecommendation {
                recommendation_type: RecommendationType::CreateIndex,
                related_metric: metric_type,
                description: format!(
                    "Index hit rate declining ({:.1}%/h). Adding indexes recommended",
                    trend.change_rate
                ),
                expected_impact: "Query performance improvement".to_string(),
                priority: 2,
                suggested_time: 0,
            }),
            TuningMetricType::BufferPoolHitRatio => Some(ProactiveRecommendation {
                recommendation_type: RecommendationType::AdjustBufferPool,
                related_metric: metric_type,
                description: format!(
                    "Buffer pool hit rate declining ({:.1}%/h). Increasing buffer pool size recommended",
                    trend.change_rate
                ),
                expected_impact: "Cache efficiency improvement".to_string(),
                priority: 2,
                suggested_time: 0,
            }),
            TuningMetricType::AvgQueryTime => Some(ProactiveRecommendation {
                recommendation_type: RecommendationType::RefreshStats,
                related_metric: metric_type,
                description: format!(
                    "Average query time increasing ({:.1}%/h). Statistics refresh and query optimization recommended",
                    trend.change_rate
                ),
                expected_impact: "Query response time improvement".to_string(),
                priority: 1,
                suggested_time: 0,
            }),
            TuningMetricType::MemoryUsage => {
                if trend.current_value > 0.85 {
                    Some(ProactiveRecommendation {
                        recommendation_type: RecommendationType::Warning,
                        related_metric: metric_type,
                        description: format!(
                            "Memory usage reached {:.1}%. Risk of OOM if it continues to increase",
                            trend.current_value * 100.0
                        ),
                        expected_impact: "Maintain system stability".to_string(),
                        priority: 1,
                        suggested_time: 0,
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 주기적 패턴 기반 권장사항
    fn analyze_periodic_recommendation(&self) -> Option<ProactiveRecommendation> {
        let pattern = self.detect_periodic_pattern(TuningMetricType::QueryFrequency);

        if !pattern.has_periodicity || pattern.peak_hours.is_empty() {
            return None;
        }

        // 현재 시간이 피크 시간 1시간 전인지 확인
        let current_hour = (current_timestamp() / 3600 % 24) as u8;
        let next_hour = (current_hour + 1) % 24;

        if pattern.peak_hours.contains(&next_hour) {
            return Some(ProactiveRecommendation {
                recommendation_type: RecommendationType::Maintenance,
                related_metric: TuningMetricType::QueryFrequency,
                description: format!(
                    "Peak hour ({:02}:00) expected in 1 hour. Pre-loading stats and cache recommended",
                    next_hour
                ),
                expected_impact: "Maintain peak hour performance".to_string(),
                priority: 3,
                suggested_time: current_timestamp() + 1800, // 30분 후
            });
        }

        None
    }

    /// 특정 기간의 데이터 포인트 반환
    fn get_points_in_period(
        &self,
        metric_type: TuningMetricType,
        period_seconds: u64,
    ) -> Vec<TuningMetricPoint> {
        let cutoff = current_timestamp() - period_seconds;

        match self.data.get(&metric_type) {
            Some(points) => points
                .iter()
                .filter(|p| p.timestamp >= cutoff)
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    /// 선형 회귀 계산 (slope, intercept, r_squared)
    fn linear_regression(&self, points: &[TuningMetricPoint]) -> (f64, f64, f64) {
        if points.len() < 2 {
            return (0.0, 0.0, 0.0);
        }

        let n = points.len() as f64;
        let base_time = points.first().map(|p| p.timestamp).unwrap_or(0);

        let xs: Vec<f64> = points
            .iter()
            .map(|p| (p.timestamp - base_time) as f64)
            .collect();
        let ys: Vec<f64> = points.iter().map(|p| p.value).collect();

        let sum_x: f64 = xs.iter().sum();
        let sum_y: f64 = ys.iter().sum();
        let sum_xy: f64 = xs.iter().zip(ys.iter()).map(|(x, y)| x * y).sum();
        let sum_x2: f64 = xs.iter().map(|x| x * x).sum();
        let _sum_y2: f64 = ys.iter().map(|y| y * y).sum();

        let denominator = n * sum_x2 - sum_x * sum_x;
        if denominator.abs() < 1e-10 {
            return (0.0, sum_y / n, 0.0);
        }

        let slope = (n * sum_xy - sum_x * sum_y) / denominator;
        let intercept = (sum_y - slope * sum_x) / n;

        // R-squared 계산
        let y_mean = sum_y / n;
        let ss_tot: f64 = ys.iter().map(|y| (y - y_mean).powi(2)).sum();
        let ss_res: f64 = xs
            .iter()
            .zip(ys.iter())
            .map(|(x, y)| {
                let predicted = slope * x + intercept;
                (y - predicted).powi(2)
            })
            .sum();

        let r_squared = if ss_tot > 0.0 {
            1.0 - (ss_res / ss_tot)
        } else {
            0.0
        };

        (slope, intercept, r_squared)
    }

    /// 평균 계산
    fn calculate_average(&self, points: &[TuningMetricPoint]) -> f64 {
        if points.is_empty() {
            return 0.0;
        }
        points.iter().map(|p| p.value).sum::<f64>() / points.len() as f64
    }

    /// 이동 평균 계산
    fn calculate_moving_average(&self, points: &[TuningMetricPoint], window: usize) -> f64 {
        if points.is_empty() {
            return 0.0;
        }
        let start = points.len().saturating_sub(window);
        let subset = &points[start..];
        subset.iter().map(|p| p.value).sum::<f64>() / subset.len() as f64
    }

    /// 표준편차 계산
    fn calculate_std_dev(&self, points: &[TuningMetricPoint]) -> f64 {
        if points.len() < 2 {
            return 0.0;
        }
        let avg = self.calculate_average(points);
        let variance =
            points.iter().map(|p| (p.value - avg).powi(2)).sum::<f64>() / (points.len() - 1) as f64;
        variance.sqrt()
    }

    /// 분산 계산 (배열용)
    fn calculate_variance(&self, values: &[f64]) -> f64 {
        let non_zero: Vec<f64> = values.iter().copied().filter(|&v| v > 0.0).collect();
        if non_zero.len() < 2 {
            return 0.0;
        }
        let avg: f64 = non_zero.iter().sum::<f64>() / non_zero.len() as f64;
        non_zero.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / (non_zero.len() - 1) as f64
    }

    /// 타임스탬프를 시간과 요일로 변환
    fn timestamp_to_hour_day(&self, timestamp: u64) -> (usize, usize) {
        // Unix timestamp를 시간과 요일로 변환
        let seconds_in_day = 86400u64;
        let seconds_in_hour = 3600u64;

        let hour = ((timestamp % seconds_in_day) / seconds_in_hour) as usize;

        // 1970-01-01은 목요일 (4)
        let day = ((timestamp / seconds_in_day + 4) % 7) as usize;

        (hour, day)
    }

    /// 최근 데이터 가져오기
    pub fn get_recent_data(
        &self,
        metric_type: TuningMetricType,
        count: usize,
    ) -> Vec<TuningMetricPoint> {
        match self.data.get(&metric_type) {
            Some(points) => {
                let start = points.len().saturating_sub(count);
                points[start..].to_vec()
            }
            None => Vec::new(),
        }
    }

    /// 전체 메트릭 목록
    pub fn metrics(&self) -> Vec<TuningMetricType> {
        self.data.keys().copied().collect()
    }

    /// 상태 초기화
    pub fn reset(&mut self) {
        self.data.clear();
        self.last_cleanup = current_timestamp();
    }
}

impl Default for TuningHistoryAnalyzer {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// 현재 타임스탬프 반환
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
