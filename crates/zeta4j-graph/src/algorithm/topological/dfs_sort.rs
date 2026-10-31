// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! DFS 기반 Topological Sort 알고리즘
//!
//! DFS 완료 역순을 이용한 위상 정렬입니다.
//! 사이클 탐지를 동시에 수행할 수 있습니다.
//!
//! ## 시간복잡도
//! - O(V + E)
//!
//! ## 특징
//! - 사이클 탐지 포함
//! - 재귀적 구현
//! - 역순으로 결과 반환

use super::kahn::{TopologicalAlgorithm, TopologicalConfig, TopologicalResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 노드 상태 (색상)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    White, // 미방문
    Gray,  // 방문 중
    Black, // 완료
}

/// DFS 기반 Topological Sort
pub struct DfsTopologicalSort<'a> {
    engine: &'a GraphEngine,
    config: TopologicalConfig,
}

impl<'a> DfsTopologicalSort<'a> {
    /// 새로운 DfsTopologicalSort 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: TopologicalConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: TopologicalConfig) -> Self {
        Self { engine, config }
    }

    /// DFS 기반 위상 정렬
    pub async fn sort(&self) -> Result<TopologicalResult> {
        let (nodes, adj) = self.build_graph().await?;

        if nodes.is_empty() {
            return Ok(TopologicalResult::empty(TopologicalAlgorithm::Dfs));
        }

        let total_nodes = nodes.len();
        let mut color: HashMap<NodeId, Color> = nodes.iter().map(|&n| (n, Color::White)).collect();
        let mut stack: Vec<NodeId> = Vec::new();
        let mut has_cycle = false;

        for &node in &nodes {
            if color[&node] == Color::White && !self.dfs_visit(node, &adj, &mut color, &mut stack) {
                has_cycle = true;
                break;
            }
        }

        // 역순이 위상 정렬
        stack.reverse();

        let processed_count = stack.len();

        Ok(TopologicalResult {
            order: stack,
            is_dag: !has_cycle,
            levels: None, // DFS는 레벨 정보 없음
            algorithm: TopologicalAlgorithm::Dfs,
            processed_count,
            total_nodes,
        })
    }

    /// DFS 방문
    fn dfs_visit(
        &self,
        node: NodeId,
        adj: &HashMap<NodeId, Vec<NodeId>>,
        color: &mut HashMap<NodeId, Color>,
        stack: &mut Vec<NodeId>,
    ) -> bool {
        color.insert(node, Color::Gray);

        if let Some(neighbors) = adj.get(&node) {
            for &neighbor in neighbors {
                match color.get(&neighbor).copied() {
                    Some(Color::Gray) => {
                        // Back edge - cycle detected
                        return false;
                    }
                    Some(Color::White) if !self.dfs_visit(neighbor, adj, color, stack) => {
                        return false;
                    }
                    _ => {}
                }
            }
        }

        color.insert(node, Color::Black);
        stack.push(node);
        true
    }

    /// 역위상 정렬 (종속 노드가 먼저)
    pub async fn reverse_sort(&self) -> Result<TopologicalResult> {
        let mut result = self.sort().await?;
        result.order.reverse();
        Ok(result)
    }

    /// 그래프 구축
    async fn build_graph(&self) -> Result<(Vec<NodeId>, HashMap<NodeId, Vec<NodeId>>)> {
        let mut nodes = Vec::new();
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 모든 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                adj.insert(node_id, Vec::new());
            }
        }

        // 모든 간선 수집 (방향 그래프)
        let edge_count = self.engine.edge_count().await;
        let mut seen_edges = HashSet::new();

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

                if from == to {
                    continue;
                }

                // 중복 간선 방지
                let edge_key = (from, to);
                if seen_edges.contains(&edge_key) {
                    continue;
                }
                seen_edges.insert(edge_key);

                if adj.contains_key(&from)
                    && adj.contains_key(&to)
                    && let Some(neighbors) = adj.get_mut(&from)
                {
                    neighbors.push(to);
                }
            }
        }

        Ok((nodes, adj))
    }
}

/// DFS 위상 정렬 편의 함수
pub async fn dfs_topological_sort(engine: &GraphEngine) -> Result<TopologicalResult> {
    DfsTopologicalSort::new(engine).sort().await
}
