//! 병렬 PageRank 알고리즘
//!
//! Rayon을 사용하여 PageRank 알고리즘을 병렬화합니다.
//! 각 반복에서 노드별 점수 계산을 병렬로 실행하여 성능을 향상시킵니다.

use rayon::prelude::*;
use std::collections::HashMap;

use super::config::ParallelConfig;
use crate::algorithm::centrality::{PageRankConfig, PageRankResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;

/// 병렬 PageRank 계산기
pub struct ParallelPageRank<'a> {
    engine: &'a GraphEngine,
    config: PageRankConfig,
    par_config: ParallelConfig,
}

impl<'a> ParallelPageRank<'a> {
    /// 새로운 병렬 PageRank 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: PageRankConfig::default(),
            par_config: ParallelConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: PageRankConfig) -> Self {
        Self {
            engine,
            config,
            par_config: ParallelConfig::default(),
        }
    }

    /// 병렬 설정 지정
    pub fn with_parallel_config(mut self, par_config: ParallelConfig) -> Self {
        self.par_config = par_config;
        self
    }

    /// 병렬 PageRank 계산
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<PageRankResult> {
        pagerank_parallel(self.engine, &self.config, &self.par_config, rel_types).await
    }
}

/// 병렬 PageRank 계산 함수
///
/// Rayon으로 노드별 점수 계산을 병렬화합니다.
///
/// # Arguments
/// * `engine` - 그래프 엔진
/// * `config` - PageRank 설정
/// * `par_config` - 병렬 처리 설정
/// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
///
/// # Returns
/// PageRank 결과
pub async fn pagerank_parallel(
    engine: &GraphEngine,
    config: &PageRankConfig,
    par_config: &ParallelConfig,
    rel_types: Option<&[String]>,
) -> Result<PageRankResult> {
    // 1. 그래프 구조 빌드
    let (nodes, outgoing, incoming) = build_graph_structure(engine, rel_types).await?;

    if nodes.is_empty() {
        return Ok(PageRankResult {
            scores: HashMap::new(),
            iterations: 0,
            converged: true,
        });
    }

    let n = nodes.len();
    let initial_score = 1.0 / n as f64;
    let damping = config.damping_factor;
    let teleport = (1.0 - damping) / n as f64;

    // 2. 초기 점수 설정
    let mut scores: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, initial_score)).collect();

    let mut iterations = 0;
    let mut converged = false;

    // 3. 반복 계산
    for _ in 0..config.max_iterations {
        iterations += 1;

        // 댕글링 노드 합계 (병렬)
        let dangling_sum: f64 = if par_config.should_parallelize(n) {
            nodes
                .par_iter()
                .filter(|&&id| outgoing.get(&id).is_none_or(|v| v.is_empty()))
                .map(|&id| scores.get(&id).copied().unwrap_or(0.0))
                .sum()
        } else {
            nodes
                .iter()
                .filter(|&&id| outgoing.get(&id).is_none_or(|v| v.is_empty()))
                .map(|&id| scores.get(&id).copied().unwrap_or(0.0))
                .sum()
        };

        let dangling_contribution = damping * dangling_sum / n as f64;

        // 새 점수 계산 (병렬 vs 순차)
        let new_scores: HashMap<NodeId, f64> = if par_config.should_parallelize(n) {
            nodes
                .par_iter()
                .map(|&node_id| {
                    let incoming_contribution =
                        calculate_incoming_contribution(node_id, &incoming, &outgoing, &scores);
                    let new_score =
                        teleport + damping * incoming_contribution + dangling_contribution;
                    (node_id, new_score)
                })
                .collect()
        } else {
            nodes
                .iter()
                .map(|&node_id| {
                    let incoming_contribution =
                        calculate_incoming_contribution(node_id, &incoming, &outgoing, &scores);
                    let new_score =
                        teleport + damping * incoming_contribution + dangling_contribution;
                    (node_id, new_score)
                })
                .collect()
        };

        // 수렴 확인
        let max_diff = check_convergence(&scores, &new_scores);
        scores = new_scores;

        if max_diff < config.tolerance {
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

    Ok(PageRankResult {
        scores,
        iterations,
        converged,
    })
}

/// 들어오는 노드들로부터의 기여도 계산
fn calculate_incoming_contribution(
    node_id: NodeId,
    incoming: &HashMap<NodeId, Vec<NodeId>>,
    outgoing: &HashMap<NodeId, Vec<NodeId>>,
    scores: &HashMap<NodeId, f64>,
) -> f64 {
    incoming
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
        .unwrap_or(0.0)
}

/// 수렴 확인: 최대 차이 계산
fn check_convergence(old_scores: &HashMap<NodeId, f64>, new_scores: &HashMap<NodeId, f64>) -> f64 {
    old_scores
        .iter()
        .map(|(id, &old_score)| {
            let new_score = new_scores.get(id).copied().unwrap_or(0.0);
            (new_score - old_score).abs()
        })
        .fold(0.0_f64, |a, b| a.max(b))
}

/// 그래프 구조 빌드
async fn build_graph_structure(
    engine: &GraphEngine,
    rel_types: Option<&[String]>,
) -> Result<(
    Vec<NodeId>,
    HashMap<NodeId, Vec<NodeId>>,
    HashMap<NodeId, Vec<NodeId>>,
)> {
    let mut nodes: Vec<NodeId> = Vec::new();
    let mut outgoing: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut incoming: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    // 노드 수집
    let node_count = engine.node_count().await?;
    for node_id in 1..=node_count as u64 {
        if engine.get_node(node_id).await?.is_some() {
            nodes.push(node_id);
            outgoing.insert(node_id, Vec::new());
            incoming.insert(node_id, Vec::new());
        }
    }

    // 엣지 수집
    let edge_count = engine.edge_count().await;
    for edge_id in 1..=edge_count as u64 {
        if let Some(edge) = engine.get_edge(edge_id).await? {
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
        }
    }

    Ok((nodes, outgoing, incoming))
}
