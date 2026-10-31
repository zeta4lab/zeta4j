// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Node2Vec 스타일 Biased Random Walk
//!
//! Node2Vec 논문(Grover & Leskovec, 2016)의 편향된 랜덤 워크 알고리즘입니다.
//! p와 q 파라미터로 BFS/DFS 탐색 스타일을 조절합니다.
//!
//! ## 파라미터
//! - **p (return parameter)**: 이전 노드로 돌아갈 확률 조절
//!   - p > 1: 이전 노드로 돌아가지 않으려 함
//!   - p < 1: 이전 노드로 돌아가려 함 (지역 탐색)
//! - **q (in-out parameter)**: BFS vs DFS 스타일 조절
//!   - q > 1: BFS-like (너비 우선, 지역 탐색)
//!   - q < 1: DFS-like (깊이 우선, 원거리 탐색)
//!
//! ## 활용
//! - 그래프 임베딩 (Node2Vec, DeepWalk)
//! - 유사 노드 탐색
//! - 추천 시스템

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use rand::SeedableRng;
use rand::prelude::*;
use rand::rngs::StdRng;
use std::collections::{HashMap, HashSet};

/// Node2Vec Biased Walk 설정
#[derive(Debug, Clone)]
pub struct Node2VecConfig {
    /// 워크 길이
    pub walk_length: usize,
    /// 노드당 워크 수
    pub walks_per_node: usize,
    /// Return parameter (p) - 이전 노드로 돌아갈 확률 조절
    pub p: f64,
    /// In-out parameter (q) - BFS vs DFS 스타일 조절
    pub q: f64,
    /// 관계 타입 필터 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 시드 값 (재현 가능한 결과용)
    pub seed: Option<u64>,
}

impl Default for Node2VecConfig {
    fn default() -> Self {
        Self {
            walk_length: 80,
            walks_per_node: 10,
            p: 1.0,
            q: 1.0,
            relationship_types: None,
            seed: None,
        }
    }
}

impl Node2VecConfig {
    /// 워크 길이 설정
    pub fn with_walk_length(mut self, length: usize) -> Self {
        self.walk_length = length.max(1);
        self
    }

    /// 노드당 워크 수 설정
    pub fn with_walks_per_node(mut self, count: usize) -> Self {
        self.walks_per_node = count.max(1);
        self
    }

    /// Return parameter (p) 설정
    pub fn with_p(mut self, p: f64) -> Self {
        self.p = p.max(0.001); // 0 방지
        self
    }

    /// In-out parameter (q) 설정
    pub fn with_q(mut self, q: f64) -> Self {
        self.q = q.max(0.001); // 0 방지
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 시드 설정
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

/// Node2Vec Biased Walk 결과
#[derive(Debug, Clone)]
pub struct Node2VecResult {
    /// 워크 경로들
    pub walks: Vec<Vec<NodeId>>,
    /// 시작 노드
    pub start_node: NodeId,
    /// p 파라미터
    pub p: f64,
    /// q 파라미터
    pub q: f64,
    /// 총 워크 수
    pub walk_count: usize,
    /// 평균 워크 길이
    pub average_length: f64,
}

impl Node2VecResult {
    /// 특정 노드가 포함된 워크들
    pub fn walks_containing(&self, node_id: NodeId) -> Vec<&Vec<NodeId>> {
        self.walks
            .iter()
            .filter(|walk| walk.contains(&node_id))
            .collect()
    }

    /// 워크에서 노드 등장 횟수
    pub fn node_frequency(&self) -> HashMap<NodeId, usize> {
        let mut freq = HashMap::new();
        for walk in &self.walks {
            for &node in walk {
                *freq.entry(node).or_insert(0) += 1;
            }
        }
        freq
    }

    /// 가장 자주 등장하는 상위 N개 노드
    pub fn top_frequent_nodes(&self, n: usize) -> Vec<(NodeId, usize)> {
        let mut freq: Vec<_> = self.node_frequency().into_iter().collect();
        freq.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        freq.truncate(n);
        freq
    }

    /// 워크에서 엣지 등장 횟수 (co-occurrence)
    pub fn edge_cooccurrence(&self) -> HashMap<(NodeId, NodeId), usize> {
        let mut cooc = HashMap::new();
        for walk in &self.walks {
            for window in walk.windows(2) {
                let (a, b) = (window[0], window[1]);
                let key = if a < b { (a, b) } else { (b, a) };
                *cooc.entry(key).or_insert(0) += 1;
            }
        }
        cooc
    }
}

/// Node2Vec Biased Random Walk 알고리즘
pub struct BiasedRandomWalk<'a> {
    engine: &'a GraphEngine,
    config: Node2VecConfig,
}

impl<'a> BiasedRandomWalk<'a> {
    /// 새로운 BiasedRandomWalk 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: Node2VecConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: Node2VecConfig) -> Self {
        Self { engine, config }
    }

