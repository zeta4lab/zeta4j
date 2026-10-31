// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Dijkstra 가중치 최단 경로 알고리즘
//!
//! 가중치가 있는 그래프에서 두 노드 간 최단 경로를 찾습니다.

use crate::core::{EdgeId, NodeId, PropertyContainer, Value};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Dijkstra 설정
#[derive(Debug, Clone)]
pub struct DijkstraConfig {
    /// 가중치 프로퍼티 이름
    pub weight_property: String,
    /// 기본 가중치 (프로퍼티가 없는 경우)
    pub default_weight: f64,
    /// 최대 비용 제한 (None이면 무제한)
    pub max_cost: Option<f64>,
}

impl Default for DijkstraConfig {
    fn default() -> Self {
        Self {
            weight_property: "weight".to_string(),
            default_weight: 1.0,
            max_cost: None,
        }
    }
}

impl DijkstraConfig {
    /// 가중치 프로퍼티 설정
    pub fn with_weight_property(mut self, property: &str) -> Self {
        self.weight_property = property.to_string();
        self
    }

    /// 기본 가중치 설정
    pub fn with_default_weight(mut self, weight: f64) -> Self {
        self.default_weight = weight;
        self
    }

    /// 최대 비용 제한 설정
    pub fn with_max_cost(mut self, cost: f64) -> Self {
        self.max_cost = Some(cost);
        self
    }
}

/// Dijkstra 최단 경로 결과
#[derive(Debug, Clone)]
pub struct DijkstraResult {
    /// 경로상의 노드들
    pub nodes: Vec<NodeId>,
    /// 경로상의 엣지들
    pub edges: Vec<EdgeId>,
    /// 총 비용
    pub total_cost: f64,
}

/// 우선순위 큐 상태
#[derive(Debug, Clone)]
struct State {
    cost: f64,
    node: NodeId,
    path: Vec<NodeId>,
    edges: Vec<EdgeId>,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.node == other.node
    }
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        // 최소 힙을 위해 역순 비교
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Dijkstra 가중치 최단 경로 탐색기
pub struct Dijkstra<'a> {
    engine: &'a GraphEngine,
    config: DijkstraConfig,
}

