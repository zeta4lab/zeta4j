// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 분류 평가 지표
//!
//! Accuracy, Precision, Recall, F1, AUC-ROC 등 평가 지표 구현

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 분류 평가 지표 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClassificationMetric {
    Accuracy,
    Precision,
    Recall,
    F1Score,
    F1Weighted,
    F1Macro,
    AucRoc,
    AucPr,
}

impl ClassificationMetric {
    /// 문자열에서 변환
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "ACCURACY" => Some(Self::Accuracy),
            "PRECISION" => Some(Self::Precision),
            "RECALL" => Some(Self::Recall),
            "F1" | "F1_SCORE" | "F1SCORE" => Some(Self::F1Score),
            "F1_WEIGHTED" => Some(Self::F1Weighted),
            "F1_MACRO" => Some(Self::F1Macro),
            "AUC" | "AUC_ROC" | "AUCROC" => Some(Self::AucRoc),
            "AUC_PR" | "AUCPR" => Some(Self::AucPr),
            _ => None,
        }
    }

    /// 이름 반환
    pub fn name(&self) -> &'static str {
        match self {
            Self::Accuracy => "accuracy",
            Self::Precision => "precision",
            Self::Recall => "recall",
            Self::F1Score => "f1",
            Self::F1Weighted => "f1_weighted",
            Self::F1Macro => "f1_macro",
            Self::AucRoc => "auc_roc",
            Self::AucPr => "auc_pr",
        }
    }
}

/// 이진 분류 지표
#[derive(Debug, Clone, Default)]
pub struct BinaryMetrics {
    pub true_positives: usize,
    pub true_negatives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
}

impl BinaryMetrics {
    /// 예측과 실제 레이블로부터 지표 계산
    pub fn from_predictions(y_true: &[i64], y_pred: &[i64], positive_label: i64) -> Self {
        let mut metrics = Self::default();

        for (&actual, &predicted) in y_true.iter().zip(y_pred.iter()) {
            if actual == positive_label {
                if predicted == positive_label {
                    metrics.true_positives += 1;
                } else {
                    metrics.false_negatives += 1;
                }
            } else if predicted == positive_label {
                metrics.false_positives += 1;
            } else {
                metrics.true_negatives += 1;
            }
        }

        metrics
    }

    /// 정확도
    pub fn accuracy(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        (self.true_positives + self.true_negatives) as f64 / total as f64
    }

    /// 정밀도 (Precision)
    pub fn precision(&self) -> f64 {
        let denominator = self.true_positives + self.false_positives;
        if denominator == 0 {
            return 0.0;
        }
        self.true_positives as f64 / denominator as f64
    }

    /// 재현율 (Recall)
    pub fn recall(&self) -> f64 {
        let denominator = self.true_positives + self.false_negatives;
        if denominator == 0 {
            return 0.0;
        }
        self.true_positives as f64 / denominator as f64
    }

    /// F1 Score
    pub fn f1(&self) -> f64 {
        let p = self.precision();
        let r = self.recall();
        if p + r == 0.0 {
            return 0.0;
        }
        2.0 * p * r / (p + r)
    }

    /// 전체 샘플 수
    pub fn total(&self) -> usize {
        self.true_positives + self.true_negatives + self.false_positives + self.false_negatives
    }

    /// 양성 클래스 샘플 수
    pub fn positive_count(&self) -> usize {
        self.true_positives + self.false_negatives
    }

    /// 음성 클래스 샘플 수
    pub fn negative_count(&self) -> usize {
        self.true_negatives + self.false_positives
    }
}

/// 분류 보고서 (다중 클래스 지원)
#[derive(Debug, Clone)]
pub struct ClassificationReport {
    /// 전체 정확도
    pub accuracy: f64,
    /// 클래스별 지표
    pub class_metrics: HashMap<i64, ClassMetrics>,
    /// Macro 평균 F1
    pub f1_macro: f64,
    /// Weighted 평균 F1
    pub f1_weighted: f64,
    /// Confusion matrix (optional)
    pub confusion_matrix: Option<Vec<Vec<usize>>>,
}

/// 클래스별 지표
#[derive(Debug, Clone)]
pub struct ClassMetrics {
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub support: usize,
}

impl ClassificationReport {
    /// 예측과 실제 레이블로부터 보고서 생성
    pub fn from_predictions(y_true: &[i64], y_pred: &[i64]) -> Self {
        if y_true.is_empty() || y_pred.is_empty() {
            return Self {
                accuracy: 0.0,
                class_metrics: HashMap::new(),
                f1_macro: 0.0,
                f1_weighted: 0.0,
                confusion_matrix: None,
            };
        }

        // 모든 클래스 수집
        let mut classes: Vec<i64> = y_true.iter().chain(y_pred.iter()).copied().collect();
        classes.sort();
        classes.dedup();

        // 클래스별 지표 계산
        let mut class_metrics = HashMap::new();
        let mut total_support = 0;

        for &class_label in &classes {
            let binary = BinaryMetrics::from_predictions(y_true, y_pred, class_label);
            let support = binary.positive_count();
            total_support += support;

            class_metrics.insert(
                class_label,
                ClassMetrics {
                    precision: binary.precision(),
                    recall: binary.recall(),
                    f1: binary.f1(),
                    support,
                },
            );
        }

        // 정확도 계산
        let correct = y_true
            .iter()
            .zip(y_pred.iter())
            .filter(|&(&a, &p)| a == p)
            .count();
        let accuracy = correct as f64 / y_true.len() as f64;

        // Macro F1 (단순 평균)
        let f1_macro = if classes.is_empty() {
            0.0
        } else {
            class_metrics.values().map(|m| m.f1).sum::<f64>() / classes.len() as f64
        };

        // Weighted F1 (support 가중 평균)
        let f1_weighted = if total_support == 0 {
            0.0
        } else {
            class_metrics
                .values()
                .map(|m| m.f1 * m.support as f64)
                .sum::<f64>()
                / total_support as f64
        };

        // Confusion matrix
        let confusion_matrix = Self::compute_confusion_matrix(y_true, y_pred, &classes);

        Self {
            accuracy,
            class_metrics,
            f1_macro,
            f1_weighted,
            confusion_matrix: Some(confusion_matrix),
        }
    }

