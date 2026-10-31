// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Hybrid Query (하이브리드 쿼리) 구현
//!
//! 벡터 유사도 검색, 그래프 구조 쿼리, BM25 전문 검색을 결합한 하이브리드 쿼리 기능.
//!
//! # 전략
//!
//! - `VectorFirst`: 벡터 검색 후 그래프 필터
//! - `GraphFirst`: 그래프 탐색 후 벡터 유사도 계산
//! - `Parallel`: 병렬 실행 후 결과 병합
//!
//! # 병합 방식
//!
//! - `WeightedSum`: 가중 합산
//! - `RRF`: Reciprocal Rank Fusion
//! - `Intersection`: 교집합
//! - `Union`: 합집합
//!
//! # 3-way Fusion
//!
//! 벡터, 그래프, 전문검색(BM25) 세 가지 검색 결과를 융합합니다.

use std::collections::HashMap;

/// 하이브리드 검색 결과
#[derive(Debug, Clone, PartialEq)]
pub struct HybridResult {
    /// 노드 ID
    pub node_id: u64,
    /// 벡터 점수 (유사도 또는 거리)
    pub vector_score: Option<f32>,
    /// 그래프 점수
    pub graph_score: Option<f32>,
    /// 전문 검색 점수 (BM25)
    pub fulltext_score: Option<f32>,
    /// 통합 점수
    pub hybrid_score: f32,
}

impl HybridResult {
    pub fn new(node_id: u64, hybrid_score: f32) -> Self {
        Self {
            node_id,
            vector_score: None,
            graph_score: None,
            fulltext_score: None,
            hybrid_score,
        }
    }

    pub fn with_scores(
        node_id: u64,
        vector_score: Option<f32>,
        graph_score: Option<f32>,
        hybrid_score: f32,
    ) -> Self {
        Self {
            node_id,
            vector_score,
            graph_score,
            fulltext_score: None,
            hybrid_score,
        }
    }

    /// 3-way fusion을 위한 전체 점수 설정
    pub fn with_all_scores(
        node_id: u64,
        vector_score: Option<f32>,
        graph_score: Option<f32>,
        fulltext_score: Option<f32>,
        hybrid_score: f32,
    ) -> Self {
        Self {
            node_id,
            vector_score,
            graph_score,
            fulltext_score,
            hybrid_score,
        }
    }
}

/// 하이브리드 쿼리 전략
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HybridQueryStrategy {
    /// 벡터 검색 먼저, 결과로 그래프 필터
    #[default]
    VectorFirst,

    /// 그래프 필터 먼저, 결과에서 벡터 검색
    GraphFirst,

    /// 병렬 실행 후 결과 병합
    Parallel,
}

/// 결과 병합 방식
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeMode {
    /// 가중 합산
    #[default]
    WeightedSum,

    /// Reciprocal Rank Fusion
    RRF,

    /// 교집합 (둘 다 만족)
    Intersection,

    /// 합집합 (하나라도 만족)
    Union,
}

/// 하이브리드 쿼리 설정
#[derive(Debug, Clone)]
pub struct HybridQueryConfig {
    /// 실행 전략
    pub strategy: HybridQueryStrategy,
    /// 벡터 점수 가중치 (0.0 ~ 1.0)
    pub vector_weight: f32,
    /// 그래프 점수 가중치 (0.0 ~ 1.0)
    pub graph_weight: f32,
    /// 전문 검색(BM25) 점수 가중치 (0.0 ~ 1.0)
    pub fulltext_weight: f32,
    /// 결과 병합 방식
    pub merge_mode: MergeMode,
    /// 최대 결과 수 (0 = 무제한)
    pub limit: usize,
    /// RRF k 상수 (일반적으로 60)
    pub rrf_k: f32,
}

impl Default for HybridQueryConfig {
    fn default() -> Self {
        Self {
            strategy: HybridQueryStrategy::VectorFirst,
            vector_weight: 0.7,
            graph_weight: 0.3,
            fulltext_weight: 0.0,
            merge_mode: MergeMode::WeightedSum,
            limit: 100,
            rrf_k: 20.0,
        }
    }
}

