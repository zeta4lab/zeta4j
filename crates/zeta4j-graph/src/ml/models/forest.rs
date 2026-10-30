//! Random Forest
//!
//! 결정 트리 앙상블 분류기

use super::{Model, ModelType};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Random Forest 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomForestConfig {
    /// 트리 개수
    pub n_estimators: usize,
    /// 최대 깊이 (None = 제한 없음)
    pub max_depth: Option<usize>,
    /// 분할에 필요한 최소 샘플 수
    pub min_samples_split: usize,
    /// 리프 노드의 최소 샘플 수
    pub min_samples_leaf: usize,
    /// 분할 시 고려할 최대 피처 수 (sqrt = 피처 수의 제곱근)
    pub max_features: MaxFeatures,
    /// 부트스트랩 샘플링 사용
    pub bootstrap: bool,
    /// 랜덤 시드
    pub random_seed: Option<u64>,
}

impl Default for RandomForestConfig {
    fn default() -> Self {
        Self {
            n_estimators: 100,
            max_depth: None,
            min_samples_split: 2,
            min_samples_leaf: 1,
            max_features: MaxFeatures::Sqrt,
            bootstrap: true,
            random_seed: None,
        }
    }
}

/// 분할 시 고려할 피처 수
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MaxFeatures {
    /// 모든 피처
    All,
    /// 피처 수의 제곱근
    Sqrt,
    /// 피처 수의 로그
    Log2,
    /// 고정 개수
    Fixed(usize),
    /// 비율
    Fraction(f64),
}

impl MaxFeatures {
    fn compute(&self, n_features: usize) -> usize {
        match self {
            Self::All => n_features,
            Self::Sqrt => (n_features as f64).sqrt().ceil() as usize,
            Self::Log2 => (n_features as f64).log2().ceil() as usize,
            Self::Fixed(n) => (*n).min(n_features),
            Self::Fraction(f) => ((n_features as f64) * f).ceil() as usize,
        }
        .max(1)
    }
}

/// 결정 트리 노드
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TreeNode {
    /// 리프 노드 (예측값)
    Leaf {
        class: i64,
        probabilities: HashMap<i64, f64>,
    },
    /// 분할 노드
    Split {
        feature_idx: usize,
        threshold: f64,
        left: Box<TreeNode>,
        right: Box<TreeNode>,
    },
}

impl TreeNode {
    /// 예측
    fn predict(&self, x: &[f64]) -> i64 {
        match self {
            Self::Leaf { class, .. } => *class,
            Self::Split {
                feature_idx,
                threshold,
                left,
                right,
            } => {
                if x.get(*feature_idx).copied().unwrap_or(0.0) <= *threshold {
                    left.predict(x)
                } else {
                    right.predict(x)
                }
            }
        }
    }

    /// 예측 확률
    fn predict_proba(&self, x: &[f64]) -> &HashMap<i64, f64> {
        match self {
            Self::Leaf { probabilities, .. } => probabilities,
            Self::Split {
                feature_idx,
                threshold,
                left,
                right,
            } => {
                if x.get(*feature_idx).copied().unwrap_or(0.0) <= *threshold {
                    left.predict_proba(x)
                } else {
                    right.predict_proba(x)
                }
            }
        }
    }
}

/// 결정 트리
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTree {
    root: Option<TreeNode>,
    max_depth: Option<usize>,
    min_samples_split: usize,
    min_samples_leaf: usize,
    classes: Vec<i64>,
}

impl DecisionTree {
    /// 새 결정 트리 생성
    pub fn new(
        max_depth: Option<usize>,
        min_samples_split: usize,
        min_samples_leaf: usize,
    ) -> Self {
        Self {
            root: None,
            max_depth,
            min_samples_split,
            min_samples_leaf,
            classes: Vec::new(),
        }
    }

    /// 지니 불순도
    fn gini_impurity(labels: &[i64]) -> f64 {
        if labels.is_empty() {
            return 0.0;
        }

        let n = labels.len() as f64;
        let mut counts: HashMap<i64, usize> = HashMap::new();
        for &l in labels {
            *counts.entry(l).or_insert(0) += 1;
        }

        let mut gini = 1.0;
        for &count in counts.values() {
            let p = count as f64 / n;
            gini -= p * p;
        }
        gini
    }

