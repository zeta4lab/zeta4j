//! Katz Centrality 알고리즘
//!
//! Eigenvector Centrality의 일반화 버전으로, 감쇠 계수(alpha)와
//! 기본 중심성(beta)을 도입하여 더 유연한 중심성 측정을 제공합니다.
//!
//! 공식: x = alpha * A * x + beta
//! 여기서 A는 인접 행렬, alpha는 감쇠 계수, beta는 기본 중심성입니다.
//!
//! alpha가 너무 크면 발산할 수 있으므로 alpha < 1/λ_max 조건을 만족해야 합니다.

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Katz Centrality 설정
#[derive(Debug, Clone)]
pub struct KatzConfig {
    /// 감쇠 계수 (alpha)
    /// 멀리 있는 노드의 영향력을 감쇠시킴
    /// 값이 작을수록 가까운 이웃의 영향력이 커짐
    pub alpha: f64,
    /// 기본 중심성 (beta)
    /// 모든 노드에 부여되는 기본 점수
    pub beta: f64,
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 허용 오차
    pub tolerance: f64,
    /// 정규화 여부 (true면 최대값이 1.0이 되도록 정규화)
    pub normalized: bool,
    /// 방향성 고려 여부
    pub directed: bool,
}

impl Default for KatzConfig {
    fn default() -> Self {
        Self {
            alpha: 0.1,
            beta: 1.0,
            max_iterations: 100,
            tolerance: 1e-6,
            normalized: true,
            directed: false,
        }
    }
}

impl KatzConfig {
    /// 감쇠 계수 설정
    pub fn with_alpha(mut self, alpha: f64) -> Self {
        self.alpha = alpha;
        self
    }

    /// 기본 중심성 설정
    pub fn with_beta(mut self, beta: f64) -> Self {
        self.beta = beta;
        self
    }

    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// 수렴 허용 오차 설정
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// 정규화 설정
    pub fn with_normalized(mut self, normalized: bool) -> Self {
        self.normalized = normalized;
        self
    }

    /// 방향성 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }
}

/// Katz Centrality 결과
#[derive(Debug, Clone)]
pub struct KatzResult {
    /// 노드별 Katz Centrality 점수
    pub scores: HashMap<NodeId, f64>,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 사용된 alpha 값
    pub alpha: f64,
    /// 사용된 beta 값
    pub beta: f64,
}

impl KatzResult {
    /// 특정 노드의 점수 조회
    pub fn score(&self, node_id: NodeId) -> Option<f64> {
        self.scores.get(&node_id).copied()
    }

    /// 상위 N개 노드 반환 (점수 내림차순)
    pub fn top_n(&self, n: usize) -> Vec<(NodeId, f64)> {
        let mut items: Vec<_> = self
            .scores
            .iter()
            .map(|(&id, &score)| (id, score))
            .collect();
        items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        items.truncate(n);
        items
    }

    /// 빈 결과 생성
    pub fn empty(alpha: f64, beta: f64) -> Self {
        Self {
            scores: HashMap::new(),
            iterations: 0,
            converged: true,
            alpha,
            beta,
        }
    }
}

/// Katz Centrality 계산기
pub struct KatzCentrality<'a> {
    engine: &'a GraphEngine,
    config: KatzConfig,
}

impl<'a> KatzCentrality<'a> {
    /// 새로운 Katz Centrality 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: KatzConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: KatzConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 Katz Centrality 계산
    ///
    /// Power Iteration 방법 사용:
    /// x^(k+1) = alpha * A * x^(k) + beta * 1
    ///
    /// # Arguments
    /// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<KatzResult> {
        // 1. 그래프 구조 수집
        let (nodes, adjacency) = self.build_graph_structure(rel_types).await?;

        if nodes.is_empty() {
            return Ok(KatzResult::empty(self.config.alpha, self.config.beta));
        }

        // 2. 초기 점수 설정 (모두 beta)
        let mut scores: HashMap<NodeId, f64> =
            nodes.iter().map(|&id| (id, self.config.beta)).collect();

