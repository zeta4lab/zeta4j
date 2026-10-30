//! Floyd-Warshall 모든 쌍 최단 경로 알고리즘
//!
//! 그래프의 모든 노드 쌍 간 최단 경로를 계산합니다.
//! 음수 가중치를 허용하지만 음수 사이클이 있으면 안 됩니다.
//!
//! 시간복잡도: O(V³)
//! 공간복잡도: O(V²)

use crate::core::{NodeId, PropertyContainer, Value};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Floyd-Warshall 설정
#[derive(Debug, Clone)]
pub struct FloydWarshallConfig {
    /// 가중치 프로퍼티 이름
    pub weight_property: String,
    /// 기본 가중치
    pub default_weight: f64,
    /// 무한대 값 (도달 불가능 표시)
    pub infinity: f64,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl Default for FloydWarshallConfig {
    fn default() -> Self {
        Self {
            weight_property: "weight".to_string(),
            default_weight: 1.0,
            infinity: f64::INFINITY,
            relationship_types: None,
        }
    }
}

impl FloydWarshallConfig {
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

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Floyd-Warshall 결과
#[derive(Debug, Clone)]
pub struct FloydWarshallResult {
    /// 거리 행렬 (node_i, node_j) -> distance
    pub distances: HashMap<(NodeId, NodeId), f64>,
    /// 다음 노드 (경로 재구성용) (node_i, node_j) -> next_hop
    pub next_hop: HashMap<(NodeId, NodeId), NodeId>,
    /// 노드 목록 (순서 유지)
    pub nodes: Vec<NodeId>,
    /// 음수 사이클 존재 여부
    pub has_negative_cycle: bool,
}

impl FloydWarshallResult {
    /// 두 노드 간 최단 거리
    pub fn distance(&self, from: NodeId, to: NodeId) -> f64 {
        *self.distances.get(&(from, to)).unwrap_or(&f64::INFINITY)
    }

    /// 두 노드 간 경로가 존재하는지
    pub fn path_exists(&self, from: NodeId, to: NodeId) -> bool {
        self.distances
            .get(&(from, to))
            .map(|d| d.is_finite())
            .unwrap_or(false)
    }

    /// 두 노드 간 경로 재구성
    pub fn get_path(&self, from: NodeId, to: NodeId) -> Vec<NodeId> {
        if !self.path_exists(from, to) {
            return Vec::new();
        }

        if from == to {
            return vec![from];
        }

        let mut path = vec![from];
        let mut current = from;

        while current != to {
            if let Some(&next) = self.next_hop.get(&(current, to)) {
                path.push(next);
                current = next;
            } else {
                return Vec::new(); // 경로 없음
            }
        }

        path
    }

    /// 모든 노드 쌍의 거리를 Vec으로 반환
    pub fn all_distances(&self) -> Vec<(NodeId, NodeId, f64)> {
        self.distances
            .iter()
            .filter(|&(_, &d)| d.is_finite())
            .map(|(&(from, to), &dist)| (from, to, dist))
            .collect()
    }

    /// 특정 노드에서의 모든 거리
    pub fn distances_from(&self, source: NodeId) -> HashMap<NodeId, f64> {
        self.distances
            .iter()
            .filter(|((from, _), _)| *from == source)
            .map(|(&(_, to), &dist)| (to, dist))
            .collect()
    }

    /// 특정 노드로의 모든 거리
    pub fn distances_to(&self, target: NodeId) -> HashMap<NodeId, f64> {
        self.distances
            .iter()
            .filter(|((_, to), _)| *to == target)
            .map(|(&(from, _), &dist)| (from, dist))
            .collect()
    }

    /// 직경 (가장 긴 최단 경로)
    pub fn diameter(&self) -> f64 {
        self.distances
            .values()
            .filter(|d| d.is_finite())
            .cloned()
            .fold(0.0, f64::max)
    }

    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            distances: HashMap::new(),
            next_hop: HashMap::new(),
            nodes: Vec::new(),
            has_negative_cycle: false,
        }
    }
}

/// Floyd-Warshall 알고리즘
pub struct FloydWarshall<'a> {
    engine: &'a GraphEngine,
    config: FloydWarshallConfig,
}

