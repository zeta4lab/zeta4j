//! 워크로드 패턴 인식
//!
//! 시계열 데이터에서 시간대별, 요일별, 주기적 패턴을 인식합니다.

use std::time::Duration;

use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};

use super::collector::DataPoint;
use super::timeseries::{TimeSeriesAnalyzer, mean, std_dev};

/// 워크로드 패턴 유형
#[derive(Debug, Clone, PartialEq)]
pub enum PatternType {
    /// 시간대별 패턴 (예: 오전 9시~11시 피크)
    Hourly {
        /// 피크 시간대 (0-23)
        peak_hours: Vec<u8>,
        /// 시간대별 평균 부하 (24개)
        hourly_profile: [f64; 24],
    },
    /// 요일별 패턴 (예: 월~금 높음, 주말 낮음)
    Daily {
        /// 피크 요일
        peak_days: Vec<Weekday>,
        /// 요일별 평균 부하 (7개, 월=0)
        daily_profile: [f64; 7],
    },
    /// 주간 패턴 (시간*요일 = 168개 슬롯)
    Weekly {
        /// 주간 프로파일 (168개 = 7일 * 24시간)
        weekly_profile: Vec<f64>,
        /// 피크 시간대 (요일, 시간)
        peak_slots: Vec<(Weekday, u8)>,
    },
    /// 스파이크 패턴 (갑작스러운 증가)
    Spike {
        /// 스파이크 크기 (배수)
        magnitude: f64,
        /// 평균 지속 시간
        avg_duration: Duration,
        /// 빈도 (시간당)
        frequency_per_hour: f64,
    },
    /// 점진적 트렌드
    Trend {
        /// 기울기 (단위 시간당 변화량)
        slope: f64,
        /// 방향
        direction: TrendDirection,
        /// R² (결정 계수)
        r_squared: f64,
    },
    /// 주기적 패턴 (일반)
    Periodic {
        /// 주기 (초)
        period_secs: u64,
        /// 진폭
        amplitude: f64,
        /// 위상
        phase: f64,
    },
}

/// 트렌드 방향
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendDirection {
    /// 증가
    Increasing,
    /// 감소
    Decreasing,
    /// 변화 없음 (안정)
    Stable,
}

/// 인식된 패턴
#[derive(Debug, Clone)]
pub struct RecognizedPattern {
    /// 패턴 유형
    pub pattern_type: PatternType,
    /// 신뢰도 (0.0 ~ 1.0)
    pub confidence: f64,
    /// 패턴 시작 시간
    pub start_time: DateTime<Utc>,
    /// 패턴 종료 시간 (없으면 지속 중)
    pub end_time: Option<DateTime<Utc>>,
    /// 관련 메트릭 이름
    pub metric_name: Option<String>,
    /// 설명
    pub description: String,
}

impl RecognizedPattern {
    /// 새 패턴 생성
    pub fn new(pattern_type: PatternType, confidence: f64) -> Self {
        Self {
            pattern_type,
            confidence,
            start_time: Utc::now(),
            end_time: None,
            metric_name: None,
            description: String::new(),
        }
    }

    /// 메트릭 이름 설정
    pub fn with_metric(mut self, name: impl Into<String>) -> Self {
        self.metric_name = Some(name.into());
        self
    }

    /// 설명 설정
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// 시간 범위 설정
    pub fn with_time_range(mut self, start: DateTime<Utc>, end: Option<DateTime<Utc>>) -> Self {
        self.start_time = start;
        self.end_time = end;
        self
    }
}

/// 패턴 인식기 설정
#[derive(Debug, Clone)]
pub struct PatternConfig {
    /// 최소 신뢰도 (이 값 이상만 보고)
    pub min_confidence: f64,
    /// 스파이크 탐지 임계값 (표준편차 배수)
    pub spike_threshold: f64,
    /// 트렌드 판단 최소 기울기
    pub trend_min_slope: f64,
    /// 피크 판단 임계값 (평균 대비 비율)
    pub peak_threshold: f64,
    /// 최소 데이터 포인트 수
    pub min_data_points: usize,
}

