//! 시계열 분석
//!
//! STL 분해, 자기상관, 주기 탐지, 이상치 탐지 등
//! 시계열 데이터 분석 기능을 제공합니다.

use std::collections::VecDeque;

/// 시계열 분해 결과
#[derive(Debug, Clone)]
pub struct DecomposedSeries {
    /// 트렌드 성분 (장기 추세)
    pub trend: Vec<f64>,
    /// 계절성 성분 (주기적 패턴)
    pub seasonal: Vec<f64>,
    /// 잔차 (노이즈)
    pub residual: Vec<f64>,
    /// 원본 데이터
    pub original: Vec<f64>,
    /// 주기 길이
    pub period: usize,
}

impl DecomposedSeries {
    /// 계절성 강도 계산 (0.0 ~ 1.0)
    pub fn seasonal_strength(&self) -> f64 {
        let residual_var = variance(&self.residual);
        let seasonal_residual: Vec<f64> = self
            .seasonal
            .iter()
            .zip(&self.residual)
            .map(|(s, r)| s + r)
            .collect();
        let sr_var = variance(&seasonal_residual);

        if sr_var == 0.0 {
            0.0
        } else {
            (1.0 - residual_var / sr_var).max(0.0)
        }
    }

    /// 트렌드 강도 계산 (0.0 ~ 1.0)
    pub fn trend_strength(&self) -> f64 {
        let residual_var = variance(&self.residual);
        let trend_residual: Vec<f64> = self
            .trend
            .iter()
            .zip(&self.residual)
            .map(|(t, r)| t + r)
            .collect();
        let tr_var = variance(&trend_residual);

        if tr_var == 0.0 {
            0.0
        } else {
            (1.0 - residual_var / tr_var).max(0.0)
        }
    }
}

/// 분해 방식
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecompositionMethod {
    /// 가산 모델 (Y = T + S + R)
    Additive,
    /// 승산 모델 (Y = T * S * R)
    Multiplicative,
}

/// 이상치 탐지 방법
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnomalyMethod {
    /// IQR 방법 (사분위수 범위)
    IQR { multiplier: f64 },
    /// Z-Score 방법
    ZScore { threshold: f64 },
    /// Modified Z-Score (MAD 기반)
    ModifiedZScore { threshold: f64 },
    /// 이동 표준편차
    RollingStd { window: usize, threshold: f64 },
}

impl Default for AnomalyMethod {
    fn default() -> Self {
        AnomalyMethod::IQR { multiplier: 1.5 }
    }
}

/// 정상성 테스트 결과
#[derive(Debug, Clone)]
pub struct StationarityResult {
    /// ADF 통계량
    pub adf_statistic: f64,
    /// p-값 (추정)
    pub p_value: f64,
    /// 정상성 여부 (p < 0.05)
    pub is_stationary: bool,
    /// 사용된 시차
    pub lags_used: usize,
    /// 임계값들
    pub critical_values: CriticalValues,
}

/// ADF 테스트 임계값
#[derive(Debug, Clone)]
pub struct CriticalValues {
    pub pct_1: f64,
    pub pct_5: f64,
    pub pct_10: f64,
}

/// 시계열 분석기 설정
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// 기본 분해 방식
    pub decomposition_method: DecompositionMethod,
    /// 이상치 탐지 방법
    pub anomaly_method: AnomalyMethod,
    /// LOESS 스무딩 윈도우
    pub loess_window: usize,
    /// 최대 자기상관 시차
    pub max_lag: usize,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            decomposition_method: DecompositionMethod::Additive,
            anomaly_method: AnomalyMethod::IQR { multiplier: 1.5 },
            loess_window: 7,
            max_lag: 50,
        }
    }
}

/// 시계열 분석기
pub struct TimeSeriesAnalyzer {
    config: AnalyzerConfig,
}

impl TimeSeriesAnalyzer {
    /// 새 분석기 생성
    pub fn new(config: AnalyzerConfig) -> Self {
        Self { config }
    }

    /// STL 분해 수행
    ///
    /// Seasonal-Trend decomposition using LOESS 간소화 구현
    ///
    /// # Arguments
    /// * `data` - 시계열 데이터
    /// * `period` - 계절 주기 (예: 24 for 시간별 데이터의 일별 패턴)
    pub fn decompose(&self, data: &[f64], period: usize) -> DecomposedSeries {
        if data.len() < period * 2 {
            // 데이터가 너무 짧으면 빈 분해 결과 반환
            return DecomposedSeries {
                trend: data.to_vec(),
                seasonal: vec![0.0; data.len()],
                residual: vec![0.0; data.len()],
                original: data.to_vec(),
                period,
            };
        }

        match self.config.decomposition_method {
            DecompositionMethod::Additive => self.decompose_additive(data, period),
            DecompositionMethod::Multiplicative => self.decompose_multiplicative(data, period),
        }
    }