impl<'a> FloydWarshall<'a> {
    /// 새로운 FloydWarshall 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: FloydWarshallConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: FloydWarshallConfig) -> Self {
        Self { engine, config }
    }

    /// 모든 쌍 최단 경로 계산
    ///
    /// # 메모리 제한
    ///
    /// Floyd-Warshall은 O(n²) 메모리를 사용합니다. 대규모 그래프(10,000+ 노드)에서는
    /// `streaming::ChunkedFloydWarshall`을 사용하거나 SSSP 기반 접근법을 권장합니다.
    #[expect(clippy::needless_range_loop)]
    pub async fn compute(&self) -> Result<FloydWarshallResult> {
        // 노드 수집
        let nodes = self.collect_nodes().await?;
        let n = nodes.len();

        if n == 0 {
            return Ok(FloydWarshallResult::empty());
        }

        // 메모리 보호: 10,000 노드 초과 시 에러
        // n² * 16바이트 (f64 거리 + Option<NodeId>) 예상 메모리
        const MAX_NODES: usize = 10_000;
        if n > MAX_NODES {
            let estimated_gb = (n * n * 16) / (1024 * 1024 * 1024);
            return Err(crate::error::GraphError::validation(format!(
                "Floyd-Warshall은 O(n²) 메모리가 필요합니다 ({} 노드 = ~{}GB). \
                 대규모 그래프에서는 streaming::ChunkedFloydWarshall 또는 SSSP 기반 알고리즘을 사용하세요.",
                n,
                estimated_gb.max(1)
            )));
        }

        // 노드 인덱스 매핑
        let node_to_idx: HashMap<NodeId, usize> =
            nodes.iter().enumerate().map(|(i, &n)| (n, i)).collect();

        // 거리 및 다음 노드 행렬 초기화
        let mut dist = vec![vec![self.config.infinity; n]; n];
        let mut next: Vec<Vec<Option<NodeId>>> = vec![vec![None; n]; n];

        // 자기 자신으로의 거리는 0
        for i in 0..n {
            dist[i][i] = 0.0;
            next[i][i] = Some(nodes[i]);
        }

        // 간선 정보로 초기화
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

                if from == to {
                    continue;
                }

                let from_idx = match node_to_idx.get(&from) {
                    Some(&i) => i,
                    None => continue,
                };
                let to_idx = match node_to_idx.get(&to) {
                    Some(&i) => i,
                    None => continue,
                };

                // 가중치 추출
                let weight = edge
                    .get_property(&self.config.weight_property)
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(self.config.default_weight);

                // 더 짧은 간선으로 업데이트
                if weight < dist[from_idx][to_idx] {
                    dist[from_idx][to_idx] = weight;
                    next[from_idx][to_idx] = Some(to);
                }
            }
        }

        // Floyd-Warshall 알고리즘
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    if dist[i][k] < self.config.infinity && dist[k][j] < self.config.infinity {
                        let new_dist = dist[i][k] + dist[k][j];
                        if new_dist < dist[i][j] {
                            dist[i][j] = new_dist;
                            next[i][j] = next[i][k];
                        }
                    }
                }
            }
        }

        // 음수 사이클 검출
        let mut has_negative_cycle = false;
        for i in 0..n {
            if dist[i][i] < 0.0 {
                has_negative_cycle = true;
                break;
            }
        }

        // 결과 변환
        let mut distances = HashMap::new();
        let mut next_hop = HashMap::new();

        for i in 0..n {
            for j in 0..n {
                let from = nodes[i];
                let to = nodes[j];

                if dist[i][j] < self.config.infinity {
                    distances.insert((from, to), dist[i][j]);
                }

                if let Some(next_node) = next[i][j] {
                    next_hop.insert((from, to), next_node);
                }
            }
        }

        Ok(FloydWarshallResult {
            distances,
            next_hop,
            nodes,
            has_negative_cycle,
        })
    }

    /// 모든 노드 수집
    async fn collect_nodes(&self) -> Result<Vec<NodeId>> {
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

/// 편의 함수: Floyd-Warshall 계산
pub async fn floyd_warshall(engine: &GraphEngine) -> Result<FloydWarshallResult> {
    FloydWarshall::new(engine).compute().await
}

/// 편의 함수: 가중치 프로퍼티 지정 Floyd-Warshall
pub async fn floyd_warshall_weighted(
    engine: &GraphEngine,
    weight_property: &str,
) -> Result<FloydWarshallResult> {
    let config = FloydWarshallConfig::default().with_weight_property(weight_property);
    FloydWarshall::with_config(engine, config).compute().await
}

/// 편의 함수: 모든 쌍 거리만 계산
pub async fn all_pairs_distances(engine: &GraphEngine) -> Result<HashMap<(NodeId, NodeId), f64>> {
    let result = FloydWarshall::new(engine).compute().await?;
    Ok(result.distances)
}

/// 편의 함수: 가중치 지정 모든 쌍 거리
pub async fn all_pairs_distances_weighted(
    engine: &GraphEngine,
    weight_property: &str,
) -> Result<HashMap<(NodeId, NodeId), f64>> {
    let config = FloydWarshallConfig::default().with_weight_property(weight_property);
    let result = FloydWarshall::with_config(engine, config).compute().await?;
    Ok(result.distances)
}