impl HybridQueryConfig {
    /// 가중치 정규화 (벡터 + 그래프 + 전문검색)
    pub fn normalize_weights(&mut self) {
        let total = self.vector_weight + self.graph_weight + self.fulltext_weight;
        if total > 0.0 {
            self.vector_weight /= total;
            self.graph_weight /= total;
            self.fulltext_weight /= total;
        }
    }

    /// 균등 가중치 설정 (벡터, 그래프)
    pub fn equal_weights() -> Self {
        Self {
            vector_weight: 0.5,
            graph_weight: 0.5,
            fulltext_weight: 0.0,
            ..Default::default()
        }
    }

    /// 3-way 균등 가중치 설정 (벡터, 그래프, 전문검색)
    pub fn equal_weights_3way() -> Self {
        Self {
            vector_weight: 0.34,
            graph_weight: 0.33,
            fulltext_weight: 0.33,
            ..Default::default()
        }
    }

    /// 벡터 중심 설정
    pub fn vector_focused() -> Self {
        Self {
            vector_weight: 0.8,
            graph_weight: 0.2,
            fulltext_weight: 0.0,
            ..Default::default()
        }
    }

    /// 그래프 중심 설정
    pub fn graph_focused() -> Self {
        Self {
            vector_weight: 0.2,
            graph_weight: 0.8,
            fulltext_weight: 0.0,
            ..Default::default()
        }
    }

    /// 벡터 + 전문검색 중심 설정
    pub fn rag_focused() -> Self {
        Self {
            vector_weight: 0.5,
            graph_weight: 0.2,
            fulltext_weight: 0.3,
            ..Default::default()
        }
    }

    /// 전문검색 중심 설정 (키워드 검색에 적합)
    pub fn fulltext_focused() -> Self {
        Self {
            vector_weight: 0.2,
            graph_weight: 0.1,
            fulltext_weight: 0.7,
            ..Default::default()
        }
    }
}

/// 벡터 후보
#[derive(Debug, Clone)]
pub struct VectorCandidate {
    pub node_id: u64,
    pub score: f32,
}

/// 그래프 후보
#[derive(Debug, Clone)]
pub struct GraphCandidate {
    pub node_id: u64,
    pub score: f32,
}

/// 전문 검색(BM25) 후보
#[derive(Debug, Clone)]
pub struct FulltextCandidate {
    pub node_id: u64,
    pub score: f32,
}

/// 하이브리드 결과 병합기
pub struct HybridMerger {
    config: HybridQueryConfig,
}

impl HybridMerger {
    pub fn new(config: HybridQueryConfig) -> Self {
        Self { config }
    }

    /// 2-way 결과 병합 (벡터 + 그래프)
    pub fn merge(
        &self,
        vector_results: Vec<VectorCandidate>,
        graph_results: Vec<GraphCandidate>,
    ) -> Vec<HybridResult> {
        self.merge_3way(vector_results, graph_results, vec![])
    }

    /// 3-way 결과 병합 (벡터 + 그래프 + 전문검색)
    pub fn merge_3way(
        &self,
        vector_results: Vec<VectorCandidate>,
        graph_results: Vec<GraphCandidate>,
        fulltext_results: Vec<FulltextCandidate>,
    ) -> Vec<HybridResult> {
        match self.config.merge_mode {
            MergeMode::WeightedSum => {
                self.merge_weighted_sum_3way(vector_results, graph_results, fulltext_results)
            }
            MergeMode::RRF => self.merge_rrf_3way(vector_results, graph_results, fulltext_results),
            MergeMode::Intersection => {
                self.merge_intersection_3way(vector_results, graph_results, fulltext_results)
            }
            MergeMode::Union => {
                self.merge_union_3way(vector_results, graph_results, fulltext_results)
            }
        }
    }

    /// Reciprocal Rank Fusion (3-way)
    ///
    /// RRF 공식: score = Σ 1 / (k + rank_i)
    #[expect(clippy::type_complexity)]
    fn merge_rrf_3way(
        &self,
        vector_results: Vec<VectorCandidate>,
        graph_results: Vec<GraphCandidate>,
        fulltext_results: Vec<FulltextCandidate>,
    ) -> Vec<HybridResult> {
        let k = self.config.rrf_k;
        // (hybrid_score, vector_score, graph_score, fulltext_score)
        let mut score_map: HashMap<u64, (f32, Option<f32>, Option<f32>, Option<f32>)> =
            HashMap::new();

        // 벡터 결과 순위
        for (rank, candidate) in vector_results.iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            let entry = score_map
                .entry(candidate.node_id)
                .or_insert((0.0, None, None, None));
            entry.0 += rrf_score * self.config.vector_weight;
            entry.1 = Some(candidate.score);
        }