    /// 가산 분해 (Y = T + S + R)
    fn decompose_additive(&self, data: &[f64], period: usize) -> DecomposedSeries {
        let n = data.len();

        // 1. 이동평균으로 트렌드 추출
        let trend = self.moving_average(data, period);

        // 2. 디트렌드 (원본 - 트렌드)
        let detrended: Vec<f64> = data.iter().zip(&trend).map(|(d, t)| d - t).collect();

        // 3. 계절성 추출 (각 주기 위치의 평균)
        let mut seasonal = vec![0.0; n];
        let mut seasonal_avg = vec![0.0; period];
        let mut counts = vec![0usize; period];

        for (i, val) in detrended.iter().enumerate() {
            let pos = i % period;
            if val.is_finite() {
                seasonal_avg[pos] += val;
                counts[pos] += 1;
            }
        }

        for i in 0..period {
            if counts[i] > 0 {
                seasonal_avg[i] /= counts[i] as f64;
            }
        }

        // 계절성 평균을 0으로 조정
        let seasonal_mean: f64 = seasonal_avg.iter().sum::<f64>() / period as f64;
        for avg in &mut seasonal_avg {
            *avg -= seasonal_mean;
        }

        // 전체 시계열에 적용
        for i in 0..n {
            seasonal[i] = seasonal_avg[i % period];
        }

        // 4. 잔차 계산 (원본 - 트렌드 - 계절성)
        let residual: Vec<f64> = data
            .iter()
            .zip(&trend)
            .zip(&seasonal)
            .map(|((d, t), s)| d - t - s)
            .collect();

        DecomposedSeries {
            trend,
            seasonal,
            residual,
            original: data.to_vec(),
            period,
        }
    }

    /// 승산 분해 (Y = T * S * R)
    fn decompose_multiplicative(&self, data: &[f64], period: usize) -> DecomposedSeries {
        let _n = data.len();

        // 로그 변환 후 가산 분해
        let log_data: Vec<f64> = data.iter().map(|x| x.max(1e-10).ln()).collect();

        let decomposed = self.decompose_additive(&log_data, period);

        // 지수 변환으로 복원
        let trend: Vec<f64> = decomposed.trend.iter().map(|x| x.exp()).collect();
        let seasonal: Vec<f64> = decomposed.seasonal.iter().map(|x| x.exp()).collect();
        let residual: Vec<f64> = decomposed.residual.iter().map(|x| x.exp()).collect();

        DecomposedSeries {
            trend,
            seasonal,
            residual,
            original: data.to_vec(),
            period,
        }
    }

    /// 이동평균 계산 (중심화)
    #[expect(clippy::needless_range_loop)]
    fn moving_average(&self, data: &[f64], window: usize) -> Vec<f64> {
        let n = data.len();
        let half = window / 2;
        let mut result = vec![0.0; n];

        for i in 0..n {
            let start = i.saturating_sub(half);
            let end = (i + half + 1).min(n);

            let sum: f64 = data[start..end].iter().filter(|x| x.is_finite()).sum();
            let count = data[start..end].iter().filter(|x| x.is_finite()).count();

            result[i] = if count > 0 { sum / count as f64 } else { 0.0 };
        }

        result
    }

    /// 자기상관 함수 (ACF) 계산
    ///
    /// # Arguments
    /// * `data` - 시계열 데이터
    /// * `max_lag` - 최대 시차
    ///
    /// # Returns
    /// 시차 0부터 max_lag까지의 자기상관 계수
    pub fn autocorrelation(&self, data: &[f64], max_lag: usize) -> Vec<f64> {
        let n = data.len();
        let max_lag = max_lag.min(n - 1);
        let mean = mean(data);
        let var = variance(data);

        if var == 0.0 {
            return vec![1.0; max_lag + 1];
        }

        let mut acf = vec![0.0; max_lag + 1];
        acf[0] = 1.0; // 시차 0은 항상 1

        for lag in 1..=max_lag {
            let mut sum = 0.0;
            for i in 0..(n - lag) {
                sum += (data[i] - mean) * (data[i + lag] - mean);
            }
            acf[lag] = sum / ((n - lag) as f64 * var);
        }

        acf
    }

