//! 청크 기반 PageRank 알고리즘
//!
//! GraphView 트레이트를 사용하여 대규모 그래프에서 메모리 효율적인 PageRank 계산을 수행합니다.
//! 전체 그래프를 메모리에 로드하지 않고, 청크 단위로 처리합니다.

use crate::core::NodeId;
use crate::engine::graph_view::GraphView;
use crate::error::Result;
use std::collections::HashMap;

/// 청크 기반 PageRank 설정
#[derive(Debug, Clone)]
pub struct ChunkedPageRankConfig {
    /// 감쇠 계수 (damping factor)
    pub damping_factor: f64,
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 임계값
    pub tolerance: f64,
    /// 청크 크기 (한 번에 처리할 노드 수)
    pub chunk_size: usize,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl Default for ChunkedPageRankConfig {
    fn default() -> Self {
        Self {
            damping_factor: 0.85,
            max_iterations: 20,
            tolerance: 1e-6,
            chunk_size: 10_000,
            relationship_types: None,
        }
    }
}

impl ChunkedPageRankConfig {
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

    /// 청크 크기 설정
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 청크 기반 PageRank 결과
#[derive(Debug, Clone)]
pub struct ChunkedPageRankResult {
    /// 노드별 PageRank 점수
    pub scores: HashMap<NodeId, f64>,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 처리된 청크 수
    pub chunks_processed: usize,
}

impl ChunkedPageRankResult {
    /// 특정 노드의 PageRank 점수 조회
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

/// 청크 기반 PageRank 계산기
///
/// GraphView 트레이트를 구현한 모든 그래프에 대해 PageRank를 계산합니다.
/// 대규모 그래프에서 메모리 효율적인 처리를 위해 청크 단위로 작업합니다.
///
/// # 사용 예시
///
/// ```ignore
/// use zeta4j_graph::algorithm::streaming::ChunkedPageRank;
/// use zeta4j_graph::engine::GraphView;
///
/// let config = ChunkedPageRankConfig::default()
///     .with_chunk_size(5000)
///     .with_max_iterations(30);
///
/// let chunked_pr = ChunkedPageRank::with_config(&engine, config);
/// let result = chunked_pr.compute().await?;
///
/// for (node_id, score) in result.top_n(10) {
///     println!("Node {}: {:.6}", node_id, score);
/// }
/// ```
pub struct ChunkedPageRank<'a, G: GraphView> {
    graph: &'a G,
    config: ChunkedPageRankConfig,
}

impl<'a, G: GraphView> ChunkedPageRank<'a, G> {
    /// 기본 설정으로 ChunkedPageRank 생성
    pub fn new(graph: &'a G) -> Self {
        Self {
            graph,
            config: ChunkedPageRankConfig::default(),
        }
    }

    /// 설정과 함께 ChunkedPageRank 생성
    pub fn with_config(graph: &'a G, config: ChunkedPageRankConfig) -> Self {
        Self { graph, config }
    }

    /// PageRank 계산
    ///
    /// 청크 단위로 그래프를 처리하여 메모리 사용량을 최적화합니다.
    pub async fn compute(&self) -> Result<ChunkedPageRankResult> {
        // 1. 노드 ID 수집
        let node_ids = self.graph.all_node_ids().await;
        let n = node_ids.len();

        if n == 0 {
            return Ok(ChunkedPageRankResult {
                scores: HashMap::new(),
                iterations: 0,
                converged: true,
                chunks_processed: 0,
            });
        }

        let initial_score = 1.0 / n as f64;
        let damping = self.config.damping_factor;
        let teleport = (1.0 - damping) / n as f64;

        // 2. 초기 점수 설정
        let mut scores: HashMap<NodeId, f64> =
            node_ids.iter().map(|&id| (id, initial_score)).collect();

        // 3. 아웃고잉 차수 계산 (청크 단위)
        let mut out_degrees: HashMap<NodeId, usize> = HashMap::with_capacity(n);
        let mut chunks_processed = 0;

        for chunk in node_ids.chunks(self.config.chunk_size) {
            for &node_id in chunk {
                let out_edges = self.graph.find_outgoing_edges(node_id).await;
                let filtered_edges = self.filter_edges(&out_edges).await?;
                out_degrees.insert(node_id, filtered_edges.len());
            }
            chunks_processed += 1;
        }

        // 4. 반복 계산
        let mut iterations = 0;
        let mut converged = false;

        for _ in 0..self.config.max_iterations {
            iterations += 1;
            let mut new_scores: HashMap<NodeId, f64> = HashMap::with_capacity(n);
            let mut max_diff: f64 = 0.0;

            // 댕글링 노드 기여도 계산
            let dangling_sum: f64 = node_ids
                .iter()
                .filter(|&&id| out_degrees.get(&id).is_none_or(|&d| d == 0))
                .map(|&id| scores.get(&id).copied().unwrap_or(0.0))
                .sum();
            let dangling_contribution = damping * dangling_sum / n as f64;

            // 청크 단위로 점수 업데이트
            for chunk in node_ids.chunks(self.config.chunk_size) {
                for &node_id in chunk {
                    // 들어오는 노드들로부터 기여도 계산
                    let incoming_edges = self.graph.find_incoming_edges(node_id).await;
                    let filtered_incoming = self.filter_edges(&incoming_edges).await?;

                    let mut incoming_contribution: f64 = 0.0;
                    for &edge_id in &filtered_incoming {
                        if let Some(edge) = self.graph.get_edge(edge_id).await? {
                            let src = edge.from_node();
                            let src_score = scores.get(&src).copied().unwrap_or(0.0);
                            let src_out_degree = out_degrees.get(&src).copied().unwrap_or(0);
                            if src_out_degree > 0 {
                                incoming_contribution += src_score / src_out_degree as f64;
                            }
                        }
                    }

                    let new_score =
                        teleport + damping * incoming_contribution + dangling_contribution;

                    let old_score = scores.get(&node_id).copied().unwrap_or(0.0);
                    max_diff = max_diff.max((new_score - old_score).abs());

                    new_scores.insert(node_id, new_score);
                }
            }

            scores = new_scores;

            // 수렴 확인
            if max_diff < self.config.tolerance {
                converged = true;
                break;
            }
        }

        // 5. 점수 정규화
        let total: f64 = scores.values().sum();
        if total > 0.0 {
            for score in scores.values_mut() {
                *score /= total;
            }
        }

        Ok(ChunkedPageRankResult {
            scores,
            iterations,
            converged,
            chunks_processed,
        })
    }

    /// 관계 타입으로 엣지 필터링
    async fn filter_edges(&self, edge_ids: &[u64]) -> Result<Vec<u64>> {
        match &self.config.relationship_types {
            None => Ok(edge_ids.to_vec()),
            Some(types) => {
                let mut filtered = Vec::with_capacity(edge_ids.len());
                for &edge_id in edge_ids {
                    if let Some(edge) = self.graph.get_edge(edge_id).await?
                        && types.contains(&edge.relation_type().to_string())
                    {
                        filtered.push(edge_id);
                    }
                }
                Ok(filtered)
            }
        }
    }
}
