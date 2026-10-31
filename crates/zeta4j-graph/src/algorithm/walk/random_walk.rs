// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Basic Random Walk 알고리즘
//!
//! 그래프에서 무작위로 이웃 노드를 선택하며 이동하는 알고리즘입니다.
//! 추천 시스템, 그래프 샘플링, 임베딩 등에 활용됩니다.
//!
//! ## 알고리즘 개요
//! 1. 시작 노드에서 출발
//! 2. 현재 노드의 이웃 중 하나를 무작위 선택
//! 3. 선택한 노드로 이동
//! 4. 지정된 길이만큼 반복

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use rand::prelude::*;
use std::collections::HashMap;

/// 탐색 방향
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WalkDirection {
    /// 나가는 엣지만 (기본값)
    Outgoing,
    /// 들어오는 엣지만
    Incoming,
    /// 양방향
    #[default]
    Both,
}

/// Random Walk 설정
#[derive(Debug, Clone)]
pub struct RandomWalkConfig {
    /// 워크 길이 (스텝 수)
    pub walk_length: usize,
    /// 시작 노드당 워크 수
    pub walks_per_node: usize,
    /// 관계 타입 필터 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 가중치 속성명 (None이면 균등 확률)
    pub weight_property: Option<String>,
    /// 탐색 방향
    pub direction: WalkDirection,
    /// 랜덤 시드 (재현성을 위해)
    pub seed: Option<u64>,
}

impl Default for RandomWalkConfig {
    fn default() -> Self {
        Self {
            walk_length: 10,
            walks_per_node: 1,
            relationship_types: None,
            weight_property: None,
            direction: WalkDirection::Both,
            seed: None,
        }
    }
}

impl RandomWalkConfig {
    /// 워크 길이 설정
    pub fn with_walk_length(mut self, length: usize) -> Self {
        self.walk_length = length;
        self
    }

    /// 노드당 워크 수 설정
    pub fn with_walks_per_node(mut self, count: usize) -> Self {
        self.walks_per_node = count;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 가중치 속성 설정
    pub fn with_weight_property(mut self, property: String) -> Self {
        self.weight_property = Some(property);
        self
    }

    /// 탐색 방향 설정
    pub fn with_direction(mut self, direction: WalkDirection) -> Self {
        self.direction = direction;
        self
    }

    /// 랜덤 시드 설정
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

/// Random Walk 결과
#[derive(Debug, Clone)]
pub struct RandomWalkResult {
    /// 워크 경로들 (각 워크는 노드 ID 시퀀스)
    pub walks: Vec<Vec<NodeId>>,
    /// 시작 노드
    pub start_node: NodeId,
    /// 총 워크 수
    pub walk_count: usize,
    /// 평균 워크 길이
    pub average_length: f64,
    /// 설정된 최대 길이
    pub max_length: usize,
}

impl RandomWalkResult {
    /// 특정 인덱스의 워크 반환
    pub fn get_walk(&self, index: usize) -> Option<&Vec<NodeId>> {
        self.walks.get(index)
    }

    /// 첫 번째 워크 반환
    pub fn first_walk(&self) -> Option<&Vec<NodeId>> {
        self.walks.first()
    }

    /// 모든 워크에서 방문한 고유 노드들
    pub fn unique_nodes(&self) -> Vec<NodeId> {
        let mut nodes: Vec<NodeId> = self.walks.iter().flatten().copied().collect();
        nodes.sort();
        nodes.dedup();
        nodes
    }

    /// 노드별 방문 횟수
    pub fn visit_counts(&self) -> HashMap<NodeId, usize> {
        let mut counts = HashMap::new();
        for walk in &self.walks {
            for &node in walk {
                *counts.entry(node).or_insert(0) += 1;
            }
        }
        counts
    }

    /// 가장 많이 방문한 노드들 (상위 N개)
    pub fn most_visited(&self, n: usize) -> Vec<(NodeId, usize)> {
        let mut counts: Vec<_> = self.visit_counts().into_iter().collect();
        counts.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        counts.truncate(n);
        counts
    }
}

/// Random Walk 알고리즘 실행기
pub struct RandomWalk<'a> {
    engine: &'a GraphEngine,
    config: RandomWalkConfig,
}

impl<'a> RandomWalk<'a> {
    /// 새로운 RandomWalk 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: RandomWalkConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: RandomWalkConfig) -> Self {
        Self { engine, config }
    }

    /// 단일 워크 수행
    pub async fn walk(&self, start_node: NodeId) -> Result<Vec<NodeId>> {
        let mut rng = self.create_rng();
        self.walk_with_rng(start_node, &mut rng).await
    }