    /// 편자기상관 함수 (PACF) 계산
    ///
    /// Durbin-Levinson 알고리즘 사용
    pub fn partial_autocorrelation(&self, data: &[f64], max_lag: usize) -> Vec<f64> {
        let acf = self.autocorrelation(data, max_lag);
        let n = acf.len();

        if n <= 1 {
            return acf;
        }

        let mut pacf = vec![0.0; n];
        pacf[0] = 1.0;
        pacf[1] = acf[1];

        // Durbin-Levinson recursion
        let mut phi = vec![vec![0.0; n]; n];
        phi[1][1] = acf[1];

        for k in 2..n {
            let mut num = acf[k];
            let mut den = 1.0;

            for j in 1..k {
                num -= phi[k - 1][j] * acf[k - j];
                den -= phi[k - 1][j] * acf[j];
            }

            if den.abs() < 1e-10 {
                phi[k][k] = 0.0;
            } else {
                phi[k][k] = num / den;
            }

            for j in 1..k {
                phi[k][j] = phi[k - 1][j] - phi[k][k] * phi[k - 1][k - j];
            }

            pacf[k] = phi[k][k];
        }

        pacf
    }

    /// 주기 자동 탐지
    ///
    /// ACF에서 가장 강한 주기 패턴을 찾습니다.
    ///
    /// # Arguments
    /// * `data` - 시계열 데이터
    /// * `min_period` - 최소 주기 (기본: 2)
    /// * `max_period` - 최대 주기 (기본: 데이터 길이 / 3)
    pub fn detect_period(
        &self,
        data: &[f64],
        min_period: Option<usize>,
        max_period: Option<usize>,
    ) -> Option<usize> {
        let n = data.len();
        let min_p = min_period.unwrap_or(2);
        let max_p = max_period.unwrap_or(n / 3).min(n / 2);

        if max_p <= min_p || n < min_p * 3 {
            return None;
        }

        let acf = self.autocorrelation(data, max_p);

        // ACF에서 피크 찾기
        let mut best_period = None;
        let mut best_acf = 0.0;

        for lag in min_p..=max_p {
            // 로컬 최대값이고 양수인 경우
            if acf[lag] > best_acf
                && acf[lag] > 0.1 // 최소 상관계수
                && (lag < 2 || acf[lag] > acf[lag - 1])
                && (lag >= max_p || acf[lag] > acf[lag + 1])
            {
                best_acf = acf[lag];
                best_period = Some(lag);
            }
        }

        best_period
    }

    /// 이상치 탐지
    ///
    /// # Returns
    /// 이상치 인덱스와 해당 값의 벡터
    pub fn detect_anomalies(&self, data: &[f64]) -> Vec<(usize, f64)> {
        match self.config.anomaly_method {
            AnomalyMethod::IQR { multiplier } => self.detect_anomalies_iqr(data, multiplier),
            AnomalyMethod::ZScore { threshold } => self.detect_anomalies_zscore(data, threshold),
            AnomalyMethod::ModifiedZScore { threshold } => {
                self.detect_anomalies_modified_zscore(data, threshold)
            }
            AnomalyMethod::RollingStd { window, threshold } => {
                self.detect_anomalies_rolling(data, window, threshold)
            }
        }
    }

    /// IQR 방법으로 이상치 탐지
    fn detect_anomalies_iqr(&self, data: &[f64], multiplier: f64) -> Vec<(usize, f64)> {
        let mut sorted: Vec<f64> = data.iter().filter(|x| x.is_finite()).cloned().collect();
        if sorted.len() < 4 {
            return vec![];
        }

        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let q1 = percentile(&sorted, 25.0);
        let q3 = percentile(&sorted, 75.0);
        let iqr = q3 - q1;

        let lower = q1 - multiplier * iqr;
        let upper = q3 + multiplier * iqr;

        data.iter()
            .enumerate()
            .filter(|(_, v)| **v < lower || **v > upper)
            .map(|(i, v)| (i, *v))
            .collect()
    }

    /// Z-Score 방법으로 이상치 탐지
    fn detect_anomalies_zscore(&self, data: &[f64], threshold: f64) -> Vec<(usize, f64)> {
        let m = mean(data);
        let s = std_dev(data);

        if s == 0.0 {
            return vec![];
        }

        data.iter()
            .enumerate()
            .filter(|(_, v)| ((*v - m) / s).abs() > threshold)
            .map(|(i, v)| (i, *v))
            .collect()
    }

