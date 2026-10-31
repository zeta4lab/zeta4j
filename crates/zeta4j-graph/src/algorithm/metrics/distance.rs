// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 거리 기반 그래프 메트릭
//!
//! - Eccentricity: 노드에서 가장 먼 노드까지의 거리
//! - Diameter: 그래프의 최대 이심률 (최장 최단 경로)
//! - Radius: 그래프의 최소 이심률
//! - Average Path Length: 평균 최단 경로 길이

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// 거리 메트릭 설정
#[derive(Debug, Clone, Default)]
pub struct DistanceConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 방향 고려 여부
    pub directed: bool,
}

impl DistanceConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 관계 타입 필터 설정 (Option)
    pub fn with_relationship_types_opt(mut self, types: Option<Vec<String>>) -> Self {
        self.relationship_types = types;
        self
    }

    /// 방향 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }
}

/// 거리 메트릭 계산기
pub struct DistanceMetrics<'a> {
    engine: &'a GraphEngine,
    config: DistanceConfig,
}

impl<'a> DistanceMetrics<'a> {
    /// 새로운 DistanceMetrics 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: DistanceConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: DistanceConfig) -> Self {
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

                if self.config.directed {
                    // 방향 고려: 나가는 엣지만
                    if from == node_id {
                        neighbors.insert(to);
                    }
                } else {
                    // 양방향
                    if from == node_id {
                        neighbors.insert(to);
                    } else if to == node_id {
                        neighbors.insert(from);
                    }
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

    /// BFS를 사용하여 한 노드에서 모든 노드까지의 최단 거리 계산
    async fn bfs_distances(&self, start: NodeId) -> Result<HashMap<NodeId, u64>> {
        let mut distances = HashMap::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        distances.insert(start, 0);
        visited.insert(start);
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            let current_dist = *distances.get(&current).unwrap_or(&0);
            let neighbors = self.get_neighbors(current).await?;

            for neighbor in neighbors {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    distances.insert(neighbor, current_dist + 1);
                    queue.push_back(neighbor);
                }
            }
        }

