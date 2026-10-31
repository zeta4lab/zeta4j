// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Betweenness Centrality 알고리즘
//!
//! 노드가 다른 노드들 사이의 최단 경로에 얼마나 자주 등장하는지를 측정합니다.
//! Brandes 알고리즘을 사용하여 O(VE) 시간복잡도로 계산합니다.

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, VecDeque};

/// Betweenness Centrality 설정
#[derive(Debug, Clone)]
pub struct BetweennessConfig {
    /// 정규화 여부 (true면 0~1 범위로 정규화)
    pub normalized: bool,
    /// 방향성 고려 여부
    pub directed: bool,
}

impl Default for BetweennessConfig {
    fn default() -> Self {
        Self {
            normalized: true,
            directed: false,
        }
    }
}

impl BetweennessConfig {
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

/// Betweenness Centrality 결과
#[derive(Debug, Clone)]
pub struct BetweennessResult {
    /// 노드별 Betweenness Centrality 점수
    pub scores: HashMap<NodeId, f64>,
}

impl BetweennessResult {
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
}

/// Betweenness Centrality 계산기
pub struct BetweennessCentrality<'a> {
    engine: &'a GraphEngine,
    config: BetweennessConfig,
}

impl<'a> BetweennessCentrality<'a> {
    /// 새로운 Betweenness Centrality 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: BetweennessConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: BetweennessConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 Betweenness Centrality 계산
    ///
    /// Brandes 알고리즘 사용 (O(VE) 시간복잡도)
    ///
    /// # Arguments
    /// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<BetweennessResult> {
        // 1. 그래프 구조 수집
        let (nodes, adjacency) = self.build_graph_structure(rel_types).await?;

        if nodes.is_empty() {
            return Ok(BetweennessResult {
                scores: HashMap::new(),
            });
        }

        let n = nodes.len();

        // 2. 모든 노드의 betweenness 초기화
        let mut betweenness: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();

        // 3. 각 노드를 소스로 하여 Brandes 알고리즘 실행
        for &source in &nodes {
            // BFS를 위한 자료구조
            let mut stack: Vec<NodeId> = Vec::new();
            let mut predecessors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
            let mut sigma: HashMap<NodeId, f64> = HashMap::new(); // 최단 경로 수
            let mut distance: HashMap<NodeId, i64> = HashMap::new();

            // 초기화
            for &node in &nodes {
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

                // 이웃 탐색
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

            // 역방향 축적 (Back propagation)
            let mut delta: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();

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
                    *betweenness.entry(w).or_insert(0.0) += delta_w;
                }
            }
        }

        // 4. 무방향 그래프의 경우 2로 나눔 (양방향으로 카운트되므로)
        if !self.config.directed {
            for score in betweenness.values_mut() {
                *score /= 2.0;
            }
        }

        // 5. 정규화
        if self.config.normalized && n > 2 {
            let normalization_factor = if self.config.directed {
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

    /// 단일 노드의 Betweenness Centrality 점수 계산
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

                // 방향성 그래프: from -> to만 추가
                adjacency.entry(from).or_default().push(to);

                // 무방향 그래프: 양방향 추가
                if !self.config.directed {
                    adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok((nodes, adjacency))
    }
}