    /// Modified Z-Score (MAD 기반) 방법으로 이상치 탐지
    fn detect_anomalies_modified_zscore(&self, data: &[f64], threshold: f64) -> Vec<(usize, f64)> {
        let med = median(data);
        let mad = median_absolute_deviation(data);

        if mad == 0.0 {
            return vec![];
        }

        // Modified Z-Score = 0.6745 * (x - median) / MAD
        let k = 0.6745;

        data.iter()
            .enumerate()
            .filter(|(_, v)| (k * (*v - med) / mad).abs() > threshold)
            .map(|(i, v)| (i, *v))
            .collect()
    }

    /// 이동 표준편차 기반 이상치 탐지
    fn detect_anomalies_rolling(
        &self,
        data: &[f64],
        window: usize,
        threshold: f64,
    ) -> Vec<(usize, f64)> {
        let n = data.len();
        if n < window {
            return vec![];
        }

        let mut anomalies = vec![];
        let mut window_data: VecDeque<f64> = VecDeque::with_capacity(window);

        for (i, &val) in data.iter().enumerate() {
            if window_data.len() >= window {
                window_data.pop_front();
            }
            window_data.push_back(val);

            if window_data.len() == window {
                let slice: Vec<f64> = window_data.iter().cloned().collect();
                let m = mean(&slice);
                let s = std_dev(&slice);

                if s > 0.0 && ((val - m) / s).abs() > threshold {
                    anomalies.push((i, val));
                }
            }
        }

        anomalies
    }

    /// 정상성 테스트 (간소화된 ADF 테스트)
    ///
    /// Augmented Dickey-Fuller 테스트의 간소화 버전
    pub fn test_stationarity(&self, data: &[f64]) -> StationarityResult {
        let n = data.len();
        if n < 10 {
            return StationarityResult {
                adf_statistic: 0.0,
                p_value: 1.0,
                is_stationary: false,
                lags_used: 0,
                critical_values: CriticalValues {
                    pct_1: -3.43,
                    pct_5: -2.86,
                    pct_10: -2.57,
                },
            };
        }

        // 시차 선택 (간단한 규칙: 12 * (n / 100)^0.25)
        let lags = ((12.0 * (n as f64 / 100.0).powf(0.25)) as usize)
            .max(1)
            .min(n / 4);

        // 1차 차분
        let diff: Vec<f64> = data.windows(2).map(|w| w[1] - w[0]).collect();

        // ADF 회귀: Δy_t = α + β*y_{t-1} + Σ(γ_i * Δy_{t-i}) + ε_t
        // 간소화: β 계수의 t-통계량 계산

        // y_{t-1} 과 Δy_t의 상관관계로 근사
        let y_lag: Vec<f64> = data[..n - 1].to_vec();
        let y_diff: Vec<f64> = diff.clone();

        let mean_lag = mean(&y_lag);
        let mean_diff = mean(&y_diff);
        let std_lag = std_dev(&y_lag);
        let std_diff = std_dev(&y_diff);

        let correlation = if std_lag > 0.0 && std_diff > 0.0 {
            let cov: f64 = y_lag
                .iter()
                .zip(&y_diff)
                .map(|(l, d)| (l - mean_lag) * (d - mean_diff))
                .sum::<f64>()
                / (n - 1) as f64;
            cov / (std_lag * std_diff)
        } else {
            0.0
        };

        // 근사 t-통계량
        let t_stat =
            correlation * ((n - 2) as f64).sqrt() / (1.0 - correlation * correlation).sqrt();

        // 간소화된 임계값 (MacKinnon 근사)
        let critical_values = CriticalValues {
            pct_1: -3.43,
            pct_5: -2.86,
            pct_10: -2.57,
        };

        // p-값 근사 (t-분포 기반 간소화)
        let p_value = self.approximate_adf_pvalue(t_stat, n);

        StationarityResult {
            adf_statistic: t_stat,
            p_value,
            is_stationary: p_value < 0.05,
            lags_used: lags,
            critical_values,
        }
    }

    /// ADF p-값 근사 (간소화)
    fn approximate_adf_pvalue(&self, t_stat: f64, _n: usize) -> f64 {
        // 간소화된 p-값 근사
        if t_stat < -3.43 {
            0.01 // p < 0.01
        } else if t_stat < -2.86 {
            0.05 // 0.01 <= p < 0.05
        } else if t_stat < -2.57 {
            0.10 // 0.05 <= p < 0.10
        } else if t_stat < -1.94 {
            0.25
        } else if t_stat < -1.62 {
            0.50
        } else {
            0.90 // p > 0.50
        }
    }

    /// 차분 (differencing)
    pub fn difference(&self, data: &[f64], order: usize) -> Vec<f64> {
        let mut result = data.to_vec();
        for _ in 0..order {
            if result.len() < 2 {
                break;
            }
            result = result.windows(2).map(|w| w[1] - w[0]).collect();
        }
        result
    }

