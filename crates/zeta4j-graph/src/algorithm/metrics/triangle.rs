// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 삼각형 계수 (Triangle Count)
//!
//! 그래프에서 삼각형(3개 노드가 모두 연결된 패턴) 개수를 계산합니다.
//!
//! 삼각형은 그래프의 클러스터링 정도를 나타내는 중요한 지표입니다.

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashSet;

/// 삼각형 계수 설정
#[derive(Debug, Clone, Default)]
pub struct TriangleConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
}

impl TriangleConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 삼각형 계수 계산기
pub struct TriangleCount<'a> {
    engine: &'a GraphEngine,
    config: TriangleConfig,
}

impl<'a> TriangleCount<'a> {
    /// 새로운 TriangleCount 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: TriangleConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: TriangleConfig) -> Self {
        Self { engine, config }
    }

    /// 노드의 이웃 집합 가져오기
    async fn get_neighbors(&self, node_id: NodeId) -> Result<HashSet<NodeId>> {
        let mut neighbors = HashSet::new();
        let edge_ids = self.engine.get_connected_edges(node_id).await;

        for edge_id in edge_ids {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // 양방향으로 이웃 추가
                if from == node_id {
                    neighbors.insert(to);
                } else if to == node_id {
                    neighbors.insert(from);
                }
            }
        }

        Ok(neighbors)
    }

    /// 모든 노드 ID 가져오기
    async fn get_all_node_ids(&self) -> Result<Vec<NodeId>> {
        let mut nodes = Vec::new();
        let node_count = self.engine.node_count().await?;

        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
            }
        }

        Ok(nodes)
    }

    /// 특정 노드가 속한 삼각형 개수
    ///
    /// 노드 v가 속한 삼각형 = v의 이웃 쌍 중 서로 연결된 쌍의 수
    pub async fn node_count(&self, node_id: NodeId) -> Result<u64> {
        let neighbors = self.get_neighbors(node_id).await?;
        let neighbor_list: Vec<_> = neighbors.iter().cloned().collect();

        let mut count = 0u64;

        // 이웃 쌍에 대해 서로 연결되어 있는지 확인
        for i in 0..neighbor_list.len() {
            for j in (i + 1)..neighbor_list.len() {
                let n1 = neighbor_list[i];
                let n2 = neighbor_list[j];

                // n1과 n2가 연결되어 있으면 삼각형
                let n1_neighbors = self.get_neighbors(n1).await?;
                if n1_neighbors.contains(&n2) {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// 전체 그래프의 삼각형 개수
    ///
    /// 각 삼각형은 3개의 노드에 의해 3번 카운트되므로 3으로 나눔
    pub async fn total_count(&self) -> Result<u64> {
        let nodes = self.get_all_node_ids().await?;
        let mut total = 0u64;

        for node_id in nodes {
            total += self.node_count(node_id).await?;
        }

        // 각 삼각형이 3번 카운트되므로
        Ok(total / 3)
    }

    /// 모든 노드의 삼각형 개수 계산
    pub async fn all_counts(&self) -> Result<Vec<(NodeId, u64)>> {
        let nodes = self.get_all_node_ids().await?;
        let mut results = Vec::new();

        for node_id in nodes {
            let count = self.node_count(node_id).await?;
            results.push((node_id, count));
        }

        // 삼각형 개수 기준 내림차순 정렬
        results.sort_by_key(|result| std::cmp::Reverse(result.1));

        Ok(results)
    }
}

/// 전체 삼각형 개수 계산 (편의 함수)
pub async fn count_triangles(engine: &GraphEngine) -> Result<u64> {
    TriangleCount::new(engine).total_count().await
}

/// 노드별 삼각형 개수 계산 (편의 함수)
pub async fn count_node_triangles(engine: &GraphEngine, node_id: NodeId) -> Result<u64> {
    TriangleCount::new(engine).node_count(node_id).await
}
