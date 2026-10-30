//! ArticleRank 알고리즘
//!
//! PageRank의 변형으로, teleportation 확률 계산에 average degree를 사용합니다.
//! 일반적인 웹 그래프보다는 citation/article 그래프에 더 적합합니다.
//!
//! 공식: AR(v) = (1 - d) / (n * avgDegree) + d * Σ(AR(u) / out(u))

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// ArticleRank 설정
#[derive(Debug, Clone)]
pub struct ArticleRankConfig {
    /// 감쇠 계수 (damping factor), 일반적으로 0.85
    pub damping_factor: f64,
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 임계값 (tolerance)
    pub tolerance: f64,
}

impl Default for ArticleRankConfig {
    fn default() -> Self {
        Self {
            damping_factor: 0.85,
            max_iterations: 20,
            tolerance: 1e-6,
        }
    }
}

impl ArticleRankConfig {
    /// 감쇠 계수 설정
    pub fn with_damping_factor(mut self, factor: f64) -> Self {
        self.damping_factor = factor;
        self
    }

    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, iterations: usize) -> Self {
        self.max_iterations = iterations;
        self
    }

    /// 수렴 임계값 설정
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }
}

/// ArticleRank 결과
#[derive(Debug, Clone)]
pub struct ArticleRankResult {
    /// 노드별 ArticleRank 점수
    pub scores: HashMap<NodeId, f64>,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 그래프의 평균 차수
    pub average_degree: f64,
}

impl ArticleRankResult {
    /// 특정 노드의 ArticleRank 점수 조회
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
}

/// ArticleRank 계산기
pub struct ArticleRank<'a> {
    engine: &'a GraphEngine,
    config: ArticleRankConfig,
}

impl<'a> ArticleRank<'a> {
    /// 새로운 ArticleRank 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ArticleRankConfig::default(),
        }
    }

    /// 설정과 함께 ArticleRank 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: ArticleRankConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 ArticleRank 계산
    ///
    /// # Arguments
    /// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
    ///
    /// # Returns
    /// ArticleRank 결과 (노드별 점수, 반복 횟수, 수렴 여부, 평균 차수)
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<ArticleRankResult> {
        // 1. 그래프 구조 수집 (인접 리스트)
        let (nodes, outgoing, incoming, total_edges) =
            self.build_graph_structure(rel_types).await?;

        if nodes.is_empty() {
            return Ok(ArticleRankResult {
                scores: HashMap::new(),
                iterations: 0,
                converged: true,
                average_degree: 0.0,
            });
        }

        let n = nodes.len();

        // ArticleRank의 핵심 차이점: average degree 계산
        let average_degree = if n > 0 {
            total_edges as f64 / n as f64
        } else {
            1.0
        };

        // average_degree가 0이면 1로 설정 (division by zero 방지)
        let avg_degree = if average_degree > 0.0 {
            average_degree
        } else {
            1.0
        };

        let initial_score = 1.0 / n as f64;
        let damping = self.config.damping_factor;

        // ArticleRank: teleport = (1 - d) / (n * avgDegree)
        // PageRank와의 차이점: n 대신 n * avgDegree 사용
        let teleport = (1.0 - damping) / (n as f64 * avg_degree);

        // 2. 초기 점수 설정
        let mut scores: HashMap<NodeId, f64> =
            nodes.iter().map(|&id| (id, initial_score)).collect();

        let mut iterations = 0;
        let mut converged = false;

        // 3. 반복 계산
        for _ in 0..self.config.max_iterations {
            iterations += 1;
            let mut new_scores: HashMap<NodeId, f64> = HashMap::new();
            let mut max_diff: f64 = 0.0;

            // 댕글링 노드 처리 (나가는 엣지가 없는 노드)
            let dangling_sum: f64 = nodes
                .iter()
                .filter(|&&id| outgoing.get(&id).is_none_or(|v| v.is_empty()))
                .map(|&id| scores.get(&id).copied().unwrap_or(0.0))
                .sum();

            // ArticleRank에서도 dangling contribution은 average degree를 사용
            let dangling_contribution = damping * dangling_sum / (n as f64 * avg_degree);

            for &node_id in &nodes {
                // 들어오는 노드들로부터 점수 수집
                let incoming_contribution: f64 = incoming
                    .get(&node_id)
                    .map(|sources| {
                        sources
                            .iter()
                            .map(|&src| {
                                let src_score = scores.get(&src).copied().unwrap_or(0.0);
                                let src_out_degree = outgoing.get(&src).map_or(0, |v| v.len());
                                if src_out_degree > 0 {
                                    src_score / src_out_degree as f64
                                } else {
                                    0.0
                                }
                            })
                            .sum()
                    })
                    .unwrap_or(0.0);

                let new_score = teleport + damping * incoming_contribution + dangling_contribution;

                let old_score = scores.get(&node_id).copied().unwrap_or(0.0);
                max_diff = max_diff.max((new_score - old_score).abs());

                new_scores.insert(node_id, new_score);
            }

            scores = new_scores;

            // 수렴 확인
            if max_diff < self.config.tolerance {
                converged = true;
                break;
            }
        }

        // 4. 점수 정규화 (합이 1이 되도록)
        let total: f64 = scores.values().sum();
        if total > 0.0 {
            for score in scores.values_mut() {
                *score /= total;
            }
        }

        Ok(ArticleRankResult {
            scores,
            iterations,
            converged,
            average_degree,
        })
    }

    /// 단일 노드의 ArticleRank 점수 계산
    pub async fn calculate_single(
        &self,
        node_id: NodeId,
        rel_types: Option<&[String]>,
    ) -> Result<f64> {
        let result = self.calculate(rel_types).await?;
        Ok(result.score(node_id).unwrap_or(0.0))
    }

    /// 그래프 구조 빌드 (노드 목록, 나가는 엣지, 들어오는 엣지, 총 엣지 수)
    async fn build_graph_structure(
        &self,
        rel_types: Option<&[String]>,
    ) -> Result<(
        Vec<NodeId>,
        HashMap<NodeId, Vec<NodeId>>,
        HashMap<NodeId, Vec<NodeId>>,
        usize,
    )> {
        let mut nodes: Vec<NodeId> = Vec::new();
        let mut outgoing: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut incoming: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut total_edges: usize = 0;

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                outgoing.insert(node_id, Vec::new());
                incoming.insert(node_id, Vec::new());
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

                // 나가는 엣지 추가
                outgoing.entry(from).or_default().push(to);
                // 들어오는 엣지 추가
                incoming.entry(to).or_default().push(from);

                total_edges += 1;
            }
        }

        Ok((nodes, outgoing, incoming, total_edges))
    }
}
