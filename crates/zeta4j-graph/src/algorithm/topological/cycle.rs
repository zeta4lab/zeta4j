// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Cycle Detection 알고리즘
//!
//! 그래프에서 사이클(순환)을 탐지합니다.
//! DFS 기반으로 방향 그래프의 back edge를 찾아 사이클을 탐지합니다.
//!
//! ## 시간복잡도
//! - O(V + E)
//!
//! ## 색상 기반 DFS
//! - WHITE: 미방문
//! - GRAY: 방문 중 (스택에 있음)
//! - BLACK: 방문 완료

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

/// Cycle Detection 설정
#[derive(Debug, Clone, Default)]
pub struct CycleConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl CycleConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 사이클 탐지 결과
#[derive(Debug, Clone)]
pub struct CycleResult {
    /// 사이클 존재 여부
    pub has_cycle: bool,
    /// 사이클 경로 (있는 경우)
    pub cycle_path: Option<Vec<NodeId>>,
    /// 탐지된 back edge (from, to)
    pub back_edge: Option<(NodeId, NodeId)>,
}

impl CycleResult {
    /// 사이클 없음
    pub fn no_cycle() -> Self {
        Self {
            has_cycle: false,
            cycle_path: None,
            back_edge: None,
        }
    }

    /// 사이클 발견
    pub fn with_cycle(path: Vec<NodeId>, back_edge: (NodeId, NodeId)) -> Self {
        Self {
            has_cycle: true,
            cycle_path: Some(path),
            back_edge: Some(back_edge),
        }
    }

    /// 사이클 길이
    pub fn cycle_length(&self) -> usize {
        self.cycle_path.as_ref().map(|p| p.len()).unwrap_or(0)
    }
}

/// Cycle Detector
pub struct CycleDetector<'a> {
    engine: &'a GraphEngine,
    config: CycleConfig,
}

impl<'a> CycleDetector<'a> {
    /// 새로운 CycleDetector 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: CycleConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: CycleConfig) -> Self {
        Self { engine, config }
    }

    /// 사이클 존재 여부 확인
    pub async fn has_cycle(&self) -> Result<bool> {
        let result = self.find_cycle().await?;
        Ok(result.has_cycle)
    }

    /// DAG 여부 확인 (사이클 없음)
    pub async fn is_dag(&self) -> Result<bool> {
        Ok(!self.has_cycle().await?)
    }

    /// 사이클 탐지 및 경로 반환
    pub async fn find_cycle(&self) -> Result<CycleResult> {
        let adj = self.build_directed_adjacency_list().await?;
        let nodes: Vec<NodeId> = adj.keys().cloned().collect();

        if nodes.is_empty() {
            return Ok(CycleResult::no_cycle());
        }

        let mut color: HashMap<NodeId, Color> = nodes.iter().map(|&n| (n, Color::White)).collect();
        let mut parent: HashMap<NodeId, NodeId> = HashMap::new();

        for &start_node in &nodes {
            if color[&start_node] == Color::White
                && let Some(result) = self.dfs_cycle(start_node, &adj, &mut color, &mut parent)?
            {
                return Ok(result);
            }
        }

        Ok(CycleResult::no_cycle())
    }

    /// 모든 사이클 찾기
    pub async fn find_all_cycles(&self) -> Result<Vec<Vec<NodeId>>> {
        let adj = self.build_directed_adjacency_list().await?;
        let nodes: Vec<NodeId> = adj.keys().cloned().collect();

        if nodes.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_cycles = Vec::new();
        let mut visited_globally = HashSet::new();

        for &start_node in &nodes {
            if !visited_globally.contains(&start_node) {
                let mut color: HashMap<NodeId, Color> =
                    nodes.iter().map(|&n| (n, Color::White)).collect();
                let mut path = Vec::new();

                self.dfs_all_cycles(start_node, &adj, &mut color, &mut path, &mut all_cycles)?;

                visited_globally.insert(start_node);
            }
        }

        Ok(all_cycles)
    }

    /// DFS로 사이클 탐지
    fn dfs_cycle(
        &self,
        node: NodeId,
        adj: &HashMap<NodeId, Vec<NodeId>>,
        color: &mut HashMap<NodeId, Color>,
        parent: &mut HashMap<NodeId, NodeId>,
    ) -> Result<Option<CycleResult>> {
        color.insert(node, Color::Gray);

        if let Some(neighbors) = adj.get(&node) {
            for &neighbor in neighbors {
                match color.get(&neighbor) {
                    Some(Color::Gray) => {
                        // Back edge found - cycle detected!
                        let cycle_path = self.reconstruct_cycle(node, neighbor, parent);
                        return Ok(Some(CycleResult::with_cycle(cycle_path, (node, neighbor))));
                    }
                    Some(Color::White) => {
                        parent.insert(neighbor, node);
                        if let Some(result) = self.dfs_cycle(neighbor, adj, color, parent)? {
                            return Ok(Some(result));
                        }
                    }
                    _ => {}
                }
            }
        }

        color.insert(node, Color::Black);
        Ok(None)
    }

    /// 사이클 경로 재구성
    fn reconstruct_cycle(
        &self,
        from: NodeId,
        to: NodeId,
        parent: &HashMap<NodeId, NodeId>,
    ) -> Vec<NodeId> {
        let mut path = vec![to];
        let mut current = from;

        while current != to {
            path.push(current);
            current = match parent.get(&current) {
                Some(&p) => p,
                None => break,
            };
        }

        path.push(to);
        path.reverse();
        path
    }

    /// DFS로 모든 사이클 찾기
    fn dfs_all_cycles(
        &self,
        node: NodeId,
        adj: &HashMap<NodeId, Vec<NodeId>>,
        color: &mut HashMap<NodeId, Color>,
        path: &mut Vec<NodeId>,
        cycles: &mut Vec<Vec<NodeId>>,
    ) -> Result<()> {
        color.insert(node, Color::Gray);
        path.push(node);

        if let Some(neighbors) = adj.get(&node) {
            for &neighbor in neighbors {
                match color.get(&neighbor) {
                    Some(Color::Gray) => {
                        // Back edge - extract cycle from path
                        if let Some(pos) = path.iter().position(|&n| n == neighbor) {
                            let mut cycle: Vec<NodeId> = path[pos..].to_vec();
                            cycle.push(neighbor);
                            cycles.push(cycle);
                        }
                    }
                    Some(Color::White) => {
                        self.dfs_all_cycles(neighbor, adj, color, path, cycles)?;
                    }
                    _ => {}
                }
            }
        }

        path.pop();
        color.insert(node, Color::Black);
        Ok(())
    }

    /// 방향 그래프 인접 리스트 구축
    async fn build_directed_adjacency_list(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 모든 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adj.insert(node_id, Vec::new());
            }
        }

        // 모든 간선 수집 (방향 그래프)
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

                if adj.contains_key(&from)
                    && adj.contains_key(&to)
                    && from != to
                    && let Some(neighbors) = adj.get_mut(&from)
                {
                    neighbors.push(to);
                }
            }
        }

        Ok(adj)
    }
}

/// 사이클 존재 여부 편의 함수
pub async fn has_cycle(engine: &GraphEngine) -> Result<bool> {
    CycleDetector::new(engine).has_cycle().await
}

/// DAG 여부 편의 함수
pub async fn is_dag(engine: &GraphEngine) -> Result<bool> {
    CycleDetector::new(engine).is_dag().await
}

/// 사이클 탐지 편의 함수
pub async fn find_cycle(engine: &GraphEngine) -> Result<CycleResult> {
    CycleDetector::new(engine).find_cycle().await
}