        let mut converged = false;
        let mut iterations = 0;

        // 3. Power Iteration
        for iter in 0..self.config.max_iterations {
            iterations = iter + 1;

            // 새로운 점수 계산: x_new = alpha * A * x + beta
            let mut new_scores: HashMap<NodeId, f64> = HashMap::new();

            for &node in &nodes {
                let mut sum = 0.0;

                // 이웃 노드들의 점수 합산
                if let Some(neighbors) = adjacency.get(&node) {
                    for &neighbor in neighbors {
                        sum += scores.get(&neighbor).unwrap_or(&0.0);
                    }
                }

                // Katz 공식: alpha * sum + beta
                let score = self.config.alpha * sum + self.config.beta;
                new_scores.insert(node, score);
            }

            // 수렴 체크
            let diff: f64 = nodes
                .iter()
                .map(|&node| {
                    let old = scores.get(&node).unwrap_or(&0.0);
                    let new = new_scores.get(&node).unwrap_or(&0.0);
                    (old - new).abs()
                })
                .sum();

            scores = new_scores;

            if diff < self.config.tolerance {
                converged = true;
                break;
            }
        }

        // 4. 최종 정규화 (최대값을 1.0으로)
        if self.config.normalized {
            let max_score = scores.values().cloned().fold(0.0_f64, f64::max);

            if max_score > 1e-10 {
                for score in scores.values_mut() {
                    *score /= max_score;
                }
            }
        }

        Ok(KatzResult {
            scores,
            iterations,
            converged,
            alpha: self.config.alpha,
            beta: self.config.beta,
        })
    }

    /// 단일 노드의 Katz Centrality 점수 계산
    pub async fn calculate_single(
        &self,
        node_id: NodeId,
        rel_types: Option<&[String]>,
    ) -> Result<f64> {
        let result = self.calculate(rel_types).await?;
        Ok(result.score(node_id).unwrap_or(0.0))
    }

    /// 그래프 구조 빌드 (노드 목록, 인접 리스트)
    async fn build_graph_structure(
        &self,
        rel_types: Option<&[String]>,
    ) -> Result<(Vec<NodeId>, HashMap<NodeId, Vec<NodeId>>)> {
        let mut nodes: Vec<NodeId> = Vec::new();
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                adjacency.insert(node_id, Vec::new());
            }
        }

        // 엣지 수집
        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(types) = rel_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // Katz는 들어오는 엣지를 기반으로 계산
                // to 노드의 점수에 from 노드가 기여
                adjacency.entry(to).or_default().push(from);

                // 무방향 그래프: 양방향 추가
                if !self.config.directed {
                    adjacency.entry(from).or_default().push(to);
                }
            }
        }

        Ok((nodes, adjacency))
    }
}

/// 편의 함수: Katz Centrality 계산 (기본 설정)
pub async fn katz_centrality(engine: &GraphEngine) -> Result<KatzResult> {
    KatzCentrality::new(engine).calculate(None).await
}

/// 편의 함수: alpha 지정 Katz Centrality
pub async fn katz_centrality_with_alpha(engine: &GraphEngine, alpha: f64) -> Result<KatzResult> {
    let config = KatzConfig::default().with_alpha(alpha);
    KatzCentrality::with_config(engine, config)
        .calculate(None)
        .await
}

/// 편의 함수: alpha, beta 지정 Katz Centrality
pub async fn katz_centrality_with_params(
    engine: &GraphEngine,
    alpha: f64,
    beta: f64,
) -> Result<KatzResult> {
    let config = KatzConfig::default().with_alpha(alpha).with_beta(beta);
    KatzCentrality::with_config(engine, config)
        .calculate(None)
        .await
}

/// 편의 함수: 단일 노드 Katz 점수
pub async fn katz_score(engine: &GraphEngine, node_id: NodeId) -> Result<f64> {
    KatzCentrality::new(engine)
        .calculate_single(node_id, None)
        .await
}