        // 그래프 결과 순위
        for (rank, candidate) in graph_results.iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            let entry = score_map
                .entry(candidate.node_id)
                .or_insert((0.0, None, None, None));
            entry.0 += rrf_score * self.config.graph_weight;
            entry.2 = Some(candidate.score);
        }

        // 전문검색 결과 순위
        for (rank, candidate) in fulltext_results.iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);
            let entry = score_map
                .entry(candidate.node_id)
                .or_insert((0.0, None, None, None));
            entry.0 += rrf_score * self.config.fulltext_weight;
            entry.3 = Some(candidate.score);
        }

        // 결과 정렬
        let mut results: Vec<HybridResult> = score_map
            .into_iter()
            .map(|(node_id, (score, v_score, g_score, f_score))| {
                HybridResult::with_all_scores(node_id, v_score, g_score, f_score, score)
            })
            .collect();

        results.sort_by(|a, b| {
            b.hybrid_score
                .partial_cmp(&a.hybrid_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if self.config.limit > 0 {
            results.truncate(self.config.limit);
        }

        results
    }

    /// 가중 합산 병합 (3-way)
    #[expect(clippy::type_complexity)]
    fn merge_weighted_sum_3way(
        &self,
        vector_results: Vec<VectorCandidate>,
        graph_results: Vec<GraphCandidate>,
        fulltext_results: Vec<FulltextCandidate>,
    ) -> Vec<HybridResult> {
        // (vector_score, graph_score, fulltext_score)
        let mut score_map: HashMap<u64, (Option<f32>, Option<f32>, Option<f32>)> = HashMap::new();

        // 점수 정규화를 위한 최대값
        let max_vector = vector_results
            .iter()
            .map(|c| c.score)
            .fold(0.0f32, f32::max);
        let max_graph = graph_results.iter().map(|c| c.score).fold(0.0f32, f32::max);
        let max_fulltext = fulltext_results
            .iter()
            .map(|c| c.score)
            .fold(0.0f32, f32::max);

        for candidate in &vector_results {
            let normalized = if max_vector > 0.0 {
                candidate.score / max_vector
            } else {
                0.0
            };
            score_map
                .entry(candidate.node_id)
                .or_insert((None, None, None))
                .0 = Some(normalized);
        }

        for candidate in &graph_results {
            let normalized = if max_graph > 0.0 {
                candidate.score / max_graph
            } else {
                0.0
            };
            score_map
                .entry(candidate.node_id)
                .or_insert((None, None, None))
                .1 = Some(normalized);
        }

        for candidate in &fulltext_results {
            let normalized = if max_fulltext > 0.0 {
                candidate.score / max_fulltext
            } else {
                0.0
            };
            score_map
                .entry(candidate.node_id)
                .or_insert((None, None, None))
                .2 = Some(normalized);
        }

        let mut results: Vec<HybridResult> = score_map
            .into_iter()
            .map(|(node_id, (v_score, g_score, f_score))| {
                let vector_contrib = v_score.unwrap_or(0.0) * self.config.vector_weight;
                let graph_contrib = g_score.unwrap_or(0.0) * self.config.graph_weight;
                let fulltext_contrib = f_score.unwrap_or(0.0) * self.config.fulltext_weight;

                HybridResult::with_all_scores(
                    node_id,
                    v_score,
                    g_score,
                    f_score,
                    vector_contrib + graph_contrib + fulltext_contrib,
                )
            })
            .collect();

        results.sort_by(|a, b| {
            b.hybrid_score
                .partial_cmp(&a.hybrid_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if self.config.limit > 0 {
            results.truncate(self.config.limit);
        }

        results
    }

    /// 교집합 병합 (3-way: 모든 소스에 존재하는 노드만)
    fn merge_intersection_3way(
        &self,
        vector_results: Vec<VectorCandidate>,
        graph_results: Vec<GraphCandidate>,
        fulltext_results: Vec<FulltextCandidate>,
    ) -> Vec<HybridResult> {
        let vector_set: HashMap<u64, f32> = vector_results
            .iter()
            .map(|c| (c.node_id, c.score))
            .collect();

        let graph_set: HashMap<u64, f32> =
            graph_results.iter().map(|c| (c.node_id, c.score)).collect();

        let fulltext_set: HashMap<u64, f32> = fulltext_results
            .iter()
            .map(|c| (c.node_id, c.score))
            .collect();

        // fulltext가 비어있으면 2-way 교집합
        let has_fulltext = !fulltext_results.is_empty() && self.config.fulltext_weight > 0.0;

        let mut results: Vec<HybridResult> = vector_set
            .iter()
            .filter_map(|(node_id, v_score)| {
                let g_score = graph_set.get(node_id)?;

                if has_fulltext {
                    let f_score = fulltext_set.get(node_id)?;
                    let hybrid_score = v_score * self.config.vector_weight
                        + g_score * self.config.graph_weight
                        + f_score * self.config.fulltext_weight;
                    Some(HybridResult::with_all_scores(
                        *node_id,
                        Some(*v_score),
                        Some(*g_score),
                        Some(*f_score),
                        hybrid_score,
                    ))
                } else {
                    let hybrid_score =
                        v_score * self.config.vector_weight + g_score * self.config.graph_weight;
                    Some(HybridResult::with_all_scores(
                        *node_id,
                        Some(*v_score),
                        Some(*g_score),
                        None,
                        hybrid_score,
                    ))
                }
            })
            .collect();

        results.sort_by(|a, b| {
            b.hybrid_score
                .partial_cmp(&a.hybrid_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if self.config.limit > 0 {
            results.truncate(self.config.limit);
        }

        results
    }

    /// 합집합 병합 (3-way: 하나라도 만족하는 노드)
    #[expect(clippy::type_complexity)]
    fn merge_union_3way(
        &self,
        vector_results: Vec<VectorCandidate>,
        graph_results: Vec<GraphCandidate>,
        fulltext_results: Vec<FulltextCandidate>,
    ) -> Vec<HybridResult> {
        let mut score_map: HashMap<u64, (Option<f32>, Option<f32>, Option<f32>)> = HashMap::new();

        for candidate in &vector_results {
            score_map
                .entry(candidate.node_id)
                .or_insert((None, None, None))
                .0 = Some(candidate.score);
        }

        for candidate in &graph_results {
            score_map
                .entry(candidate.node_id)
                .or_insert((None, None, None))
                .1 = Some(candidate.score);
        }

        for candidate in &fulltext_results {
            score_map
                .entry(candidate.node_id)
                .or_insert((None, None, None))
                .2 = Some(candidate.score);
        }

        // 최대값으로 정규화
        let max_vector = vector_results
            .iter()
            .map(|c| c.score)
            .fold(0.0f32, f32::max);
        let max_graph = graph_results.iter().map(|c| c.score).fold(0.0f32, f32::max);
        let max_fulltext = fulltext_results
            .iter()
            .map(|c| c.score)
            .fold(0.0f32, f32::max);

        let mut results: Vec<HybridResult> = score_map
            .into_iter()
            .map(|(node_id, (v_score, g_score, f_score))| {
                let v_normalized = v_score.map(|s| {
                    if max_vector > 0.0 {
                        s / max_vector
                    } else {
                        0.0
                    }
                });
                let g_normalized =
                    g_score.map(|s| if max_graph > 0.0 { s / max_graph } else { 0.0 });
                let f_normalized = f_score.map(|s| {
                    if max_fulltext > 0.0 {
                        s / max_fulltext
                    } else {
                        0.0
                    }
                });

                let hybrid_score = v_normalized.unwrap_or(0.0) * self.config.vector_weight
                    + g_normalized.unwrap_or(0.0) * self.config.graph_weight
                    + f_normalized.unwrap_or(0.0) * self.config.fulltext_weight;

                HybridResult::with_all_scores(node_id, v_score, g_score, f_score, hybrid_score)
            })
            .collect();

        results.sort_by(|a, b| {
            b.hybrid_score
                .partial_cmp(&a.hybrid_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if self.config.limit > 0 {
            results.truncate(self.config.limit);
        }

        results
    }
}
