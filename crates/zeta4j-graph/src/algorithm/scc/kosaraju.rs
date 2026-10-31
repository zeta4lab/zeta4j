// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Kosaraju's Strongly Connected Components Algorithm
//!
//! 두 번의 DFS를 사용하여 방향 그래프의 모든 강연결요소(SCC)를 찾습니다.
//! 첫 번째 DFS로 종료 순서를 구하고, 역방향 그래프에서 역순으로 DFS합니다.
//!
//! 시간복잡도: O(V + E)

use super::SCCResult;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// Kosaraju 알고리즘 설정
#[derive(Debug, Clone, Default)]
pub struct KosarajuConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl KosarajuConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Kosaraju's SCC 알고리즘
pub struct KosarajuSCC<'a> {
    engine: &'a GraphEngine,
    config: KosarajuConfig,
}

impl<'a> KosarajuSCC<'a> {
    /// 새로운 KosarajuSCC 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: KosarajuConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: KosarajuConfig) -> Self {
        Self { engine, config }
    }

    /// SCC 찾기
    pub async fn find_scc(&self) -> Result<SCCResult> {
        let (adjacency, reverse_adjacency) = self.build_adjacencies().await?;

        if adjacency.is_empty() {
            return Ok(SCCResult::empty());
        }

        // 1단계: 원본 그래프에서 DFS, 종료 순서 기록
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut finish_order: Vec<NodeId> = Vec::new();

        for &node in adjacency.keys() {
            if !visited.contains(&node) {
                self.dfs_first(&adjacency, node, &mut visited, &mut finish_order);
            }
        }

        // 2단계: 역방향 그래프에서 종료 역순으로 DFS
        visited.clear();
        let mut components: Vec<Vec<NodeId>> = Vec::new();

        for &node in finish_order.iter().rev() {
            if !visited.contains(&node) {
                let mut component: Vec<NodeId> = Vec::new();
                self.dfs_second(&reverse_adjacency, node, &mut visited, &mut component);
                component.sort();
                components.push(component);
            }
        }

        // component_map 구축
        let mut component_map: HashMap<NodeId, usize> = HashMap::new();
        for (idx, component) in components.iter().enumerate() {
            for &node in component {
                component_map.insert(node, idx);
            }
        }

        let count = components.len();
        let node_count = adjacency.len();
        let is_strongly_connected = count == 1 && !components.is_empty();

        Ok(SCCResult {
            components,
            component_map,
            count,
            is_strongly_connected,
            node_count,
            algorithm: "Kosaraju".to_string(),
        })
    }

    /// 1단계 DFS: 종료 순서 기록
    fn dfs_first(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node: NodeId,
        visited: &mut HashSet<NodeId>,
        finish_order: &mut Vec<NodeId>,
    ) {
        visited.insert(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                if !visited.contains(&neighbor) {
                    self.dfs_first(adjacency, neighbor, visited, finish_order);
                }
            }
        }

        finish_order.push(node);
    }

    /// 2단계 DFS: SCC 수집
    fn dfs_second(
        &self,
        reverse_adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node: NodeId,
        visited: &mut HashSet<NodeId>,
        component: &mut Vec<NodeId>,
    ) {
        visited.insert(node);
        component.push(node);

        if let Some(neighbors) = reverse_adjacency.get(&node) {
            for &neighbor in neighbors {
                if !visited.contains(&neighbor) {
                    self.dfs_second(reverse_adjacency, neighbor, visited, component);
                }
            }
        }
    }

    /// 인접 리스트 구축 (원본 + 역방향)
    async fn build_adjacencies(
        &self,
    ) -> Result<(HashMap<NodeId, Vec<NodeId>>, HashMap<NodeId, Vec<NodeId>>)> {
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut reverse_adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, Vec::new());
                reverse_adjacency.insert(node_id, Vec::new());
            }
        }

        // 엣지 수집
        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from != to {
                    adjacency.entry(from).or_default().push(to);
                    reverse_adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok((adjacency, reverse_adjacency))
    }
}

/// 편의 함수: Kosaraju SCC
pub async fn kosaraju_scc(engine: &GraphEngine) -> Result<SCCResult> {
    KosarajuSCC::new(engine).find_scc().await
}