impl Default for PatternConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
            spike_threshold: 3.0,
            trend_min_slope: 0.01,
            peak_threshold: 1.2, // 평균의 120% 이상이면 피크
            min_data_points: 24,
        }
    }
}

/// 패턴 인식기
pub struct PatternRecognizer {
    config: PatternConfig,
    ts_analyzer: TimeSeriesAnalyzer,
}

impl PatternRecognizer {
    /// 새 인식기 생성
    pub fn new(config: PatternConfig) -> Self {
        Self {
            config,
            ts_analyzer: TimeSeriesAnalyzer::default(),
        }
    }

    /// 시간대별 패턴 분석
    ///
    /// 24시간 주기의 시간대별 부하 패턴을 분석합니다.
    pub fn analyze_hourly(&self, data: &[DataPoint]) -> Option<RecognizedPattern> {
        if data.len() < self.config.min_data_points {
            return None;
        }

        // 시간대별 값 수집
        let mut hourly_values: [Vec<f64>; 24] = Default::default();
        for point in data {
            let hour = point.timestamp.hour() as usize;
            hourly_values[hour].push(point.value);
        }

        // 시간대별 평균 계산
        let mut hourly_profile = [0.0; 24];
        let mut valid_hours = 0;
        for (hour, values) in hourly_values.iter().enumerate() {
            if !values.is_empty() {
                hourly_profile[hour] = mean(values);
                valid_hours += 1;
            }
        }

        if valid_hours < 12 {
            return None; // 최소 12시간 데이터 필요
        }

        // 전체 평균
        let overall_mean = mean(&hourly_profile);
        if overall_mean == 0.0 {
            return None;
        }

        // 피크 시간 찾기 (평균의 threshold 배 이상)
        let peak_hours: Vec<u8> = (0..24)
            .filter(|&h| hourly_profile[h] >= overall_mean * self.config.peak_threshold)
            .map(|h| h as u8)
            .collect();

        // 신뢰도 계산: 분산 비율 (높을수록 패턴이 명확)
        let profile_std = std_dev(&hourly_profile);
        let confidence = (profile_std / overall_mean).min(1.0);

        if confidence < self.config.min_confidence {
            return None;
        }

        let pattern = PatternType::Hourly {
            peak_hours: peak_hours.clone(),
            hourly_profile,
        };

        let desc = if peak_hours.is_empty() {
            "Uniform hourly distribution".to_string()
        } else {
            format!(
                "Peak hours: {}",
                peak_hours
                    .iter()
                    .map(|h| h.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        Some(RecognizedPattern::new(pattern, confidence).with_description(desc))
    }

    /// 요일별 패턴 분석
    pub fn analyze_daily(&self, data: &[DataPoint]) -> Option<RecognizedPattern> {
        if data.len() < self.config.min_data_points {
            return None;
        }

        // 요일별 값 수집
        let mut daily_values: [Vec<f64>; 7] = Default::default();
        for point in data {
            let day = point.timestamp.weekday().num_days_from_monday() as usize;
            daily_values[day].push(point.value);
        }

        // 요일별 평균 계산
        let mut daily_profile = [0.0; 7];
        let mut valid_days = 0;
        for (day, values) in daily_values.iter().enumerate() {
            if !values.is_empty() {
                daily_profile[day] = mean(values);
                valid_days += 1;
            }
        }

        if valid_days < 5 {
            return None; // 최소 5일 데이터 필요
        }

        let overall_mean = mean(&daily_profile);
        if overall_mean == 0.0 {
            return None;
        }

        // 피크 요일 찾기
        let weekdays = [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ];

        let peak_days: Vec<Weekday> = (0..7)
            .filter(|&d| daily_profile[d] >= overall_mean * self.config.peak_threshold)
            .map(|d| weekdays[d])
            .collect();

        // 신뢰도 계산
        let profile_std = std_dev(&daily_profile);
        let confidence = (profile_std / overall_mean).min(1.0);

        if confidence < self.config.min_confidence / 2.0 {
            // 요일별은 변동이 작을 수 있음
            return None;
        }

        let pattern = PatternType::Daily {
            peak_days: peak_days.clone(),
            daily_profile,
        };

        // 주중/주말 패턴 분석
        let weekday_avg = (daily_profile[0..5].iter().sum::<f64>()) / 5.0;
        let weekend_avg = (daily_profile[5] + daily_profile[6]) / 2.0;

        let desc = if weekday_avg > weekend_avg * 1.3 {
            "Weekday > weekend pattern".to_string()
        } else if weekend_avg > weekday_avg * 1.3 {
            "Weekend > weekday pattern".to_string()
        } else {
            "Uniform daily distribution".to_string()
        };

        Some(RecognizedPattern::new(pattern, confidence).with_description(desc))
    }

    /// 주간 패턴 분석 (시간*요일)
    pub fn analyze_weekly(&self, data: &[DataPoint]) -> Option<RecognizedPattern> {
        if data.len() < 168 {
            // 최소 1주일 시간별 데이터
            return None;
        }

        // 168개 슬롯 (7일 * 24시간)
        let mut weekly_values: Vec<Vec<f64>> = vec![Vec::new(); 168];

        for point in data {
            let day = point.timestamp.weekday().num_days_from_monday() as usize;
            let hour = point.timestamp.hour() as usize;
            let slot = day * 24 + hour;
            weekly_values[slot].push(point.value);
        }

        // 슬롯별 평균
        let weekly_profile: Vec<f64> = weekly_values
            .iter()
            .map(|values| if values.is_empty() { 0.0 } else { mean(values) })
            .collect();

        let overall_mean = mean(&weekly_profile);
        if overall_mean == 0.0 {
            return None;
        }

        // 피크 슬롯 찾기
        let weekdays = [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ];

        let peak_slots: Vec<(Weekday, u8)> = (0..168)
            .filter(|&slot| weekly_profile[slot] >= overall_mean * self.config.peak_threshold)
            .map(|slot| {
                let day = slot / 24;
                let hour = slot % 24;
                (weekdays[day], hour as u8)
            })
            .collect();

        let profile_std = std_dev(&weekly_profile);
        let confidence = (profile_std / overall_mean).min(1.0);

        if confidence < self.config.min_confidence {
            return None;
        }

        let pattern = PatternType::Weekly {
            weekly_profile,
            peak_slots: peak_slots.clone(),
        };

        let desc = format!("Weekly pattern, {} peak slots detected", peak_slots.len());

        Some(RecognizedPattern::new(pattern, confidence).with_description(desc))
    }

    /// 스파이크 패턴 탐지
    pub fn detect_spikes(&self, data: &[DataPoint]) -> Vec<SpikeEvent> {
        if data.len() < 10 {
            return vec![];
        }

        let values: Vec<f64> = data.iter().map(|p| p.value).collect();
        let m = mean(&values);
        let s = std_dev(&values);

        if s == 0.0 {
            return vec![];
        }

        let threshold = m + self.config.spike_threshold * s;
        let mut spikes = Vec::new();
        let mut in_spike = false;
        let mut spike_start = 0;
        let mut spike_peak = 0.0;
        let mut spike_peak_idx = 0;

        for (i, point) in data.iter().enumerate() {
            if point.value > threshold {
                if !in_spike {
                    in_spike = true;
                    spike_start = i;
                    spike_peak = point.value;
                    spike_peak_idx = i;
                } else if point.value > spike_peak {
                    spike_peak = point.value;
                    spike_peak_idx = i;
                }
            } else if in_spike {
                // 스파이크 종료
                let duration = if spike_peak_idx > spike_start && spike_peak_idx < data.len() {
                    data[spike_peak_idx].timestamp - data[spike_start].timestamp
                } else {
                    chrono::Duration::zero()
                };

                spikes.push(SpikeEvent {
                    start_time: data[spike_start].timestamp,
                    peak_time: data[spike_peak_idx].timestamp,
                    end_time: data[i].timestamp,
                    peak_value: spike_peak,
                    magnitude: spike_peak / m,
                    duration,
                });

                in_spike = false;
            }
        }

        // 마지막 스파이크 처리
        if in_spike && spike_peak_idx > spike_start {
            let duration = data[spike_peak_idx].timestamp - data[spike_start].timestamp;
            spikes.push(SpikeEvent {
                start_time: data[spike_start].timestamp,
                peak_time: data[spike_peak_idx].timestamp,
                end_time: data.last().map(|p| p.timestamp).unwrap_or(Utc::now()),
                peak_value: spike_peak,
                magnitude: spike_peak / m,
                duration,
            });
        }

        spikes
    }

    /// 스파이크 패턴 분석 (요약)
    pub fn analyze_spikes(&self, data: &[DataPoint]) -> Option<RecognizedPattern> {
        let spikes = self.detect_spikes(data);

        if spikes.is_empty() {
            return None;
        }

        // 평균 스파이크 크기
        let avg_magnitude = spikes.iter().map(|s| s.magnitude).sum::<f64>() / spikes.len() as f64;

        // 평균 지속 시간
        let avg_duration_secs = spikes
            .iter()
            .map(|s| s.duration.num_seconds() as f64)
            .sum::<f64>()
            / spikes.len() as f64;

        // 빈도 계산
        let total_hours = if let (Some(first), Some(last)) = (data.first(), data.last()) {
            let first = first.timestamp;
            let last = last.timestamp;
            (last - first).num_hours().max(1) as f64
        } else {
            1.0
        };
        let frequency = spikes.len() as f64 / total_hours;

        let confidence = if spikes.len() >= 3 { 0.8 } else { 0.5 };

        let pattern = PatternType::Spike {
            magnitude: avg_magnitude,
            avg_duration: Duration::from_secs(avg_duration_secs as u64),
            frequency_per_hour: frequency,
        };

        let desc = format!(
            "{} spikes detected, average {:.1}x increase",
            spikes.len(),
            avg_magnitude
        );

        Some(RecognizedPattern::new(pattern, confidence).with_description(desc))
    }

    /// 트렌드 분석
    pub fn analyze_trend(&self, data: &[DataPoint]) -> Option<RecognizedPattern> {
        if data.len() < self.config.min_data_points {
            return None;
        }

        let values: Vec<f64> = data.iter().map(|p| p.value).collect();
        let n = values.len() as f64;

        // 선형 회귀: y = a + bx
        let _x: Vec<f64> = (0..values.len()).map(|i| i as f64).collect();
        let x_mean = (n - 1.0) / 2.0;
        let y_mean = mean(&values);

        let mut ss_xy = 0.0;
        let mut ss_xx = 0.0;

        for (i, &y) in values.iter().enumerate() {
            let x_i = i as f64;
            ss_xy += (x_i - x_mean) * (y - y_mean);
            ss_xx += (x_i - x_mean) * (x_i - x_mean);
        }

        if ss_xx == 0.0 {
            return None;
        }

        let slope = ss_xy / ss_xx;
        let intercept = y_mean - slope * x_mean;

        // R² 계산
        let mut ss_tot = 0.0;
        let mut ss_res = 0.0;

        for (i, &y) in values.iter().enumerate() {
            let y_pred = intercept + slope * (i as f64);
            ss_tot += (y - y_mean).powi(2);
            ss_res += (y - y_pred).powi(2);
        }

        let r_squared = if ss_tot > 0.0 {
            1.0 - (ss_res / ss_tot)
        } else {
            0.0
        };

        // 정규화된 기울기 (평균 대비)
        let normalized_slope = if y_mean != 0.0 { slope / y_mean } else { slope };

        let direction = if normalized_slope > self.config.trend_min_slope {
            TrendDirection::Increasing
        } else if normalized_slope < -self.config.trend_min_slope {
            TrendDirection::Decreasing
        } else {
            TrendDirection::Stable
        };

        // 신뢰도: R² 값 사용
        let confidence = r_squared;

        if confidence < self.config.min_confidence && direction == TrendDirection::Stable {
            return None;
        }

        let pattern = PatternType::Trend {
            slope,
            direction,
            r_squared,
        };

        let desc = match direction {
            TrendDirection::Increasing => format!("Increasing trend (R²={:.2})", r_squared),
            TrendDirection::Decreasing => format!("Decreasing trend (R²={:.2})", r_squared),
            TrendDirection::Stable => format!("Stable (R²={:.2})", r_squared),
        };

        Some(RecognizedPattern::new(pattern, confidence).with_description(desc))
    }

    /// 주기적 패턴 분석
    pub fn analyze_periodic(&self, data: &[DataPoint]) -> Option<RecognizedPattern> {
        if data.len() < 48 {
            return None;
        }

        let values: Vec<f64> = data.iter().map(|p| p.value).collect();

        // 주기 탐지
        let period = self
            .ts_analyzer
            .detect_period(&values, Some(2), Some(values.len() / 3))?;

        // 주기의 진폭과 위상 추정
        let decomposed = self.ts_analyzer.decompose(&values, period);
        let amplitude = std_dev(&decomposed.seasonal);
        let _overall_std = std_dev(&values);

        // 계절성 강도를 신뢰도로 사용
        let confidence = decomposed.seasonal_strength();

        if confidence < self.config.min_confidence {
            return None;
        }

        // 주기를 초로 변환 (데이터 간격 추정)
        let period_secs = if data.len() >= 2 {
            let interval = (data[1].timestamp - data[0].timestamp).num_seconds() as u64;
            period as u64 * interval.max(1)
        } else {
            period as u64 * 3600 // 기본 1시간 간격 가정
        };

        let pattern = PatternType::Periodic {
            period_secs,
            amplitude,
            phase: 0.0, // 간소화
        };

        let desc = format!("Period: {}s, amplitude: {:.2}", period_secs, amplitude);

        Some(RecognizedPattern::new(pattern, confidence).with_description(desc))
    }

    /// 모든 패턴 인식 수행
    pub fn recognize_all(&self, data: &[DataPoint]) -> Vec<RecognizedPattern> {
        let mut patterns = Vec::new();

        if let Some(p) = self.analyze_hourly(data) {
            patterns.push(p);
        }

        if let Some(p) = self.analyze_daily(data) {
            patterns.push(p);
        }

        if let Some(p) = self.analyze_weekly(data) {
            patterns.push(p);
        }

        if let Some(p) = self.analyze_spikes(data) {
            patterns.push(p);
        }

        if let Some(p) = self.analyze_trend(data) {
            patterns.push(p);
        }

        if let Some(p) = self.analyze_periodic(data) {
            patterns.push(p);
        }

        // 신뢰도 순으로 정렬
        patterns.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        patterns
    }

    /// 피크 시간 예측
    pub fn predict_peak_hours(&self, data: &[DataPoint]) -> Vec<(u8, f64)> {
        if let Some(pattern) = self.analyze_hourly(data)
            && let PatternType::Hourly { hourly_profile, .. } = pattern.pattern_type
        {
            let mut hours: Vec<(u8, f64)> = hourly_profile
                .iter()
                .enumerate()
                .map(|(h, &v)| (h as u8, v))
                .collect();
            hours.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            return hours;
        }
        vec![]
    }

    /// 피크 요일 예측
    pub fn predict_peak_days(&self, data: &[DataPoint]) -> Vec<(Weekday, f64)> {
        let weekdays = [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ];

        if let Some(pattern) = self.analyze_daily(data)
            && let PatternType::Daily { daily_profile, .. } = pattern.pattern_type
        {
            let mut days: Vec<(Weekday, f64)> = daily_profile
                .iter()
                .enumerate()
                .map(|(d, &v)| (weekdays[d], v))
                .collect();
            days.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            return days;
        }
        vec![]
    }
}

impl Default for PatternRecognizer {
    fn default() -> Self {
        Self::new(PatternConfig::default())
    }
}

/// 스파이크 이벤트
#[derive(Debug, Clone)]
pub struct SpikeEvent {
    /// 시작 시간
    pub start_time: DateTime<Utc>,
    /// 피크 시간
    pub peak_time: DateTime<Utc>,
    /// 종료 시간
    pub end_time: DateTime<Utc>,
    /// 피크 값
    pub peak_value: f64,
    /// 크기 (평균 대비 배수)
    pub magnitude: f64,
    /// 지속 시간
    pub duration: chrono::Duration,
}
