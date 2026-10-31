// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bron-Kerbosch 알고리즘 - 모든 극대 클리크 찾기
//!
//! 피벗 최적화와 퇴화 순서(degeneracy ordering)를 사용한 효율적인 구현
//!
//! 시간복잡도: O(3^(n/3)) 최악의 경우

use super::{CliqueConfig, CliqueResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// Bron-Kerbosch 알고리즘
pub struct BronKerbosch<'a> {
    engine: &'a GraphEngine,
    config: CliqueConfig,
}

impl<'a> BronKerbosch<'a> {
    /// 새로운 BronKerbosch 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: CliqueConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: CliqueConfig) -> Self {
        Self { engine, config }
    }

    /// 모든 극대 클리크 찾기
    pub async fn find_all_maximal_cliques(&self) -> Result<CliqueResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(CliqueResult::empty());
        }

        let nodes: Vec<NodeId> = adjacency.keys().copied().collect();
        let mut cliques = Vec::new();
        let max_cliques = self.config.max_cliques.unwrap_or(usize::MAX);

        // 퇴화 순서로 정렬 (낮은 차수부터)
        let ordered_nodes = self.degeneracy_ordering(&adjacency);

        let mut p: HashSet<NodeId> = nodes.iter().copied().collect();
        let mut x: HashSet<NodeId> = HashSet::new();

        for v in ordered_nodes {
            if cliques.len() >= max_cliques {
                break;
            }

            let neighbors: HashSet<NodeId> = adjacency.get(&v).cloned().unwrap_or_default();

            let mut r = HashSet::new();
            r.insert(v);

            let new_p: HashSet<NodeId> = p.intersection(&neighbors).copied().collect();
            let new_x: HashSet<NodeId> = x.intersection(&neighbors).copied().collect();

            self.bron_kerbosch_pivot(&adjacency, r, new_p, new_x, &mut cliques, max_cliques);

            p.remove(&v);
            x.insert(v);
        }

        let max_size = cliques.iter().map(|c| c.len()).max().unwrap_or(0);

        Ok(CliqueResult {
            clique_count: cliques.len(),
            max_clique_size: max_size,
            cliques,
        })
    }

    /// 최대 클리크 찾기
    pub async fn find_maximum_clique(&self) -> Result<Vec<NodeId>> {
        let result = self.find_all_maximal_cliques().await?;

        Ok(result
            .cliques
            .into_iter()
            .max_by_key(|c| c.len())
            .unwrap_or_default())
    }

    /// 피벗 최적화된 Bron-Kerbosch
    fn bron_kerbosch_pivot(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        r: HashSet<NodeId>,
        mut p: HashSet<NodeId>,
        mut x: HashSet<NodeId>,
        cliques: &mut Vec<Vec<NodeId>>,
        max_cliques: usize,
    ) {
        if cliques.len() >= max_cliques {
            return;
        }

        if p.is_empty() && x.is_empty() {
            // R은 극대 클리크
            let mut clique: Vec<NodeId> = r.into_iter().collect();
            clique.sort();
            cliques.push(clique);
            return;
        }

        if p.is_empty() {
            return;
        }

        // 피벗 선택: P ∪ X에서 가장 많은 이웃을 가진 노드
        let pivot = self.choose_pivot(&p, &x, adjacency);
        let pivot_neighbors: HashSet<NodeId> = adjacency.get(&pivot).cloned().unwrap_or_default();

        // P \ N(pivot)의 노드들에 대해 탐색
        let candidates: Vec<NodeId> = p
            .iter()
            .filter(|n| !pivot_neighbors.contains(n))
            .copied()
            .collect();

        for v in candidates {
            if cliques.len() >= max_cliques {
                return;
            }

            let neighbors: HashSet<NodeId> = adjacency.get(&v).cloned().unwrap_or_default();

            let mut new_r = r.clone();
            new_r.insert(v);

            let new_p: HashSet<NodeId> = p.intersection(&neighbors).copied().collect();
            let new_x: HashSet<NodeId> = x.intersection(&neighbors).copied().collect();

            self.bron_kerbosch_pivot(adjacency, new_r, new_p, new_x, cliques, max_cliques);

            p.remove(&v);
            x.insert(v);
        }
    }

    /// 피벗 선택: 가장 많은 이웃을 가진 노드
    fn choose_pivot(
        &self,
        p: &HashSet<NodeId>,
        x: &HashSet<NodeId>,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
    ) -> NodeId {
        let candidates: HashSet<NodeId> = p.union(x).copied().collect();

        candidates
            .into_iter()
            .max_by_key(|&node| {
                adjacency
                    .get(&node)
                    .map(|neighbors| neighbors.intersection(p).count())
                    .unwrap_or(0)
            })
            .unwrap_or(1) // 기본값
    }

    /// 퇴화 순서 계산
    fn degeneracy_ordering(&self, adjacency: &HashMap<NodeId, HashSet<NodeId>>) -> Vec<NodeId> {
        let mut degrees: HashMap<NodeId, usize> = adjacency
            .iter()
            .map(|(&node, neighbors)| (node, neighbors.len()))
            .collect();

        let mut ordering = Vec::new();
        let mut remaining: HashSet<NodeId> = adjacency.keys().copied().collect();

        while !remaining.is_empty() {
            // 가장 낮은 차수의 노드 선택
            let Some(&min_node) = remaining
                .iter()
                .min_by_key(|&&node| degrees.get(&node).copied().unwrap_or(0))
            else {
                break;
            };

            ordering.push(min_node);
            remaining.remove(&min_node);

            // 이웃들의 차수 감소
            if let Some(neighbors) = adjacency.get(&min_node) {
                for &neighbor in neighbors {
                    if remaining.contains(&neighbor)
                        && let Some(deg) = degrees.get_mut(&neighbor)
                    {
                        *deg = deg.saturating_sub(1);
                    }
                }
            }
        }

        ordering
    }

    /// 인접 리스트 구축 (무방향)
    async fn build_adjacency(&self) -> Result<HashMap<NodeId, HashSet<NodeId>>> {
        let mut adjacency: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, HashSet::new());
            }
        }

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from != to {
                    adjacency.entry(from).or_default().insert(to);
                    adjacency.entry(to).or_default().insert(from);
                }
            }
        }

        Ok(adjacency)
    }
}

/// 모든 극대 클리크 찾기
pub async fn find_all_maximal_cliques(engine: &GraphEngine) -> Result<CliqueResult> {
    BronKerbosch::new(engine).find_all_maximal_cliques().await
}

/// 최대 클리크 찾기
pub async fn find_maximum_clique(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    BronKerbosch::new(engine).find_maximum_clique().await
}