    /// 최적 분할점 찾기
    fn find_best_split(
        features: &[Vec<f64>],
        labels: &[i64],
        feature_indices: &[usize],
    ) -> Option<(usize, f64, f64)> {
        let n = features.len();
        if n == 0 {
            return None;
        }

        let current_gini = Self::gini_impurity(labels);
        let mut best_gain = 0.0;
        let mut best_split = None;

        for &feature_idx in feature_indices {
            // 피처 값 정렬
            let mut indexed: Vec<(f64, i64)> = features
                .iter()
                .zip(labels.iter())
                .map(|(f, &l)| (f.get(feature_idx).copied().unwrap_or(0.0), l))
                .collect();
            indexed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

            // 각 분할점 시도
            for i in 1..indexed.len() {
                if (indexed[i].0 - indexed[i - 1].0).abs() < 1e-10 {
                    continue;
                }

                let threshold = (indexed[i].0 + indexed[i - 1].0) / 2.0;

                let left_labels: Vec<i64> = indexed[..i].iter().map(|(_, l)| *l).collect();
                let right_labels: Vec<i64> = indexed[i..].iter().map(|(_, l)| *l).collect();

                let n_left = left_labels.len() as f64;
                let n_right = right_labels.len() as f64;

                let weighted_gini = (n_left * Self::gini_impurity(&left_labels)
                    + n_right * Self::gini_impurity(&right_labels))
                    / n as f64;

                let gain = current_gini - weighted_gini;

                if gain > best_gain {
                    best_gain = gain;
                    best_split = Some((feature_idx, threshold, gain));
                }
            }
        }

        best_split
    }

    /// 재귀적 트리 구축
    #[expect(clippy::too_many_arguments)]
    fn build_tree(
        features: &[Vec<f64>],
        labels: &[i64],
        feature_indices: &[usize],
        depth: usize,
        max_depth: Option<usize>,
        min_samples_split: usize,
        min_samples_leaf: usize,
        classes: &[i64],
        rng: &mut ChaCha8Rng,
    ) -> TreeNode {
        let n = features.len();

        // 종료 조건
        let should_stop = n < min_samples_split
            || max_depth.is_some_and(|d| depth >= d)
            || Self::gini_impurity(labels) == 0.0;

        if should_stop {
            return Self::create_leaf(labels, classes);
        }

        // 랜덤 피처 선택
        let mut selected_features = feature_indices.to_vec();
        selected_features.shuffle(rng);
        let n_select = feature_indices.len().min(selected_features.len());
        selected_features.truncate(n_select);

        // 최적 분할 찾기
        if let Some((feature_idx, threshold, _)) =
            Self::find_best_split(features, labels, &selected_features)
        {
            // 분할
            let mut left_features = Vec::new();
            let mut left_labels = Vec::new();
            let mut right_features = Vec::new();
            let mut right_labels = Vec::new();

            for (f, &l) in features.iter().zip(labels.iter()) {
                if f.get(feature_idx).copied().unwrap_or(0.0) <= threshold {
                    left_features.push(f.clone());
                    left_labels.push(l);
                } else {
                    right_features.push(f.clone());
                    right_labels.push(l);
                }
            }

            // 최소 샘플 체크
            if left_labels.len() < min_samples_leaf || right_labels.len() < min_samples_leaf {
                return Self::create_leaf(labels, classes);
            }

            TreeNode::Split {
                feature_idx,
                threshold,
                left: Box::new(Self::build_tree(
                    &left_features,
                    &left_labels,
                    feature_indices,
                    depth + 1,
                    max_depth,
                    min_samples_split,
                    min_samples_leaf,
                    classes,
                    rng,
                )),
                right: Box::new(Self::build_tree(
                    &right_features,
                    &right_labels,
                    feature_indices,
                    depth + 1,
                    max_depth,
                    min_samples_split,
                    min_samples_leaf,
                    classes,
                    rng,
                )),
            }
        } else {
            Self::create_leaf(labels, classes)
        }
    }

    /// 리프 노드 생성
    fn create_leaf(labels: &[i64], classes: &[i64]) -> TreeNode {
        let n = labels.len() as f64;
        let mut counts: HashMap<i64, usize> = HashMap::new();
        for &l in labels {
            *counts.entry(l).or_insert(0) += 1;
        }

        let class = counts
            .iter()
            .max_by_key(|&(_, &c)| c)
            .map(|(&l, _)| l)
            .unwrap_or(0);

        let probabilities: HashMap<i64, f64> = classes
            .iter()
            .map(|&c| {
                let count = counts.get(&c).copied().unwrap_or(0);
                (c, count as f64 / n.max(1.0))
            })
            .collect();

        TreeNode::Leaf {
            class,
            probabilities,
        }
    }

    /// 학습
    pub fn fit(&mut self, features: &[Vec<f64>], labels: &[i64], seed: Option<u64>) {
        if features.is_empty() {
            return;
        }

        let n_features = features[0].len();
        let feature_indices: Vec<usize> = (0..n_features).collect();

        // 클래스 수집
        let mut classes: Vec<i64> = labels.to_vec();
        classes.sort();
        classes.dedup();
        self.classes = classes.clone();

        let mut rng = match seed {
            Some(s) => ChaCha8Rng::seed_from_u64(s),
            None => ChaCha8Rng::from_os_rng(),
        };

        self.root = Some(Self::build_tree(
            features,
            labels,
            &feature_indices,
            0,
            self.max_depth,
            self.min_samples_split,
            self.min_samples_leaf,
            &self.classes,
            &mut rng,
        ));
    }

    /// 예측
    pub fn predict(&self, x: &[f64]) -> i64 {
        self.root.as_ref().map_or(0, |r| r.predict(x))
    }

