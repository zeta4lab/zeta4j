//! 훈련/테스트 데이터 분할
//!
//! Stratified sampling을 지원하는 train/test/validation 분할

use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

/// 분할 설정
#[derive(Debug, Clone)]
pub struct TrainTestSplitConfig {
    /// 테스트 데이터 비율 (0.0 ~ 1.0)
    pub test_fraction: f64,
    /// 검증 데이터 비율 (0.0 ~ 1.0, optional)
    pub validation_fraction: f64,
    /// 층화 샘플링 사용 여부
    pub stratify: bool,
    /// 랜덤 시드 (재현성)
    pub random_seed: Option<u64>,
}

impl Default for TrainTestSplitConfig {
    fn default() -> Self {
        Self {
            test_fraction: 0.2,
            validation_fraction: 0.0,
            stratify: true,
            random_seed: None,
        }
    }
}

/// 분할 결과
#[derive(Debug, Clone)]
pub struct SplitResult<T> {
    /// 훈련 데이터
    pub train: Vec<T>,
    /// 테스트 데이터
    pub test: Vec<T>,
    /// 검증 데이터 (optional)
    pub validation: Option<Vec<T>>,
}

impl<T> SplitResult<T> {
    /// 훈련 데이터 크기
    pub fn train_size(&self) -> usize {
        self.train.len()
    }

    /// 테스트 데이터 크기
    pub fn test_size(&self) -> usize {
        self.test.len()
    }

    /// 검증 데이터 크기
    pub fn validation_size(&self) -> usize {
        self.validation.as_ref().map_or(0, |v| v.len())
    }
}

/// Train/Test/Validation 분할기
pub struct TrainTestSplit;

impl TrainTestSplit {
    /// 데이터를 훈련/테스트로 분할
    ///
    /// # Arguments
    /// * `data` - 분할할 데이터 (인덱스 또는 ID)
    /// * `labels` - 각 데이터의 레이블 (stratified sampling용)
    /// * `config` - 분할 설정
    ///
    /// # Returns
    /// 분할된 데이터 인덱스
    pub fn split<T: Clone>(
        data: &[T],
        labels: Option<&[i64]>,
        config: &TrainTestSplitConfig,
    ) -> SplitResult<T> {
        let n = data.len();
        if n == 0 {
            return SplitResult {
                train: vec![],
                test: vec![],
                validation: None,
            };
        }

        // RNG 초기화
        let mut rng = match config.random_seed {
            Some(seed) => ChaCha8Rng::seed_from_u64(seed),
            None => ChaCha8Rng::from_os_rng(),
        };

        // 인덱스 생성
        let mut indices: Vec<usize> = (0..n).collect();

        if let (true, Some(labels)) = (config.stratify, labels) {
            // 층화 샘플링
            Self::stratified_split(data, labels, &mut indices, &mut rng, config)
        } else {
            // 단순 랜덤 셔플
            indices.shuffle(&mut rng);
            Self::simple_split(data, &indices, config)
        }
    }

    /// 단순 분할 (셔플된 인덱스 사용)
    fn simple_split<T: Clone>(
        data: &[T],
        indices: &[usize],
        config: &TrainTestSplitConfig,
    ) -> SplitResult<T> {
        let n = data.len();
        let test_size = (n as f64 * config.test_fraction).round() as usize;
        let validation_size = (n as f64 * config.validation_fraction).round() as usize;
        let train_size = n - test_size - validation_size;

        let train: Vec<T> = indices[..train_size]
            .iter()
            .map(|&i| data[i].clone())
            .collect();

        let test: Vec<T> = indices[train_size..train_size + test_size]
            .iter()
            .map(|&i| data[i].clone())
            .collect();

        let validation = if validation_size > 0 {
            Some(
                indices[train_size + test_size..]
                    .iter()
                    .map(|&i| data[i].clone())
                    .collect(),
            )
        } else {
            None
        };

        SplitResult {
            train,
            test,
            validation,
        }
    }

