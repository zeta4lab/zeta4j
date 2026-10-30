//! ARIMA 모델
//!
//! AutoRegressive Integrated Moving Average 모델과
//! 계절성을 지원하는 SARIMA 모델을 구현합니다.

use serde::{Deserialize, Serialize};

use super::{ModelError, PredictionModel, z_score_for_confidence};
use crate::tuning::forecasting::timeseries::{mean, std_dev};

/// ARIMA 모델 파라미터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArimaParams {
    /// 자기회귀 차수 (AR)
    pub p: usize,
    /// 차분 차수 (I)
    pub d: usize,
    /// 이동평균 차수 (MA)
    pub q: usize,
}

impl ArimaParams {
    pub fn new(p: usize, d: usize, q: usize) -> Self {
        Self { p, d, q }
    }
}

/// SARIMA 계절성 파라미터
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonalParams {
    /// 계절 자기회귀 차수
    pub P: usize,
    /// 계절 차분 차수
    pub D: usize,
    /// 계절 이동평균 차수
    pub Q: usize,
    /// 계절 주기
    pub s: usize,
}

impl SeasonalParams {
    #[allow(non_snake_case)]
    pub fn new(P: usize, D: usize, Q: usize, s: usize) -> Self {
        Self { P, D, Q, s }
    }
}

/// ARIMA 상태
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArimaState {
    /// AR 계수
    ar_coeffs: Vec<f64>,
    /// MA 계수
    ma_coeffs: Vec<f64>,
    /// 잔차
    residuals: Vec<f64>,
    /// 차분된 데이터
    diff_data: Vec<f64>,
    /// 원본 데이터 마지막 값들 (역차분용)
    last_values: Vec<f64>,
    /// 잔차 표준편차
    residual_std: f64,
}

/// ARIMA 모델
/// AutoRegressive Integrated Moving Average
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arima {
    /// 비계절성 파라미터
    params: ArimaParams,
    /// 계절성 파라미터 (SARIMA)
    seasonal: Option<SeasonalParams>,
    /// 학습된 상태
    state: Option<ArimaState>,
}

impl Arima {
    /// ARIMA(p, d, q) 모델 생성
    pub fn new(p: usize, d: usize, q: usize) -> Self {
        Self {
            params: ArimaParams::new(p, d, q),
            seasonal: None,
            state: None,
        }
    }

    /// SARIMA(p, d, q)(P, D, Q, s) 모델 생성
    #[allow(non_snake_case)]
    pub fn sarima(p: usize, d: usize, q: usize, P: usize, D: usize, Q: usize, s: usize) -> Self {
        Self {
            params: ArimaParams::new(p, d, q),
            seasonal: Some(SeasonalParams::new(P, D, Q, s)),
            state: None,
        }
    }

    /// d차 차분 수행
    pub fn difference(data: &[f64], d: usize) -> Vec<f64> {
        let mut result = data.to_vec();
        for _ in 0..d {
            let new_result: Vec<f64> = result.windows(2).map(|w| w[1] - w[0]).collect();
            result = new_result;
        }
        result
    }

    /// 계절 차분 수행
    #[allow(non_snake_case)]
    fn seasonal_difference(data: &[f64], D: usize, s: usize) -> Vec<f64> {
        let mut result = data.to_vec();
        for _ in 0..D {
            if result.len() <= s {
                break;
            }
            let new_result: Vec<f64> = result
                .iter()
                .skip(s)
                .enumerate()
                .map(|(i, &v)| v - result[i])
                .collect();
            result = new_result;
        }
        result
    }

    /// 역차분 수행
    pub fn inverse_difference(diff_data: &[f64], last_values: &[f64]) -> Vec<f64> {
        let mut result = diff_data.to_vec();
        for &last in last_values.iter().rev() {
            let mut integrated = vec![last];
            for &d in &result {
                let next = integrated.last().copied().unwrap_or(0.0) + d;
                integrated.push(next);
            }
            result = integrated[1..].to_vec();
        }
        result
    }

    /// AR 계수 추정 (Yule-Walker 방정식 간소화)
    fn estimate_ar_coeffs(data: &[f64], p: usize) -> Vec<f64> {
        if p == 0 || data.len() < p + 1 {
            return vec![];
        }

        // 자기상관 계산
        let n = data.len();
        let data_mean = mean(data);
        let centered: Vec<f64> = data.iter().map(|x| x - data_mean).collect();

        let mut acf = Vec::with_capacity(p + 1);
        for lag in 0..=p {
            let mut sum = 0.0;
            for i in lag..n {
                sum += centered[i] * centered[i - lag];
            }
            acf.push(sum / (n - lag) as f64);
        }

        // 정규화
        let var = acf[0];
        if var.abs() < 1e-10 {
            return vec![0.0; p];
        }
        for a in &mut acf {
            *a /= var;
        }

        // Durbin-Levinson 알고리즘으로 AR 계수 추정
        let mut coeffs = vec![0.0; p];
        let mut pacf = vec![0.0; p];

        pacf[0] = acf[1];
        coeffs[0] = acf[1];

        for k in 1..p {
            let mut num = acf[k + 1];
            let mut denom = 1.0;
            for j in 0..k {
                num -= pacf[j] * acf[k - j];
                denom -= pacf[j] * acf[j + 1];
            }
            pacf[k] = if denom.abs() > 1e-10 {
                num / denom
            } else {
                0.0
            };

            // 계수 업데이트
            let new_coeffs: Vec<f64> = (0..k)
                .map(|j| pacf[j] - pacf[k] * pacf[k - 1 - j])
                .collect();
            for (j, &c) in new_coeffs.iter().enumerate() {
                coeffs[j] = c;
            }
            coeffs[k] = pacf[k];
        }

        coeffs
    }