    /// 예측 확률
    pub fn predict_proba(&self, x: &[f64]) -> HashMap<i64, f64> {
        self.root
            .as_ref()
            .map_or(HashMap::new(), |r| r.predict_proba(x).clone())
    }
}

/// Random Forest 분류기
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomForest {
    config: RandomForestConfig,
    trees: Vec<DecisionTree>,
    classes: Vec<i64>,
    n_features: usize,
    is_fitted: bool,
}

impl RandomForest {
    /// 새 모델 생성
    pub fn new(config: RandomForestConfig) -> Self {
        Self {
            config,
            trees: Vec::new(),
            classes: Vec::new(),
            n_features: 0,
            is_fitted: false,
        }
    }

    /// 부트스트랩 샘플링
    fn bootstrap_sample<'a>(
        features: &'a [Vec<f64>],
        labels: &'a [i64],
        rng: &mut ChaCha8Rng,
    ) -> (Vec<Vec<f64>>, Vec<i64>) {
        let n = features.len();
        let mut sample_features = Vec::with_capacity(n);
        let mut sample_labels = Vec::with_capacity(n);

        for _ in 0..n {
            let idx = rng.random_range(0..n);
            sample_features.push(features[idx].clone());
            sample_labels.push(labels[idx]);
        }

        (sample_features, sample_labels)
    }
}

impl Default for RandomForest {
    fn default() -> Self {
        Self::new(RandomForestConfig::default())
    }
}

impl Model for RandomForest {
    fn fit(&mut self, features: &[Vec<f64>], labels: &[i64]) {
        if features.is_empty() || labels.is_empty() {
            return;
        }

        self.n_features = features[0].len();
        let n_features_to_use = self.config.max_features.compute(self.n_features);

        // 클래스 수집
        let mut classes: Vec<i64> = labels.to_vec();
        classes.sort();
        classes.dedup();
        self.classes = classes;

        let mut rng = match self.config.random_seed {
            Some(seed) => ChaCha8Rng::seed_from_u64(seed),
            None => ChaCha8Rng::from_os_rng(),
        };

        // 각 트리 학습
        self.trees = (0..self.config.n_estimators)
            .map(|_| {
                // 부트스트랩 샘플링
                let (sample_features, sample_labels) = if self.config.bootstrap {
                    Self::bootstrap_sample(features, labels, &mut rng)
                } else {
                    (features.to_vec(), labels.to_vec())
                };

                // 피처 서브셋 선택
                let mut feature_indices: Vec<usize> = (0..self.n_features).collect();
                feature_indices.shuffle(&mut rng);
                feature_indices.truncate(n_features_to_use);

                let mut tree = DecisionTree::new(
                    self.config.max_depth,
                    self.config.min_samples_split,
                    self.config.min_samples_leaf,
                );
                tree.classes = self.classes.clone();

                tree.root = Some(DecisionTree::build_tree(
                    &sample_features,
                    &sample_labels,
                    &feature_indices,
                    0,
                    self.config.max_depth,
                    self.config.min_samples_split,
                    self.config.min_samples_leaf,
                    &self.classes,
                    &mut rng,
                ));

                tree
            })
            .collect();

        self.is_fitted = true;
    }

    fn predict(&self, features: &[Vec<f64>]) -> Vec<i64> {
        if !self.is_fitted {
            return vec![0; features.len()];
        }

        features
            .iter()
            .map(|x| {
                // 다수결 투표
                let mut votes: HashMap<i64, usize> = HashMap::new();
                for tree in &self.trees {
                    let pred = tree.predict(x);
                    *votes.entry(pred).or_insert(0) += 1;
                }
                votes
                    .iter()
                    .max_by_key(|&(_, &count)| count)
                    .map(|(&class, _)| class)
                    .unwrap_or(0)
            })
            .collect()
    }

    fn predict_proba(&self, features: &[Vec<f64>]) -> Vec<Vec<f64>> {
        if !self.is_fitted {
            return vec![vec![]; features.len()];
        }

        features
            .iter()
            .map(|x| {
                // 평균 확률
                let mut avg_probs: HashMap<i64, f64> = HashMap::new();
                for tree in &self.trees {
                    let probs = tree.predict_proba(x);
                    for (&class, &prob) in &probs {
                        *avg_probs.entry(class).or_insert(0.0) += prob;
                    }
                }

                let n_trees = self.trees.len() as f64;
                self.classes
                    .iter()
                    .map(|&c| avg_probs.get(&c).copied().unwrap_or(0.0) / n_trees)
                    .collect()
            })
            .collect()
    }

    fn name(&self) -> &str {
        "RandomForest"
    }

    fn model_type(&self) -> ModelType {
        ModelType::RandomForest
    }

    fn to_bytes(&self) -> Vec<u8> {
        zeta4j_codec::serialize(self).unwrap_or_default()
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        zeta4j_codec::deserialize(bytes).ok()
    }
}