    /// Confusion matrix 계산
    fn compute_confusion_matrix(
        y_true: &[i64],
        y_pred: &[i64],
        classes: &[i64],
    ) -> Vec<Vec<usize>> {
        let n = classes.len();
        let class_to_idx: HashMap<i64, usize> =
            classes.iter().enumerate().map(|(i, &c)| (c, i)).collect();

        let mut matrix = vec![vec![0usize; n]; n];

        for (&actual, &predicted) in y_true.iter().zip(y_pred.iter()) {
            if let (Some(&i), Some(&j)) = (class_to_idx.get(&actual), class_to_idx.get(&predicted))
            {
                matrix[i][j] += 1;
            }
        }

        matrix
    }

    /// 지표 값 조회
    pub fn get_metric(&self, metric: ClassificationMetric) -> f64 {
        match metric {
            ClassificationMetric::Accuracy => self.accuracy,
            ClassificationMetric::F1Macro => self.f1_macro,
            ClassificationMetric::F1Weighted => self.f1_weighted,
            // 이진 분류의 경우 첫 번째 양성 클래스 기준
            ClassificationMetric::Precision => self
                .class_metrics
                .values()
                .next()
                .map_or(0.0, |m| m.precision),
            ClassificationMetric::Recall => {
                self.class_metrics.values().next().map_or(0.0, |m| m.recall)
            }
            ClassificationMetric::F1Score => {
                self.class_metrics.values().next().map_or(0.0, |m| m.f1)
            }
            ClassificationMetric::AucRoc | ClassificationMetric::AucPr => {
                // AUC는 확률 필요 - 별도 함수로 계산
                0.0
            }
        }
    }
}

/// AUC-ROC 계산 (이진 분류)
///
/// # Arguments
/// * `y_true` - 실제 레이블 (0 또는 1)
/// * `y_prob` - 양성 클래스 확률
pub fn auc_roc(y_true: &[i64], y_prob: &[f64]) -> f64 {
    if y_true.len() != y_prob.len() || y_true.is_empty() {
        return 0.0;
    }

    // (확률, 레이블) 쌍으로 정렬 (확률 내림차순)
    let mut pairs: Vec<(f64, i64)> = y_prob
        .iter()
        .zip(y_true.iter())
        .map(|(&p, &l)| (p, l))
        .collect();
    pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // 양성/음성 개수
    let n_pos = y_true.iter().filter(|&&l| l == 1).count();
    let n_neg = y_true.len() - n_pos;

    if n_pos == 0 || n_neg == 0 {
        return 0.5; // 한 클래스만 있으면 0.5
    }

    // Trapezoidal rule로 AUC 계산
    let mut auc = 0.0;
    let mut tp = 0;
    let mut fp = 0;
    let mut prev_tpr = 0.0;
    let mut prev_fpr = 0.0;

    for (_, label) in &pairs {
        if *label == 1 {
            tp += 1;
        } else {
            fp += 1;
        }

        let tpr = tp as f64 / n_pos as f64;
        let fpr = fp as f64 / n_neg as f64;

        // Trapezoidal area
        auc += (fpr - prev_fpr) * (tpr + prev_tpr) / 2.0;

        prev_tpr = tpr;
        prev_fpr = fpr;
    }

    auc
}

/// AUC-PR (Precision-Recall AUC) 계산
pub fn auc_pr(y_true: &[i64], y_prob: &[f64]) -> f64 {
    if y_true.len() != y_prob.len() || y_true.is_empty() {
        return 0.0;
    }

    // (확률, 레이블) 쌍으로 정렬 (확률 내림차순)
    let mut pairs: Vec<(f64, i64)> = y_prob
        .iter()
        .zip(y_true.iter())
        .map(|(&p, &l)| (p, l))
        .collect();
    pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let n_pos = y_true.iter().filter(|&&l| l == 1).count();
    if n_pos == 0 {
        return 0.0;
    }

    let mut auc = 0.0;
    let mut tp = 0;
    let mut fp = 0;
    let mut prev_recall = 0.0;
    let mut prev_precision = 1.0;

    for (_, label) in &pairs {
        if *label == 1 {
            tp += 1;
        } else {
            fp += 1;
        }

        let recall = tp as f64 / n_pos as f64;
        let precision = tp as f64 / (tp + fp) as f64;

        // Trapezoidal area
        auc += (recall - prev_recall) * (precision + prev_precision) / 2.0;

        prev_recall = recall;
        prev_precision = precision;
    }

    auc
}