    /// MA 계수 추정 (혁신 알고리즘 간소화)
    fn estimate_ma_coeffs(residuals: &[f64], q: usize) -> Vec<f64> {
        if q == 0 || residuals.len() < q + 1 {
            return vec![];
        }

        // 잔차의 자기상관으로 MA 계수 근사
        let n = residuals.len();
        let res_mean = mean(residuals);
        let centered: Vec<f64> = residuals.iter().map(|x| x - res_mean).collect();

        let mut coeffs = vec![0.0; q];
        let var: f64 = centered.iter().map(|x| x.powi(2)).sum::<f64>() / n as f64;

        if var.abs() < 1e-10 {
            return coeffs;
        }

        for lag in 1..=q {
            let mut cov = 0.0;
            for i in lag..n {
                cov += centered[i] * centered[i - lag];
            }
            cov /= (n - lag) as f64;
            coeffs[lag - 1] = cov / var;
        }

        coeffs
    }
}

impl PredictionModel for Arima {
    fn train(&mut self, data: &[f64]) -> Result<(), ModelError> {
        let ArimaParams { p, d, q } = self.params.clone();
        let min_required = p.max(q) + d + 10;

        if data.len() < min_required {
            return Err(ModelError::InsufficientData {
                required: min_required,
                actual: data.len(),
            });
        }

        // 마지막 값 저장 (역차분용)
        let mut last_values = Vec::with_capacity(d);
        let mut temp_data = data.to_vec();
        for _ in 0..d {
            if let Some(&last) = temp_data.last() {
                last_values.push(last);
            }
            temp_data = Self::difference(&temp_data, 1);
        }

        // 계절 차분 (SARIMA인 경우)
        let diff_data = if let Some(ref seasonal) = self.seasonal {
            Self::seasonal_difference(&temp_data, seasonal.D, seasonal.s)
        } else {
            temp_data
        };

        // AR 계수 추정
        let ar_coeffs = Self::estimate_ar_coeffs(&diff_data, p);

        // AR 잔차 계산
        let mut residuals = Vec::with_capacity(diff_data.len());
        for i in 0..diff_data.len() {
            let mut pred = 0.0;
            for j in 0..p.min(i) {
                pred += ar_coeffs.get(j).unwrap_or(&0.0) * diff_data[i - j - 1];
            }
            residuals.push(diff_data[i] - pred);
        }

        // MA 계수 추정
        let ma_coeffs = Self::estimate_ma_coeffs(&residuals, q);

        let residual_std = std_dev(&residuals);

        self.state = Some(ArimaState {
            ar_coeffs,
            ma_coeffs,
            residuals,
            diff_data,
            last_values,
            residual_std,
        });

        Ok(())
    }

    fn predict(&self, horizon: usize) -> Result<Vec<f64>, ModelError> {
        let state = self.state.as_ref().ok_or(ModelError::NotTrained)?;
        let ArimaParams { p, d, q } = &self.params;

        let mut predictions = Vec::with_capacity(horizon);
        let mut extended_data = state.diff_data.clone();
        let mut extended_residuals = state.residuals.clone();

        for _ in 0..horizon {
            // AR 부분
            let mut pred = 0.0;
            let n = extended_data.len();
            for j in 0..*p {
                if n > j {
                    pred += state.ar_coeffs.get(j).unwrap_or(&0.0) * extended_data[n - j - 1];
                }
            }

            // MA 부분
            let m = extended_residuals.len();
            for j in 0..*q {
                if m > j {
                    pred += state.ma_coeffs.get(j).unwrap_or(&0.0) * extended_residuals[m - j - 1];
                }
            }

            predictions.push(pred);
            extended_data.push(pred);
            extended_residuals.push(0.0); // 미래 잔차는 0으로 가정
        }

        // 역차분 적용
        if *d > 0 {
            predictions = Self::inverse_difference(&predictions, &state.last_values);
        }

        Ok(predictions)
    }

    fn predict_interval(
        &self,
        horizon: usize,
        confidence: f64,
    ) -> Result<Vec<(f64, f64)>, ModelError> {
        let forecasts = self.predict(horizon)?;
        let state = self.state.as_ref().ok_or(ModelError::NotTrained)?;

        let z = z_score_for_confidence(confidence);

        let mut intervals = Vec::with_capacity(horizon);
        for (h, forecast) in forecasts.iter().enumerate() {
            let error_factor = ((h + 1) as f64).sqrt();
            let margin = z * state.residual_std * error_factor;
            intervals.push((forecast - margin, forecast + margin));
        }

        Ok(intervals)
    }

    fn name(&self) -> &str {
        if self.seasonal.is_some() {
            "SARIMA"
        } else {
            "ARIMA"
        }
    }

    fn is_trained(&self) -> bool {
        self.state.is_some()
    }
}
