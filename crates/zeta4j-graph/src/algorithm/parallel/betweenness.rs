// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 병렬 Betweenness Centrality 알고리즘
//!
//! Rayon을 사용하여 Brandes 알고리즘을 병렬화합니다.
//! 각 노드를 소스로 하는 BFS를 병렬로 실행하여 성능을 향상시킵니다.

use rayon::prelude::*;
use std::collections::{HashMap, VecDeque};

use super::config::ParallelConfig;
use crate::algorithm::centrality::{BetweennessConfig, BetweennessResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;

/// 병렬 Betweenness Centrality 계산기
pub struct ParallelBetweenness<'a> {
    engine: &'a GraphEngine,
    config: BetweennessConfig,
    par_config: ParallelConfig,
}

impl<'a> ParallelBetweenness<'a> {
    /// 새로운 병렬 Betweenness 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: BetweennessConfig::default(),
            par_config: ParallelConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: BetweennessConfig) -> Self {
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

    /// 병렬 Betweenness Centrality 계산
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<BetweennessResult> {
        betweenness_parallel(self.engine, &self.config, &self.par_config, rel_types).await
    }
}

/// 병렬 Betweenness Centrality 계산 함수
///
/// Brandes 알고리즘을 Rayon으로 병렬화합니다.
/// 각 노드를 소스로 하는 SSSP(Single Source Shortest Path)를 병렬로 실행합니다.
///
/// # Arguments
/// * `engine` - 그래프 엔진
/// * `config` - Betweenness 설정
/// * `par_config` - 병렬 처리 설정
/// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
///
/// # Returns
/// Betweenness Centrality 결과
pub async fn betweenness_parallel(
    engine: &GraphEngine,
    config: &BetweennessConfig,
    par_config: &ParallelConfig,
    rel_types: Option<&[String]>,
) -> Result<BetweennessResult> {
    // 1. 그래프 구조 빌드
    let (nodes, adjacency) = build_graph_structure(engine, config, rel_types).await?;

    if nodes.is_empty() {
        return Ok(BetweennessResult {
            scores: HashMap::new(),
        });
    }

    let n = nodes.len();

    // 2. 병렬 vs 순차 결정
    let partial_scores: Vec<HashMap<NodeId, f64>> = if par_config.should_parallelize(n) {
        // 병렬 계산
        nodes
            .par_iter()
            .map(|&source| brandes_single_source(source, &nodes, &adjacency))
            .collect()
    } else {
        // 순차 계산
        nodes
            .iter()
            .map(|&source| brandes_single_source(source, &nodes, &adjacency))
            .collect()
    };

    // 3. 결과 병합
    let mut betweenness: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();

    for partial in partial_scores {
        for (node_id, score) in partial {
            *betweenness.entry(node_id).or_insert(0.0) += score;
        }
    }

    // 4. 무방향 그래프의 경우 2로 나눔
    if !config.directed {
        for score in betweenness.values_mut() {
            *score /= 2.0;
        }
    }

    // 5. 정규화
    if config.normalized && n > 2 {
        let normalization_factor = if config.directed {
            ((n - 1) * (n - 2)) as f64
        } else {
            ((n - 1) * (n - 2)) as f64 / 2.0
        };

        for score in betweenness.values_mut() {
            *score /= normalization_factor;
        }
    }

    Ok(BetweennessResult {
        scores: betweenness,
    })
}

/// 단일 소스에서 Brandes 알고리즘 실행
fn brandes_single_source(
    source: NodeId,
    nodes: &[NodeId],
    adjacency: &HashMap<NodeId, Vec<NodeId>>,
) -> HashMap<NodeId, f64> {
    // 자료구조 초기화
    let mut stack: Vec<NodeId> = Vec::new();
    let mut predecessors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut sigma: HashMap<NodeId, f64> = HashMap::new();
    let mut distance: HashMap<NodeId, i64> = HashMap::new();

    for &node in nodes {
        predecessors.insert(node, Vec::new());
        sigma.insert(node, 0.0);
        distance.insert(node, -1);
    }

    sigma.insert(source, 1.0);
    distance.insert(source, 0);

    // BFS
    let mut queue: VecDeque<NodeId> = VecDeque::new();
    queue.push_back(source);

    while let Some(v) = queue.pop_front() {
        stack.push(v);
        let dist_v = distance.get(&v).copied().unwrap_or(-1);

        if let Some(neighbors) = adjacency.get(&v) {
            for &w in neighbors {
                // 처음 방문
                if distance.get(&w).copied().unwrap_or(-1) < 0 {
                    distance.insert(w, dist_v + 1);
                    queue.push_back(w);
                }

                // 최단 경로인 경우
                if distance.get(&w).copied().unwrap_or(-1) == dist_v + 1 {
                    let sigma_v = sigma.get(&v).copied().unwrap_or(0.0);
                    *sigma.entry(w).or_insert(0.0) += sigma_v;
                    predecessors.entry(w).or_default().push(v);
                }
            }
        }
    }

    // 역방향 축적
    let mut delta: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();
    let mut partial_betweenness: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();

    while let Some(w) = stack.pop() {
        let sigma_w = sigma.get(&w).copied().unwrap_or(1.0);
        let delta_w = delta.get(&w).copied().unwrap_or(0.0);

        if let Some(preds) = predecessors.get(&w) {
            for &v in preds {
                let sigma_v = sigma.get(&v).copied().unwrap_or(0.0);
                let contribution = if sigma_w > 0.0 {
                    (sigma_v / sigma_w) * (1.0 + delta_w)
                } else {
                    0.0
                };
                *delta.entry(v).or_insert(0.0) += contribution;
            }
        }

        if w != source {
            *partial_betweenness.entry(w).or_insert(0.0) += delta_w;
        }
    }

    partial_betweenness
}

/// 그래프 구조 빌드
async fn build_graph_structure(
    engine: &GraphEngine,
    config: &BetweennessConfig,
    rel_types: Option<&[String]>,
) -> Result<(Vec<NodeId>, HashMap<NodeId, Vec<NodeId>>)> {
    let mut nodes: Vec<NodeId> = Vec::new();
    let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    // 노드 수집
    let node_count = engine.node_count().await?;
    for node_id in 1..=node_count as u64 {
        if engine.get_node(node_id).await?.is_some() {
            nodes.push(node_id);
            adjacency.insert(node_id, Vec::new());
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

            adjacency.entry(from).or_default().push(to);

            if !config.directed {
                adjacency.entry(to).or_default().push(from);
            }
        }
    }

    Ok((nodes, adjacency))
}