    /// 계절 차분
    pub fn seasonal_difference(&self, data: &[f64], period: usize) -> Vec<f64> {
        if data.len() <= period {
            return vec![];
        }
        data.iter()
            .skip(period)
            .zip(data.iter())
            .map(|(a, b)| a - b)
            .collect()
    }

    /// 지수 평활 (단순)
    pub fn exponential_smoothing(&self, data: &[f64], alpha: f64) -> Vec<f64> {
        if data.is_empty() {
            return vec![];
        }

        let alpha = alpha.clamp(0.0, 1.0);
        let mut result = vec![data[0]];
        let mut previous = data[0];

        for &val in data.iter().skip(1) {
            let smoothed = alpha * val + (1.0 - alpha) * previous;
            result.push(smoothed);
            previous = smoothed;
        }

        result
    }

    /// 이중 지수 평활 (Holt)
    pub fn double_exponential_smoothing(
        &self,
        data: &[f64],
        alpha: f64,
        beta: f64,
    ) -> (Vec<f64>, Vec<f64>) {
        if data.len() < 2 {
            return (data.to_vec(), vec![0.0; data.len()]);
        }

        let alpha = alpha.clamp(0.0, 1.0);
        let beta = beta.clamp(0.0, 1.0);

        let mut level = vec![data[0]];
        let mut trend = vec![data[1] - data[0]];

        for i in 1..data.len() {
            let new_level = alpha * data[i] + (1.0 - alpha) * (level[i - 1] + trend[i - 1]);
            let new_trend = beta * (new_level - level[i - 1]) + (1.0 - beta) * trend[i - 1];
            level.push(new_level);
            trend.push(new_trend);
        }

        (level, trend)
    }
}

impl Default for TimeSeriesAnalyzer {
    fn default() -> Self {
        Self::new(AnalyzerConfig::default())
    }
}

// ============================================================================
// 통계 유틸리티 함수
// ============================================================================

/// 평균
pub fn mean(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let valid: Vec<f64> = data.iter().filter(|x| x.is_finite()).cloned().collect();
    if valid.is_empty() {
        return 0.0;
    }
    valid.iter().sum::<f64>() / valid.len() as f64
}

/// 분산
pub fn variance(data: &[f64]) -> f64 {
    if data.len() < 2 {
        return 0.0;
    }
    let m = mean(data);
    let sum_sq: f64 = data
        .iter()
        .filter(|x| x.is_finite())
        .map(|x| (x - m).powi(2))
        .sum();
    let count = data.iter().filter(|x| x.is_finite()).count();
    if count < 2 {
        return 0.0;
    }
    sum_sq / (count - 1) as f64
}

/// 표준편차
pub fn std_dev(data: &[f64]) -> f64 {
    variance(data).sqrt()
}

/// 중앙값
pub fn median(data: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = data.iter().filter(|x| x.is_finite()).cloned().collect();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

/// 중앙값 절대 편차 (MAD)
pub fn median_absolute_deviation(data: &[f64]) -> f64 {
    let med = median(data);
    let deviations: Vec<f64> = data.iter().map(|x| (x - med).abs()).collect();
    median(&deviations)
}

/// 백분위수
pub fn percentile(sorted_data: &[f64], p: f64) -> f64 {
    if sorted_data.is_empty() {
        return 0.0;
    }
    let n = sorted_data.len();
    let idx = (p / 100.0) * (n - 1) as f64;
    let lower = idx.floor() as usize;
    let upper = idx.ceil() as usize;

    if lower == upper || upper >= n {
        sorted_data[lower.min(n - 1)]
    } else {
        let frac = idx - lower as f64;
        sorted_data[lower] * (1.0 - frac) + sorted_data[upper] * frac
    }
}

/// 공분산
pub fn covariance(x: &[f64], y: &[f64]) -> f64 {
    if x.len() != y.len() || x.len() < 2 {
        return 0.0;
    }
    let mean_x = mean(x);
    let mean_y = mean(y);
    let n = x.len();

    x.iter()
        .zip(y.iter())
        .map(|(xi, yi)| (xi - mean_x) * (yi - mean_y))
        .sum::<f64>()
        / (n - 1) as f64
}

/// 피어슨 상관계수
pub fn correlation(x: &[f64], y: &[f64]) -> f64 {
    let cov = covariance(x, y);
    let std_x = std_dev(x);
    let std_y = std_dev(y);

    if std_x == 0.0 || std_y == 0.0 {
        0.0
    } else {
        cov / (std_x * std_y)
    }
}