impl<'a> Dijkstra<'a> {
    /// 새로운 Dijkstra 탐색기 생성
    pub fn new(engine: &'a GraphEngine, config: DijkstraConfig) -> Self {
        Self { engine, config }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults(engine: &'a GraphEngine) -> Self {
        Self::new(engine, DijkstraConfig::default())
    }

    /// 두 노드 간 최단 경로 찾기
    ///
    /// # Arguments
    /// * `start` - 시작 노드 ID
    /// * `end` - 끝 노드 ID
    /// * `rel_types` - 탐색할 관계 타입들 (None이면 모든 타입)
    /// * `directed` - 방향성 고려 여부
    ///
    /// # Returns
    /// * `Ok(Some(result))` - 경로를 찾은 경우
    /// * `Ok(None)` - 경로가 없는 경우
    pub async fn find(
        &self,
        start: NodeId,
        end: NodeId,
        rel_types: Option<&[String]>,
        directed: bool,
    ) -> Result<Option<DijkstraResult>> {
        // 시작과 끝이 같으면 바로 반환
        if start == end {
            return Ok(Some(DijkstraResult {
                nodes: vec![start],
                edges: vec![],
                total_cost: 0.0,
            }));
        }

        let mut heap = BinaryHeap::new();
        let mut distances: HashMap<NodeId, f64> = HashMap::new();

        heap.push(State {
            cost: 0.0,
            node: start,
            path: vec![start],
            edges: vec![],
        });
        distances.insert(start, 0.0);

        while let Some(State {
            cost,
            node,
            path,
            edges,
        }) = heap.pop()
        {
            // 목적지 도착
            if node == end {
                return Ok(Some(DijkstraResult {
                    nodes: path,
                    edges,
                    total_cost: cost,
                }));
            }

            // 이미 더 짧은 경로로 방문한 경우 건너뛰기
            if cost > *distances.get(&node).unwrap_or(&f64::INFINITY) {
                continue;
            }

            // 최대 비용 초과 시 건너뛰기
            if let Some(max_cost) = self.config.max_cost
                && cost > max_cost
            {
                continue;
            }

            // 이웃 탐색
            for (neighbor, edge_id, weight) in self
                .get_weighted_neighbors(node, rel_types, directed)
                .await?
            {
                let next_cost = cost + weight;

                // 더 짧은 경로 발견
                if next_cost < *distances.get(&neighbor).unwrap_or(&f64::INFINITY) {
                    distances.insert(neighbor, next_cost);

                    let mut new_path = path.clone();
                    new_path.push(neighbor);
                    let mut new_edges = edges.clone();
                    new_edges.push(edge_id);

                    heap.push(State {
                        cost: next_cost,
                        node: neighbor,
                        path: new_path,
                        edges: new_edges,
                    });
                }
            }
        }

        // 경로를 찾지 못함
        Ok(None)
    }

    /// 시작 노드에서 모든 노드까지의 최단 거리 계산
    ///
    /// # Returns
    /// 각 노드까지의 (경로, 비용) 매핑
    pub async fn find_all_from(
        &self,
        start: NodeId,
        rel_types: Option<&[String]>,
        directed: bool,
    ) -> Result<HashMap<NodeId, DijkstraResult>> {
        let mut results: HashMap<NodeId, DijkstraResult> = HashMap::new();
        let mut heap = BinaryHeap::new();
        let mut distances: HashMap<NodeId, f64> = HashMap::new();

        heap.push(State {
            cost: 0.0,
            node: start,
            path: vec![start],
            edges: vec![],
        });
        distances.insert(start, 0.0);

        while let Some(State {
            cost,
            node,
            path,
            edges,
        }) = heap.pop()
        {
            // 이미 처리된 노드 건너뛰기
            if results.contains_key(&node) {
                continue;
            }

            // 결과 저장
            results.insert(
                node,
                DijkstraResult {
                    nodes: path.clone(),
                    edges: edges.clone(),
                    total_cost: cost,
                },
            );

            // 최대 비용 초과 시 건너뛰기
            if let Some(max_cost) = self.config.max_cost
                && cost > max_cost
            {
                continue;
            }

            // 이웃 탐색
            for (neighbor, edge_id, weight) in self
                .get_weighted_neighbors(node, rel_types, directed)
                .await?
            {
                let next_cost = cost + weight;

                if next_cost < *distances.get(&neighbor).unwrap_or(&f64::INFINITY) {
                    distances.insert(neighbor, next_cost);

                    let mut new_path = path.clone();
                    new_path.push(neighbor);
                    let mut new_edges = edges.clone();
                    new_edges.push(edge_id);

                    heap.push(State {
                        cost: next_cost,
                        node: neighbor,
                        path: new_path,
                        edges: new_edges,
                    });
                }
            }
        }

        Ok(results)
    }

    /// 노드의 가중치가 있는 이웃들 가져오기
    async fn get_weighted_neighbors(
        &self,
        node: NodeId,
        rel_types: Option<&[String]>,
        directed: bool,
    ) -> Result<Vec<(NodeId, EdgeId, f64)>> {
        let edge_ids = self.engine.find_edges_by_node(node)?;
        let mut neighbors = Vec::new();

        for edge_id in edge_ids {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(types) = rel_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                // 방향성에 따라 이웃 결정
                let neighbor = if directed {
                    if edge.from_node() == node {
                        Some(edge.to_node())
                    } else {
                        None
                    }
                } else {
                    edge.other_node(node)
                };

                if let Some(neighbor_id) = neighbor {
                    // 가중치 추출
                    let weight = self.extract_weight(&edge)?;
                    neighbors.push((neighbor_id, edge_id, weight));
                }
            }
        }

        Ok(neighbors)
    }

    /// 엣지에서 가중치 추출
    fn extract_weight(&self, edge: &crate::core::Edge) -> Result<f64> {
        match edge.get_property(&self.config.weight_property) {
            Some(value) => match value {
                Value::Integer(i) => Ok(*i as f64),
                Value::Float(f) => Ok(*f),
                _ => Ok(self.config.default_weight),
            },
            None => Ok(self.config.default_weight),
        }
    }
}
