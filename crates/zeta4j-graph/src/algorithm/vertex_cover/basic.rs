// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 기본 Vertex Cover 연산
//!
//! 정점 커버 검증 및 Greedy 2-근사 알고리즘
//!
//! 시간복잡도: is_vertex_cover O(E), greedy_vertex_cover O(E)

use super::{VertexCoverConfig, VertexCoverResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 기본 Vertex Cover 탐지기
pub struct VertexCoverDetector<'a> {
    engine: &'a GraphEngine,
    config: VertexCoverConfig,
}

impl<'a> VertexCoverDetector<'a> {
    /// 새로운 VertexCoverDetector 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: VertexCoverConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: VertexCoverConfig) -> Self {
        Self { engine, config }
    }

    /// 주어진 노드들이 정점 커버인지 확인
    pub async fn is_vertex_cover(&self, nodes: &[NodeId]) -> Result<bool> {
        let edges = self.get_edges().await?;

        if edges.is_empty() {
            return Ok(true); // 엣지 없으면 빈 집합도 커버
        }

        let cover_set: HashSet<NodeId> = nodes.iter().copied().collect();

        // 모든 엣지가 커버에 인접한지 확인
        for (u, v) in &edges {
            if !cover_set.contains(u) && !cover_set.contains(v) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Greedy 2-근사 알고리즘으로 정점 커버 찾기
    ///
    /// 임의의 엣지를 선택하고 양 끝점을 모두 커버에 추가
    pub async fn greedy_vertex_cover(&self) -> Result<VertexCoverResult> {
        let edges = self.get_edges().await?;

        if edges.is_empty() {
            return Ok(VertexCoverResult::empty());
        }

        let mut cover: Vec<NodeId> = Vec::new();
        let mut cover_set: HashSet<NodeId> = HashSet::new();
        let mut uncovered_edges: HashSet<(NodeId, NodeId)> = edges.into_iter().collect();

        while !uncovered_edges.is_empty() {
            // 임의의 커버되지 않은 엣지 선택
            let (u, v) = match uncovered_edges.iter().next() {
                Some(&edge) => edge,
                None => break,
            };

            // 양 끝점 모두 커버에 추가
            if !cover_set.contains(&u) {
                cover.push(u);
                cover_set.insert(u);
            }
            if !cover_set.contains(&v) {
                cover.push(v);
                cover_set.insert(v);
            }

            // u 또는 v에 인접한 엣지 제거
            uncovered_edges.retain(|(a, b)| !cover_set.contains(a) && !cover_set.contains(b));
        }

        cover.sort();

        Ok(VertexCoverResult {
            size: cover.len(),
            is_minimum: false, // Greedy는 근사
            uncovered_edges: 0,
            cover,
        })
    }

    /// 최대 차수 기반 Greedy 알고리즘 (더 나은 근사)
    ///
    /// 매번 가장 많은 미커버 엣지를 커버하는 노드 선택
    pub async fn greedy_max_degree(&self) -> Result<VertexCoverResult> {
        let edges = self.get_edges().await?;

        if edges.is_empty() {
            return Ok(VertexCoverResult::empty());
        }

        let mut cover: Vec<NodeId> = Vec::new();
        let mut cover_set: HashSet<NodeId> = HashSet::new();
        let mut uncovered_edges: HashSet<(NodeId, NodeId)> = edges.into_iter().collect();

        while !uncovered_edges.is_empty() {
            // 각 노드가 커버하는 미커버 엣지 수 계산
            let mut node_degrees: HashMap<NodeId, usize> = HashMap::new();

            for (u, v) in &uncovered_edges {
                if !cover_set.contains(u) {
                    *node_degrees.entry(*u).or_insert(0) += 1;
                }
                if !cover_set.contains(v) {
                    *node_degrees.entry(*v).or_insert(0) += 1;
                }
            }

            // 최대 차수 노드 선택
            if let Some((&best_node, _)) = node_degrees.iter().max_by_key(|&(_, &deg)| deg) {
                cover.push(best_node);
                cover_set.insert(best_node);

                // 해당 노드에 인접한 엣지 제거
                uncovered_edges.retain(|(a, b)| *a != best_node && *b != best_node);
            } else {
                break;
            }
        }

        cover.sort();

        Ok(VertexCoverResult {
            size: cover.len(),
            is_minimum: false,
            uncovered_edges: 0,
            cover,
        })
    }

    /// 정점 커버 수 (근사값)
    pub async fn vertex_cover_number(&self) -> Result<usize> {
        let result = self.greedy_max_degree().await?;
        Ok(result.size)
    }

    /// 엣지 목록 가져오기 (무방향)
    async fn get_edges(&self) -> Result<Vec<(NodeId, NodeId)>> {
        let mut edges = Vec::new();
        let mut seen: HashSet<(NodeId, NodeId)> = HashSet::new();

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
                    // 무방향 엣지로 처리 (정규화)
                    let edge_key = if from < to { (from, to) } else { (to, from) };
                    if !seen.contains(&edge_key) {
                        seen.insert(edge_key);
                        edges.push(edge_key);
                    }
                }
            }
        }

        Ok(edges)
    }
}

/// 주어진 노드들이 정점 커버인지 확인
pub async fn is_vertex_cover(engine: &GraphEngine, nodes: &[NodeId]) -> Result<bool> {
    VertexCoverDetector::new(engine)
        .is_vertex_cover(nodes)
        .await
}

/// Greedy 2-근사 알고리즘으로 정점 커버 찾기
pub async fn greedy_vertex_cover(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(VertexCoverDetector::new(engine)
        .greedy_vertex_cover()
        .await?
        .cover)
}

/// 최대 차수 기반 Greedy 알고리즘
pub async fn greedy_max_degree_cover(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(VertexCoverDetector::new(engine)
        .greedy_max_degree()
        .await?
        .cover)
}

/// 정점 커버 수 (근사값)
pub async fn vertex_cover_number(engine: &GraphEngine) -> Result<usize> {
    VertexCoverDetector::new(engine).vertex_cover_number().await
}