    /// 단일 biased walk 수행
    pub async fn walk(&self, start_node: NodeId) -> Result<Vec<NodeId>> {
        let mut rng = match self.config.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        };
        self.walk_with_rng(start_node, &mut rng).await
    }

    /// RNG를 받아 단일 walk 수행 (내부용)
    async fn walk_with_rng(&self, start_node: NodeId, rng: &mut StdRng) -> Result<Vec<NodeId>> {
        let neighbors_map = self.build_neighbors_map().await?;

        let mut path = vec![start_node];
        let mut current = start_node;
        let mut prev: Option<NodeId> = None;

        for _ in 1..self.config.walk_length {
            let neighbors = match neighbors_map.get(&current) {
                Some(n) if !n.is_empty() => n,
                _ => break,
            };

            // 전이 확률 계산
            let weights =
                self.calculate_transition_weights(prev, current, neighbors, &neighbors_map);

            // 가중치 기반 랜덤 선택
            let next = self.weighted_random_choice(neighbors, &weights, rng);

            path.push(next);
            prev = Some(current);
            current = next;
        }

        Ok(path)
    }

    /// 시작 노드에서 다중 walk 수행
    pub async fn run(&self, start_node: NodeId) -> Result<Node2VecResult> {
        let mut rng = match self.config.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        };

        let neighbors_map = self.build_neighbors_map().await?;
        let mut walks = Vec::with_capacity(self.config.walks_per_node);

        for _ in 0..self.config.walks_per_node {
            let walk = self.walk_internal(start_node, &neighbors_map, &mut rng)?;
            walks.push(walk);
        }

        let total_length: usize = walks.iter().map(|w| w.len()).sum();
        let average_length = if walks.is_empty() {
            0.0
        } else {
            total_length as f64 / walks.len() as f64
        };

        Ok(Node2VecResult {
            walks,
            start_node,
            p: self.config.p,
            q: self.config.q,
            walk_count: self.config.walks_per_node,
            average_length,
        })
    }

    /// 모든 노드에서 walk 생성
    pub async fn generate_walks(&self) -> Result<Vec<Vec<NodeId>>> {
        let mut rng = match self.config.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        };

        let neighbors_map = self.build_neighbors_map().await?;
        let nodes = self.get_all_node_ids().await?;
        let mut all_walks = Vec::new();

        for &node in &nodes {
            for _ in 0..self.config.walks_per_node {
                let walk = self.walk_internal(node, &neighbors_map, &mut rng)?;
                all_walks.push(walk);
            }
        }

        // 워크 셔플 (학습 시 배치 다양성을 위해)
        all_walks.shuffle(&mut rng);

        Ok(all_walks)
    }

    /// 내부 walk 함수 (이웃 맵 재사용)
    fn walk_internal(
        &self,
        start_node: NodeId,
        neighbors_map: &HashMap<NodeId, Vec<NodeId>>,
        rng: &mut StdRng,
    ) -> Result<Vec<NodeId>> {
        let mut path = vec![start_node];
        let mut current = start_node;
        let mut prev: Option<NodeId> = None;

        for _ in 1..self.config.walk_length {
            let neighbors = match neighbors_map.get(&current) {
                Some(n) if !n.is_empty() => n,
                _ => break,
            };

            let weights =
                self.calculate_transition_weights(prev, current, neighbors, neighbors_map);

            let next = self.weighted_random_choice(neighbors, &weights, rng);

            path.push(next);
            prev = Some(current);
            current = next;
        }

        Ok(path)
    }

    /// Node2Vec 전이 확률 가중치 계산
    fn calculate_transition_weights(
        &self,
        prev: Option<NodeId>,
        _current: NodeId,
        neighbors: &[NodeId],
        neighbors_map: &HashMap<NodeId, Vec<NodeId>>,
    ) -> Vec<f64> {
        let p = self.config.p;
        let q = self.config.q;

        // 이전 노드가 없으면 (첫 번째 스텝) 균등 확률
        let prev_node = match prev {
            Some(p) => p,
            None => return vec![1.0; neighbors.len()],
        };

        // 이전 노드의 이웃 집합 (빠른 조회용)
        let prev_neighbors: HashSet<NodeId> = neighbors_map
            .get(&prev_node)
            .map(|n| n.iter().cloned().collect())
            .unwrap_or_default();

        neighbors
            .iter()
            .map(|&neighbor| {
                if neighbor == prev_node {
                    // 이전 노드로 돌아가기: 1/p
                    1.0 / p
                } else if prev_neighbors.contains(&neighbor) {
                    // 이전 노드와도 연결된 노드 (거리 1): 1
                    1.0
                } else {
                    // 이전 노드와 연결 안 된 노드 (거리 2): 1/q
                    1.0 / q
                }
            })
            .collect()
    }

    /// 가중치 기반 랜덤 선택
    fn weighted_random_choice(
        &self,
        items: &[NodeId],
        weights: &[f64],
        rng: &mut StdRng,
    ) -> NodeId {
        let total: f64 = weights.iter().sum();
        if total <= 0.0 || items.is_empty() {
            return items.first().copied().unwrap_or(0);
        }

        let mut cumulative = 0.0;
        let threshold: f64 = rng.random::<f64>() * total;

        for (i, &weight) in weights.iter().enumerate() {
            cumulative += weight;
            if cumulative >= threshold {
                return items[i];
            }
        }

        *items.last().unwrap_or(&0)
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

    /// 이웃 맵 구축 (무방향)
    async fn build_neighbors_map(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
        let node_count = self.engine.node_count().await?;
        let mut neighbors_map: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                neighbors_map.insert(node_id, Vec::new());
            }
        }

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

                // 양방향 추가 (무방향 그래프로 처리)
                if neighbors_map.contains_key(&from)
                    && neighbors_map.contains_key(&to)
                    && from != to
                {
                    if let Some(neighbors) = neighbors_map.get_mut(&from) {
                        neighbors.push(to);
                    }
                    if let Some(neighbors) = neighbors_map.get_mut(&to) {
                        neighbors.push(from);
                    }
                }
            }
        }

        Ok(neighbors_map)
    }
}

/// Node2Vec walk 편의 함수
pub async fn node2vec_walk(
    engine: &GraphEngine,
    start_node: NodeId,
    walk_length: usize,
    p: f64,
    q: f64,
) -> Result<Vec<NodeId>> {
    let config = Node2VecConfig::default()
        .with_walk_length(walk_length)
        .with_p(p)
        .with_q(q);
    BiasedRandomWalk::with_config(engine, config)
        .walk(start_node)
        .await
}

/// Node2Vec 다중 walk 편의 함수
pub async fn node2vec_walks(
    engine: &GraphEngine,
    start_node: NodeId,
    walk_length: usize,
    walks_per_node: usize,
    p: f64,
    q: f64,
) -> Result<Node2VecResult> {
    let config = Node2VecConfig::default()
        .with_walk_length(walk_length)
        .with_walks_per_node(walks_per_node)
        .with_p(p)
        .with_q(q);
    BiasedRandomWalk::with_config(engine, config)
        .run(start_node)
        .await
}
