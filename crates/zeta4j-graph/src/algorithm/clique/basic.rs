//! 기본 Clique 연산
//!
//! 클리크 검증 및 특정 크기의 클리크 탐색
//!
//! 시간복잡도: is_clique O(k²), find_cliques_of_size O(n^k)

use super::CliqueConfig;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 기본 Clique 탐지기
pub struct CliqueDetector<'a> {
    engine: &'a GraphEngine,
    config: CliqueConfig,
}

impl<'a> CliqueDetector<'a> {
    /// 새로운 CliqueDetector 생성
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

    /// 주어진 노드들이 클리크인지 확인
    pub async fn is_clique(&self, nodes: &[NodeId]) -> Result<bool> {
        if nodes.len() <= 1 {
            return Ok(true);
        }

        let adjacency = self.build_adjacency().await?;

        // 모든 쌍이 연결되어 있는지 확인
        for (i, &u) in nodes.iter().enumerate() {
            for &v in nodes.iter().skip(i + 1) {
                let is_connected = adjacency
                    .get(&u)
                    .map(|neighbors| neighbors.contains(&v))
                    .unwrap_or(false);

                if !is_connected {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// 특정 크기의 모든 클리크 찾기
    pub async fn find_cliques_of_size(&self, k: usize) -> Result<Vec<Vec<NodeId>>> {
        if k == 0 {
            return Ok(vec![vec![]]);
        }

        let adjacency = self.build_adjacency().await?;
        let nodes: Vec<NodeId> = adjacency.keys().copied().collect();

        if k == 1 {
            return Ok(nodes.iter().map(|&n| vec![n]).collect());
        }

        if nodes.len() < k {
            return Ok(Vec::new());
        }

        let mut cliques = Vec::new();
        let max_cliques = self.config.max_cliques.unwrap_or(usize::MAX);

        // 조합 생성 및 클리크 검증
        self.find_cliques_recursive(
            &adjacency,
            &nodes,
            k,
            0,
            &mut Vec::new(),
            &mut cliques,
            max_cliques,
        );

        Ok(cliques)
    }

    /// 재귀적으로 k-클리크 탐색
    #[expect(clippy::too_many_arguments)]
    fn find_cliques_recursive(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        nodes: &[NodeId],
        k: usize,
        start: usize,
        current: &mut Vec<NodeId>,
        cliques: &mut Vec<Vec<NodeId>>,
        max_cliques: usize,
    ) {
        if cliques.len() >= max_cliques {
            return;
        }

        if current.len() == k {
            cliques.push(current.clone());
            return;
        }

        let remaining = k - current.len();
        if nodes.len() - start < remaining {
            return;
        }

        for i in start..nodes.len() {
            let node = nodes[i];

            // 현재 클리크의 모든 노드와 연결되어 있는지 확인
            let is_connected_to_all = current.iter().all(|&u| {
                adjacency
                    .get(&u)
                    .map(|neighbors| neighbors.contains(&node))
                    .unwrap_or(false)
            });

            if is_connected_to_all {
                current.push(node);
                self.find_cliques_recursive(
                    adjacency,
                    nodes,
                    k,
                    i + 1,
                    current,
                    cliques,
                    max_cliques,
                );
                current.pop();
            }
        }
    }

    /// 클리크 수 (최대 클리크 크기) - 휴리스틱
    pub async fn clique_number(&self) -> Result<usize> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(0);
        }

        // 그리디 휴리스틱: 가장 높은 차수의 노드부터 클리크 구성
        let mut nodes: Vec<(NodeId, usize)> = adjacency
            .iter()
            .map(|(&node, neighbors)| (node, neighbors.len()))
            .collect();

        nodes.sort_by_key(|node| std::cmp::Reverse(node.1));

        let mut max_clique_size = 1;

        for &(start_node, _) in &nodes {
            let mut clique = vec![start_node];
            let mut candidates: Vec<NodeId> = adjacency
                .get(&start_node)
                .map(|n| n.iter().copied().collect())
                .unwrap_or_default();

            // 차수 내림차순 정렬
            candidates.sort_by(|a, b| {
                let deg_a = adjacency.get(a).map(|n| n.len()).unwrap_or(0);
                let deg_b = adjacency.get(b).map(|n| n.len()).unwrap_or(0);
                deg_b.cmp(&deg_a)
            });

            for candidate in candidates {
                // 현재 클리크의 모든 노드와 연결되어 있는지 확인
                let is_connected_to_all = clique.iter().all(|&u| {
                    adjacency
                        .get(&u)
                        .map(|neighbors| neighbors.contains(&candidate))
                        .unwrap_or(false)
                });

                if is_connected_to_all {
                    clique.push(candidate);
                }
            }

            max_clique_size = max_clique_size.max(clique.len());
        }

        Ok(max_clique_size)
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

/// 주어진 노드들이 클리크인지 확인
pub async fn is_clique(engine: &GraphEngine, nodes: &[NodeId]) -> Result<bool> {
    CliqueDetector::new(engine).is_clique(nodes).await
}

/// 특정 크기의 모든 클리크 찾기
pub async fn find_cliques_of_size(engine: &GraphEngine, k: usize) -> Result<Vec<Vec<NodeId>>> {
    CliqueDetector::new(engine).find_cliques_of_size(k).await
}

/// 클리크 수 (최대 클리크 크기의 휴리스틱 근사값)
pub async fn clique_number(engine: &GraphEngine) -> Result<usize> {
    CliqueDetector::new(engine).clique_number().await
}