    /// 다중 워크 수행
    pub async fn walks(&self, start_node: NodeId) -> Result<RandomWalkResult> {
        let mut rng = self.create_rng();
        let mut walks = Vec::with_capacity(self.config.walks_per_node);

        for _ in 0..self.config.walks_per_node {
            let path = self.walk_with_rng(start_node, &mut rng).await?;
            walks.push(path);
        }

        let total_length: usize = walks.iter().map(|w| w.len()).sum();
        let average_length = if walks.is_empty() {
            0.0
        } else {
            total_length as f64 / walks.len() as f64
        };

        Ok(RandomWalkResult {
            walk_count: walks.len(),
            start_node,
            average_length,
            max_length: self.config.walk_length,
            walks,
        })
    }

    /// 여러 시작 노드에서 워크 수행
    pub async fn walks_from_nodes(&self, start_nodes: &[NodeId]) -> Result<Vec<RandomWalkResult>> {
        let mut results = Vec::with_capacity(start_nodes.len());
        for &node in start_nodes {
            results.push(self.walks(node).await?);
        }
        Ok(results)
    }

    /// 모든 노드에서 워크 수행
    pub async fn walks_from_all_nodes(&self) -> Result<Vec<RandomWalkResult>> {
        let nodes = self.get_all_node_ids().await?;
        self.walks_from_nodes(&nodes).await
    }

    /// RNG 생성
    fn create_rng(&self) -> StdRng {
        match self.config.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        }
    }

    /// RNG를 사용한 단일 워크
    async fn walk_with_rng<R: Rng>(&self, start_node: NodeId, rng: &mut R) -> Result<Vec<NodeId>> {
        let mut path = Vec::with_capacity(self.config.walk_length + 1);
        path.push(start_node);

        let mut current = start_node;

        for _ in 0..self.config.walk_length {
            let neighbors = self.get_neighbors(current).await?;

            if neighbors.is_empty() {
                break;
            }

            let next = if let Some(ref weight_prop) = self.config.weight_property {
                self.weighted_choice(&neighbors, weight_prop, rng)?
            } else {
                match neighbors.choose(rng) {
                    Some(&n) => n,
                    None => break,
                }
            };

            path.push(next);
            current = next;
        }

        Ok(path)
    }

    /// 이웃 노드 가져오기
    async fn get_neighbors(&self, node_id: NodeId) -> Result<Vec<NodeId>> {
        let mut neighbors = Vec::new();
        let edge_count = self.engine.edge_count().await;

        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                match self.config.direction {
                    WalkDirection::Outgoing => {
                        if from == node_id {
                            neighbors.push(to);
                        }
                    }
                    WalkDirection::Incoming => {
                        if to == node_id {
                            neighbors.push(from);
                        }
                    }
                    WalkDirection::Both => {
                        if from == node_id {
                            neighbors.push(to);
                        } else if to == node_id {
                            neighbors.push(from);
                        }
                    }
                }
            }
        }

        Ok(neighbors)
    }

    /// 가중치 기반 선택
    fn weighted_choice<R: Rng>(
        &self,
        neighbors: &[NodeId],
        weight_prop: &str,
        rng: &mut R,
    ) -> Result<NodeId> {
        // 각 이웃에 대한 가중치 수집 (엣지 가중치 사용)
        let mut weights = Vec::with_capacity(neighbors.len());

        for &neighbor in neighbors {
            // 간단화를 위해 기본 가중치 1.0 사용
            // 실제로는 엣지를 찾아서 가중치를 읽어야 함
            let weight = self.get_edge_weight_to_neighbor(neighbor, weight_prop)?;
            weights.push(weight);
        }

        // 가중치 기반 랜덤 선택
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return Ok(match neighbors.choose(rng) {
                Some(&n) => n,
                None => neighbors[0],
            });
        }

        let mut r = rng.random::<f64>() * total;
        for (i, &w) in weights.iter().enumerate() {
            r -= w;
            if r <= 0.0 {
                return Ok(neighbors[i]);
            }
        }

        Ok(neighbors[neighbors.len() - 1])
    }

    /// 이웃으로 가는 엣지의 가중치
    fn get_edge_weight_to_neighbor(&self, _neighbor: NodeId, _weight_prop: &str) -> Result<f64> {
        // 간단화된 구현: 기본 가중치 1.0
        // 실제로는 엣지를 찾아서 속성을 읽어야 함
        Ok(1.0)
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
}

/// 기본 Random Walk 편의 함수
pub async fn random_walk(
    engine: &GraphEngine,
    start_node: NodeId,
    length: usize,
) -> Result<Vec<NodeId>> {
    let config = RandomWalkConfig::default().with_walk_length(length);
    RandomWalk::with_config(engine, config)
        .walk(start_node)
        .await
}

/// 다중 Random Walk 편의 함수
pub async fn random_walks(
    engine: &GraphEngine,
    start_node: NodeId,
    length: usize,
    count: usize,
) -> Result<RandomWalkResult> {
    let config = RandomWalkConfig::default()
        .with_walk_length(length)
        .with_walks_per_node(count);
    RandomWalk::with_config(engine, config)
        .walks(start_node)
        .await
}