        Ok(distances)
    }

    /// 노드의 이심률 (Eccentricity)
    ///
    /// 해당 노드에서 가장 먼 노드까지의 최단 경로 길이
    ///
    /// # Returns
    /// - Some(eccentricity): 연결된 그래프에서의 이심률
    /// - None: 그래프가 연결되지 않은 경우 (일부 노드에 도달 불가)
    pub async fn eccentricity(&self, node_id: NodeId) -> Result<Option<u64>> {
        let all_nodes = self.get_all_node_ids().await?;
        let distances = self.bfs_distances(node_id).await?;

        // 그래프가 연결되어 있지 않으면 None 반환
        // (모든 노드에 도달할 수 없음)
        if distances.len() < all_nodes.len() {
            return Ok(None);
        }

        // 단일 노드인 경우 이심률은 0
        if all_nodes.len() == 1 {
            return Ok(Some(0));
        }

        let max_dist = distances.values().max().copied();
        Ok(max_dist)
    }

    /// 그래프의 직경 (Diameter)
    ///
    /// 모든 노드 쌍 간 최단 경로 중 가장 긴 것
    /// = max(eccentricity)
    ///
    /// # Returns
    /// - Some(diameter): 연결된 그래프의 직경
    /// - None: 그래프가 비어있거나 연결되지 않은 경우
    pub async fn diameter(&self) -> Result<Option<u64>> {
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(None);
        }

        let mut max_eccentricity: Option<u64> = None;

        for node_id in &nodes {
            if let Some(ecc) = self.eccentricity(*node_id).await? {
                max_eccentricity = Some(max_eccentricity.map_or(ecc, |m| m.max(ecc)));
            }
        }

        Ok(max_eccentricity)
    }

    /// 그래프의 반경 (Radius)
    ///
    /// 모든 노드 중 가장 작은 이심률
    /// = min(eccentricity)
    ///
    /// # Returns
    /// - Some(radius): 연결된 그래프의 반경
    /// - None: 그래프가 비어있거나 연결되지 않은 경우
    pub async fn radius(&self) -> Result<Option<u64>> {
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(None);
        }

        let mut min_eccentricity: Option<u64> = None;

        for node_id in &nodes {
            if let Some(ecc) = self.eccentricity(*node_id).await? {
                min_eccentricity = Some(min_eccentricity.map_or(ecc, |m| m.min(ecc)));
            }
        }

        Ok(min_eccentricity)
    }

    /// 중심 노드들 (Center)
    ///
    /// 이심률이 반경과 같은 노드들
    pub async fn center(&self) -> Result<Vec<NodeId>> {
        let radius = match self.radius().await? {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let nodes = self.get_all_node_ids().await?;
        let mut center_nodes = Vec::new();

        for node_id in nodes {
            if let Some(ecc) = self.eccentricity(node_id).await?
                && ecc == radius
            {
                center_nodes.push(node_id);
            }
        }

        Ok(center_nodes)
    }

    /// 평균 경로 길이 (Average Path Length)
    ///
    /// 모든 연결된 노드 쌍 간의 평균 최단 경로 길이
    ///
    /// # Returns
    /// - Some(avg): 평균 경로 길이
    /// - None: 그래프가 비어있거나 연결된 쌍이 없는 경우
    pub async fn average_path_length(&self) -> Result<Option<f64>> {
        let nodes = self.get_all_node_ids().await?;

        if nodes.len() <= 1 {
            return Ok(None);
        }

        let mut total_distance: u64 = 0;
        let mut pair_count: u64 = 0;

        for node_id in &nodes {
            let distances = self.bfs_distances(*node_id).await?;

            for (target, dist) in distances {
                if target != *node_id {
                    total_distance += dist;
                    pair_count += 1;
                }
            }
        }

        if pair_count == 0 {
            return Ok(None);
        }

        Ok(Some(total_distance as f64 / pair_count as f64))
    }

    /// 모든 노드의 이심률 계산
    pub async fn all_eccentricities(&self) -> Result<Vec<(NodeId, Option<u64>)>> {
        let nodes = self.get_all_node_ids().await?;
        let mut results = Vec::new();

        for node_id in nodes {
            let ecc = self.eccentricity(node_id).await?;
            results.push((node_id, ecc));
        }

        Ok(results)
    }

    /// 주변 노드들 (Peripheral)
    ///
    /// 이심률이 직경과 같은 노드들 (그래프의 가장자리에 있는 노드)
    pub async fn peripheral(&self) -> Result<Vec<NodeId>> {
        let diam = match self.diameter().await? {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };

        let nodes = self.get_all_node_ids().await?;
        let mut peripheral_nodes = Vec::new();

        for node_id in nodes {
            if let Some(ecc) = self.eccentricity(node_id).await?
                && ecc == diam
            {
                peripheral_nodes.push(node_id);
            }
        }

        Ok(peripheral_nodes)
    }
}

/// 노드의 이심률 계산 (편의 함수)
pub async fn eccentricity(engine: &GraphEngine, node_id: NodeId) -> Result<Option<u64>> {
    DistanceMetrics::new(engine).eccentricity(node_id).await
}

/// 그래프 직경 계산 (편의 함수)
pub async fn diameter(engine: &GraphEngine) -> Result<Option<u64>> {
    DistanceMetrics::new(engine).diameter().await
}

/// 그래프 반경 계산 (편의 함수)
pub async fn radius(engine: &GraphEngine) -> Result<Option<u64>> {
    DistanceMetrics::new(engine).radius().await
}

/// 평균 경로 길이 계산 (편의 함수)
pub async fn average_path_length(engine: &GraphEngine) -> Result<Option<f64>> {
    DistanceMetrics::new(engine).average_path_length().await
}
