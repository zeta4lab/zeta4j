// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Hierholzer 알고리즘 - Eulerian Path/Circuit 찾기
//!
//! 모든 엣지를 정확히 한 번씩 방문하는 경로를 찾습니다.
//!
//! 시간복잡도: O(E)

use super::{EulerianChecker, EulerianConfig};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// Eulerian 경로/회로 결과
#[derive(Debug, Clone)]
pub struct EulerianResult {
    /// 방문 노드 순서
    pub path: Vec<NodeId>,
    /// 방문 엣지 순서 (from, to)
    pub edges: Vec<(NodeId, NodeId)>,
    /// 회로 여부 (시작점 = 끝점)
    pub is_circuit: bool,
    /// 총 엣지 수
    pub edge_count: usize,
}

impl EulerianResult {
    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            path: Vec::new(),
            edges: Vec::new(),
            is_circuit: true,
            edge_count: 0,
        }
    }
}

/// Hierholzer 알고리즘
pub struct Hierholzer<'a> {
    engine: &'a GraphEngine,
    config: EulerianConfig,
}

impl<'a> Hierholzer<'a> {
    /// 새로운 Hierholzer 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: EulerianConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: EulerianConfig) -> Self {
        Self { engine, config }
    }

    /// 오일러 회로 찾기
    pub async fn find_circuit(&self) -> Result<Option<EulerianResult>> {
        let check = EulerianChecker::with_config(self.engine, self.config.clone())
            .check()
            .await?;

        if !check.has_eulerian_circuit {
            return Ok(None);
        }

        if check.edge_count == 0 {
            return Ok(Some(EulerianResult::empty()));
        }

        // 시작점: 아무 엣지가 있는 노드
        let start = check.path_start.unwrap_or(1);

        self.find_path_from(start, true).await
    }

    /// 오일러 경로 찾기
    pub async fn find_path(&self) -> Result<Option<EulerianResult>> {
        let check = EulerianChecker::with_config(self.engine, self.config.clone())
            .check()
            .await?;

        if !check.has_eulerian_path {
            return Ok(None);
        }

        if check.edge_count == 0 {
            return Ok(Some(EulerianResult::empty()));
        }

        // 시작점: 홀수 차수 노드 또는 아무 노드
        let start = check.path_start.unwrap_or(1);
        let is_circuit = check.has_eulerian_circuit;

        self.find_path_from(start, is_circuit).await
    }

    /// 지정된 시작점에서 경로 찾기
    async fn find_path_from(
        &self,
        start: NodeId,
        is_circuit: bool,
    ) -> Result<Option<EulerianResult>> {
        if self.config.directed {
            self.find_path_directed(start, is_circuit).await
        } else {
            self.find_path_undirected(start, is_circuit).await
        }
    }

    /// 무방향 그래프에서 오일러 경로 찾기
    async fn find_path_undirected(
        &self,
        start: NodeId,
        is_circuit: bool,
    ) -> Result<Option<EulerianResult>> {
        // 인접 리스트 구축 (가변)
        let mut adjacency: HashMap<NodeId, Vec<(NodeId, u64)>> = HashMap::new();

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
                    adjacency.entry(from).or_default().push((to, edge_id));
                    adjacency.entry(to).or_default().push((from, edge_id));
                }
            }
        }

        // 사용된 엣지 추적
        let mut used_edges: HashSet<u64> = HashSet::new();

        // Hierholzer 알고리즘
        let mut stack = vec![start];
        let mut path = Vec::new();
        let mut edges = Vec::new();

        while let Some(&current) = stack.last() {
            // 미사용 엣지 찾기
            let next = if let Some(neighbors) = adjacency.get_mut(&current) {
                let mut found = None;
                while let Some((neighbor, edge_id)) = neighbors.pop() {
                    if !used_edges.contains(&edge_id) {
                        used_edges.insert(edge_id);
                        found = Some((neighbor, edge_id));
                        break;
                    }
                }
                found
            } else {
                None
            };

            if let Some((next_node, _edge_id)) = next {
                stack.push(next_node);
            } else {
                // 더 이상 갈 곳이 없으면 경로에 추가
                if let Some(node) = stack.pop() {
                    path.push(node);
                }
            }
        }

        // 경로 역순
        path.reverse();

        // 엣지 리스트 생성
        for i in 0..path.len().saturating_sub(1) {
            edges.push((path[i], path[i + 1]));
        }

        Ok(Some(EulerianResult {
            path,
            edge_count: edges.len(),
            edges,
            is_circuit,
        }))
    }

    /// 방향 그래프에서 오일러 경로 찾기
    async fn find_path_directed(
        &self,
        start: NodeId,
        is_circuit: bool,
    ) -> Result<Option<EulerianResult>> {
        // 인접 리스트 구축 (가변)
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

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
                    adjacency.entry(from).or_default().push(to);
                }
            }
        }

        // Hierholzer 알고리즘
        let mut stack = vec![start];
        let mut path = Vec::new();

        while let Some(&current) = stack.last() {
            if let Some(neighbors) = adjacency.get_mut(&current) {
                if let Some(next) = neighbors.pop() {
                    stack.push(next);
                } else if let Some(node) = stack.pop() {
                    path.push(node);
                }
            } else if let Some(node) = stack.pop() {
                path.push(node);
            }
        }

        // 경로 역순
        path.reverse();

        // 엣지 리스트 생성
        let mut edges = Vec::new();
        for i in 0..path.len().saturating_sub(1) {
            edges.push((path[i], path[i + 1]));
        }

        Ok(Some(EulerianResult {
            path,
            edge_count: edges.len(),
            edges,
            is_circuit,
        }))
    }
}

/// 오일러 회로 찾기 (무방향)
pub async fn eulerian_circuit(engine: &GraphEngine) -> Result<Option<EulerianResult>> {
    Hierholzer::new(engine).find_circuit().await
}

/// 오일러 경로 찾기 (무방향)
pub async fn eulerian_path(engine: &GraphEngine) -> Result<Option<EulerianResult>> {
    Hierholzer::new(engine).find_path().await
}

/// 방향 그래프 오일러 회로 찾기
pub async fn eulerian_circuit_directed(engine: &GraphEngine) -> Result<Option<EulerianResult>> {
    let config = EulerianConfig::default().directed();
    Hierholzer::with_config(engine, config).find_circuit().await
}

/// 방향 그래프 오일러 경로 찾기
pub async fn eulerian_path_directed(engine: &GraphEngine) -> Result<Option<EulerianResult>> {
    let config = EulerianConfig::default().directed();
    Hierholzer::with_config(engine, config).find_path().await
}