    /// 층화 분할 (각 클래스의 비율 유지)
    fn stratified_split<T: Clone>(
        data: &[T],
        labels: &[i64],
        _indices: &mut [usize],
        rng: &mut ChaCha8Rng,
        config: &TrainTestSplitConfig,
    ) -> SplitResult<T> {
        // 클래스별 인덱스 그룹화
        let mut class_indices: HashMap<i64, Vec<usize>> = HashMap::new();
        for (i, &label) in labels.iter().enumerate() {
            class_indices.entry(label).or_default().push(i);
        }

        // 각 클래스별로 셔플
        for indices in class_indices.values_mut() {
            indices.shuffle(rng);
        }

        let mut train = Vec::new();
        let mut test = Vec::new();
        let mut validation = Vec::new();

        // 각 클래스에서 비율에 따라 분할
        for indices in class_indices.values() {
            let n = indices.len();
            let test_size = (n as f64 * config.test_fraction).round() as usize;
            let validation_size = (n as f64 * config.validation_fraction).round() as usize;
            let train_size = n - test_size - validation_size;

            for (i, &idx) in indices.iter().enumerate() {
                if i < train_size {
                    train.push(data[idx].clone());
                } else if i < train_size + test_size {
                    test.push(data[idx].clone());
                } else {
                    validation.push(data[idx].clone());
                }
            }
        }

        // 각 세트 셔플 (클래스 순서 섞기)
        train.shuffle(rng);
        test.shuffle(rng);
        validation.shuffle(rng);

        SplitResult {
            train,
            test,
            validation: if validation.is_empty() {
                None
            } else {
                Some(validation)
            },
        }
    }

    /// 데이터를 K개의 폴드로 분할 (cross-validation용)
    pub fn kfold_split<T: Clone>(
        data: &[T],
        labels: Option<&[i64]>,
        n_folds: usize,
        config: &TrainTestSplitConfig,
    ) -> Vec<(Vec<T>, Vec<T>)> {
        let n = data.len();
        if n == 0 || n_folds == 0 {
            return vec![];
        }

        let mut rng = match config.random_seed {
            Some(seed) => ChaCha8Rng::seed_from_u64(seed),
            None => ChaCha8Rng::from_os_rng(),
        };

        // 인덱스 생성 및 셔플
        let mut indices: Vec<usize> = (0..n).collect();

        if let (true, Some(labels)) = (config.stratify, labels) {
            Self::stratified_kfold(data, labels, n_folds, &mut rng)
        } else {
            indices.shuffle(&mut rng);
            Self::simple_kfold(data, &indices, n_folds)
        }
    }

    /// 단순 K-fold 분할
    fn simple_kfold<T: Clone>(
        data: &[T],
        indices: &[usize],
        n_folds: usize,
    ) -> Vec<(Vec<T>, Vec<T>)> {
        let n = data.len();
        let fold_size = n / n_folds;
        let mut folds = Vec::with_capacity(n_folds);

        for fold_idx in 0..n_folds {
            let start = fold_idx * fold_size;
            let end = if fold_idx == n_folds - 1 {
                n
            } else {
                (fold_idx + 1) * fold_size
            };

            let test: Vec<T> = indices[start..end]
                .iter()
                .map(|&i| data[i].clone())
                .collect();

            let train: Vec<T> = indices[..start]
                .iter()
                .chain(indices[end..].iter())
                .map(|&i| data[i].clone())
                .collect();

            folds.push((train, test));
        }

        folds
    }

    /// 층화 K-fold 분할
    fn stratified_kfold<T: Clone>(
        data: &[T],
        labels: &[i64],
        n_folds: usize,
        rng: &mut ChaCha8Rng,
    ) -> Vec<(Vec<T>, Vec<T>)> {
        // 클래스별 인덱스 그룹화
        let mut class_indices: HashMap<i64, Vec<usize>> = HashMap::new();
        for (i, &label) in labels.iter().enumerate() {
            class_indices.entry(label).or_default().push(i);
        }

        // 각 클래스별로 셔플
        for indices in class_indices.values_mut() {
            indices.shuffle(rng);
        }

        // 각 폴드에 할당될 인덱스
        let mut fold_indices: Vec<Vec<usize>> = vec![vec![]; n_folds];

        // 각 클래스에서 균등하게 분배
        for indices in class_indices.values() {
            for (i, &idx) in indices.iter().enumerate() {
                fold_indices[i % n_folds].push(idx);
            }
        }

        // 폴드 생성
        let mut folds = Vec::with_capacity(n_folds);
        for fold_idx in 0..n_folds {
            let test: Vec<T> = fold_indices[fold_idx]
                .iter()
                .map(|&i| data[i].clone())
                .collect();

            let train: Vec<T> = fold_indices
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != fold_idx)
                .flat_map(|(_, indices)| indices.iter())
                .map(|&i| data[i].clone())
                .collect();

            folds.push((train, test));
        }

        folds
    }
}
