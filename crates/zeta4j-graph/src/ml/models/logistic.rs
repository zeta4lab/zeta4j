// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Logistic Regression
//!
//! SGD 기반 로지스틱 회귀 분류기

use super::{Model, ModelType};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 로지스틱 회귀 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogisticConfig {
    /// 학습률
    pub learning_rate: f64,
    /// L2 정규화 계수
    pub penalty: f64,
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 허용 오차
    pub tolerance: f64,
    /// 랜덤 시드
    pub random_seed: Option<u64>,
    /// 배치 크기 (미니배치 SGD)
    pub batch_size: Option<usize>,
}

impl Default for LogisticConfig {
    fn default() -> Self {
        Self {
            learning_rate: 0.1,
            penalty: 0.01,
            max_iterations: 100,
            tolerance: 1e-4,
            random_seed: None,
            batch_size: None,
        }
    }
}

/// 로지스틱 회귀 분류기 (다중 클래스: One-vs-Rest)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogisticRegression {
    config: LogisticConfig,
    /// 클래스별 가중치 (class -> weights + bias)
    weights: HashMap<i64, Vec<f64>>,
    /// 클래스 목록
    classes: Vec<i64>,
    /// 피처 개수
    n_features: usize,
    /// 학습 여부
    is_fitted: bool,
}

impl LogisticRegression {
    /// 새 모델 생성
    pub fn new(config: LogisticConfig) -> Self {
        Self {
            config,
            weights: HashMap::new(),
            classes: Vec::new(),
            n_features: 0,
            is_fitted: false,
        }
    }

    /// 시그모이드 함수
    fn sigmoid(z: f64) -> f64 {
        1.0 / (1.0 + (-z).exp())
    }

    /// 내적 + bias
    fn linear(&self, x: &[f64], weights: &[f64]) -> f64 {
        let mut z = weights[self.n_features]; // bias
        for (i, &xi) in x.iter().enumerate() {
            if i < self.n_features {
                z += xi * weights[i];
            }
        }
        z
    }

    /// 단일 클래스에 대해 학습 (이진 분류기)
    fn fit_binary(&mut self, features: &[Vec<f64>], labels: &[i64], positive_class: i64) {
        let n_samples = features.len();
        if n_samples == 0 {
            return;
        }

        // RNG 초기화
        let mut rng = match self.config.random_seed {
            Some(seed) => ChaCha8Rng::seed_from_u64(seed),
            None => ChaCha8Rng::from_os_rng(),
        };

        // 가중치 초기화 (Xavier initialization)
        let init_scale = (2.0 / (self.n_features + 1) as f64).sqrt();
        let mut weights: Vec<f64> = (0..=self.n_features)
            .map(|_| rng.random::<f64>() * init_scale - init_scale / 2.0)
            .collect();

        // 이진 레이블 생성
        let binary_labels: Vec<f64> = labels
            .iter()
            .map(|&l| if l == positive_class { 1.0 } else { 0.0 })
            .collect();

        let batch_size = self.config.batch_size.unwrap_or(n_samples);
        let lr = self.config.learning_rate;
        let penalty = self.config.penalty;

        // SGD 학습
        for _ in 0..self.config.max_iterations {
            let mut total_loss = 0.0;

            // 샘플 셔플
            let mut indices: Vec<usize> = (0..n_samples).collect();
            indices.shuffle(&mut rng);

            // 미니배치 처리
            for batch_start in (0..n_samples).step_by(batch_size) {
                let batch_end = (batch_start + batch_size).min(n_samples);
                let batch_indices = &indices[batch_start..batch_end];

                // 그래디언트 누적
                let mut gradients = vec![0.0; self.n_features + 1];

                for &idx in batch_indices {
                    let x = &features[idx];
                    let y = binary_labels[idx];

                    // 예측
                    let z = self.linear(x, &weights);
                    let pred = Self::sigmoid(z);

                    // 손실 (Cross-entropy)
                    total_loss += -y * pred.ln() - (1.0 - y) * (1.0 - pred).ln();

                    // 그래디언트
                    let error = pred - y;
                    for (i, &xi) in x.iter().enumerate() {
                        if i < self.n_features {
                            gradients[i] += error * xi;
                        }
                    }
                    gradients[self.n_features] += error; // bias
                }

                // 가중치 업데이트
                let batch_len = batch_indices.len() as f64;
                for (i, weight) in weights.iter_mut().enumerate() {
                    let grad = gradients[i] / batch_len;
                    let reg = if i < self.n_features {
                        penalty * *weight
                    } else {
                        0.0
                    };
                    *weight -= lr * (grad + reg);
                }
            }

            // 수렴 체크
            let avg_loss = total_loss / n_samples as f64;
            if avg_loss < self.config.tolerance {
                break;
            }
        }

        self.weights.insert(positive_class, weights);
    }

    /// 단일 샘플 예측 확률 (각 클래스별)
    fn predict_proba_single(&self, x: &[f64]) -> Vec<f64> {
        let mut probs: Vec<f64> = self
            .classes
            .iter()
            .map(|&class| {
                if let Some(weights) = self.weights.get(&class) {
                    let z = self.linear(x, weights);
                    Self::sigmoid(z)
                } else {
                    0.5
                }
            })
            .collect();

        // Softmax 정규화 (합이 1이 되도록)
        let sum: f64 = probs.iter().sum();
        if sum > 0.0 {
            for p in &mut probs {
                *p /= sum;
            }
        }

        probs
    }
}

impl Default for LogisticRegression {
    fn default() -> Self {
        Self::new(LogisticConfig::default())
    }
}

impl Model for LogisticRegression {
    fn fit(&mut self, features: &[Vec<f64>], labels: &[i64]) {
        if features.is_empty() || labels.is_empty() {
            return;
        }

        self.n_features = features[0].len();

        // 클래스 수집
        let mut classes: Vec<i64> = labels.to_vec();
        classes.sort();
        classes.dedup();
        self.classes = classes;

        // One-vs-Rest: 각 클래스에 대해 이진 분류기 학습
        for &class in &self.classes.clone() {
            self.fit_binary(features, labels, class);
        }

        self.is_fitted = true;
    }

    fn predict(&self, features: &[Vec<f64>]) -> Vec<i64> {
        if !self.is_fitted {
            return vec![0; features.len()];
        }

        features
            .iter()
            .map(|x| {
                let probs = self.predict_proba_single(x);
                // 가장 높은 확률의 클래스 선택
                let max_idx = probs
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.classes.get(max_idx).copied().unwrap_or(0)
            })
            .collect()
    }

    fn predict_proba(&self, features: &[Vec<f64>]) -> Vec<Vec<f64>> {
        features
            .iter()
            .map(|x| self.predict_proba_single(x))
            .collect()
    }

    fn name(&self) -> &str {
        "LogisticRegression"
    }

    fn model_type(&self) -> ModelType {
        ModelType::LogisticRegression
    }

    fn to_bytes(&self) -> Vec<u8> {
        zeta4j_codec::serialize(self).unwrap_or_default()
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        zeta4j_codec::deserialize(bytes).ok()
    }
}
