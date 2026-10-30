//! Bellman-Ford 최단 경로 알고리즘
//!
//! 음수 가중치를 허용하는 단일 소스 최단 경로 알고리즘입니다.
//! 음수 사이클 감지 기능을 제공합니다.
//!
//! 시간복잡도: O(VE)

use crate::core::{EdgeId, NodeId, PropertyContainer, Value};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Bellman-Ford 설정
#[derive(Debug, Clone)]
pub struct BellmanFordConfig {
    /// 가중치 프로퍼티 이름
    pub weight_property: String,
    /// 기본 가중치 (프로퍼티가 없는 경우)
    pub default_weight: f64,
    /// 관계 타입 필터 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
}

impl Default for BellmanFordConfig {
    fn default() -> Self {
        Self {
            weight_property: "weight".to_string(),
            default_weight: 1.0,
            relationship_types: None,
        }
    }
}

impl BellmanFordConfig {
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

/// Bellman-Ford 결과
#[derive(Debug, Clone)]
pub struct BellmanFordResult {
    /// 경로상의 노드들
    pub nodes: Vec<NodeId>,
    /// 경로상의 엣지들
    pub edges: Vec<EdgeId>,
    /// 총 비용
    pub total_cost: f64,
    /// 음수 사이클 존재 여부
    pub has_negative_cycle: bool,
    /// 음수 사이클 경로 (있는 경우)
    pub negative_cycle: Option<Vec<NodeId>>,
    /// 소스 노드
    pub source: NodeId,
    /// 타겟 노드
    pub target: NodeId,
}

impl BellmanFordResult {
    /// 경로가 존재하는지 확인
    pub fn path_exists(&self) -> bool {
        !self.nodes.is_empty() && !self.has_negative_cycle
    }

    /// 경로 길이 (노드 수)
    pub fn path_length(&self) -> usize {
        self.nodes.len()
    }

    /// 빈 결과 생성
    pub fn empty(source: NodeId, target: NodeId) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            total_cost: f64::INFINITY,
            has_negative_cycle: false,
            negative_cycle: None,
            source,
            target,
        }
    }

    /// 음수 사이클 결과 생성
    pub fn negative_cycle(source: NodeId, target: NodeId, cycle: Vec<NodeId>) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            total_cost: f64::NEG_INFINITY,
            has_negative_cycle: true,
            negative_cycle: Some(cycle),
            source,
            target,
        }
    }
}

/// 간선 정보 (내부용)
#[derive(Debug, Clone)]
struct EdgeInfo {
    from: NodeId,
    to: NodeId,
    weight: f64,
    edge_id: EdgeId,
}

/// Bellman-Ford 알고리즘
pub struct BellmanFord<'a> {
    engine: &'a GraphEngine,
    config: BellmanFordConfig,
}

impl<'a> BellmanFord<'a> {
    /// 새로운 BellmanFord 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: BellmanFordConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: BellmanFordConfig) -> Self {
        Self { engine, config }
    }

    /// 최단 경로 찾기
    pub async fn find_path(&self, source: NodeId, target: NodeId) -> Result<BellmanFordResult> {
        if source == target {
            return Ok(BellmanFordResult {
                nodes: vec![source],
                edges: Vec::new(),
                total_cost: 0.0,
                has_negative_cycle: false,
                negative_cycle: None,
                source,
                target,
            });
        }

        // 모든 간선 수집
        let edges = self.collect_edges().await?;
        if edges.is_empty() {
            return Ok(BellmanFordResult::empty(source, target));
        }

        // 노드 목록 수집
        let nodes = self.collect_nodes().await?;
        let n = nodes.len();

        // 거리 및 선행자 초기화
        let mut distance: HashMap<NodeId, f64> = HashMap::new();
        let mut predecessor: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();

        for &node in &nodes {
            distance.insert(node, f64::INFINITY);
        }
        distance.insert(source, 0.0);

        // V-1번 반복하여 최단 경로 계산
        for _ in 0..n - 1 {
            let mut updated = false;
            for edge in &edges {
                if let Some(&d) = distance.get(&edge.from)
                    && d < f64::INFINITY
                {
                    let new_dist = d + edge.weight;
                    if new_dist < *distance.get(&edge.to).unwrap_or(&f64::INFINITY) {
                        distance.insert(edge.to, new_dist);
                        predecessor.insert(edge.to, (edge.from, edge.edge_id));
                        updated = true;
                    }
                }
            }
            if !updated {
                break;
            }
        }

        // 음수 사이클 검출
        for edge in &edges {
            if let Some(&d) = distance.get(&edge.from)
                && d < f64::INFINITY
            {
                let new_dist = d + edge.weight;
                if new_dist < *distance.get(&edge.to).unwrap_or(&f64::INFINITY) {
                    // 음수 사이클 발견 - 사이클 추출
                    let cycle = self.extract_negative_cycle(&edges, &predecessor, edge.to);
                    return Ok(BellmanFordResult::negative_cycle(source, target, cycle));
                }
            }
        }

        // 경로 재구성
        if distance.get(&target).copied().unwrap_or(f64::INFINITY) == f64::INFINITY {
            return Ok(BellmanFordResult::empty(source, target));
        }

        let (path_nodes, path_edges) = self.reconstruct_path(&predecessor, source, target);

        Ok(BellmanFordResult {
            nodes: path_nodes,
            edges: path_edges,
            total_cost: distance[&target],
            has_negative_cycle: false,
            negative_cycle: None,
            source,
            target,
        })
    }

    /// 음수 사이클만 탐지
    pub async fn detect_negative_cycle(&self, source: NodeId) -> Result<Option<Vec<NodeId>>> {
        let edges = self.collect_edges().await?;
        if edges.is_empty() {
            return Ok(None);
        }

        let nodes = self.collect_nodes().await?;
        let n = nodes.len();

        let mut distance: HashMap<NodeId, f64> = HashMap::new();
        let mut predecessor: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();

        for &node in &nodes {
            distance.insert(node, f64::INFINITY);
        }
        distance.insert(source, 0.0);

        // V-1번 반복
        for _ in 0..n - 1 {
            for edge in &edges {
                if let Some(&d) = distance.get(&edge.from)
                    && d < f64::INFINITY
                {
                    let new_dist = d + edge.weight;
                    if new_dist < *distance.get(&edge.to).unwrap_or(&f64::INFINITY) {
                        distance.insert(edge.to, new_dist);
                        predecessor.insert(edge.to, (edge.from, edge.edge_id));
                    }
                }
            }
        }

        // 음수 사이클 검출
        for edge in &edges {
            if let Some(&d) = distance.get(&edge.from)
                && d < f64::INFINITY
            {
                let new_dist = d + edge.weight;
                if new_dist < *distance.get(&edge.to).unwrap_or(&f64::INFINITY) {
                    let cycle = self.extract_negative_cycle(&edges, &predecessor, edge.to);
                    return Ok(Some(cycle));
                }
            }
        }

        Ok(None)
    }

    /// 모든 노드로의 최단 거리 계산
    pub async fn distances_from(&self, source: NodeId) -> Result<HashMap<NodeId, f64>> {
        let edges = self.collect_edges().await?;
        let nodes = self.collect_nodes().await?;
        let n = nodes.len();

        let mut distance: HashMap<NodeId, f64> = HashMap::new();

        for &node in &nodes {
            distance.insert(node, f64::INFINITY);
        }
        distance.insert(source, 0.0);

        for _ in 0..n - 1 {
            let mut updated = false;
            for edge in &edges {
                if let Some(&d) = distance.get(&edge.from)
                    && d < f64::INFINITY
                {
                    let new_dist = d + edge.weight;
                    if new_dist < *distance.get(&edge.to).unwrap_or(&f64::INFINITY) {
                        distance.insert(edge.to, new_dist);
                        updated = true;
                    }
                }
            }
            if !updated {
                break;
            }
        }

        // INFINITY는 도달 불가능을 의미
        Ok(distance)
    }

    /// 모든 간선 수집
    async fn collect_edges(&self) -> Result<Vec<EdgeInfo>> {
        let mut edges = Vec::new();
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

                // 가중치 추출
                let weight = edge
                    .get_property(&self.config.weight_property)
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(self.config.default_weight);

                edges.push(EdgeInfo {
                    from,
                    to,
                    weight,
                    edge_id,
                });
            }
        }

        Ok(edges)
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

    /// 경로 재구성
    fn reconstruct_path(
        &self,
        predecessor: &HashMap<NodeId, (NodeId, EdgeId)>,
        source: NodeId,
        target: NodeId,
    ) -> (Vec<NodeId>, Vec<EdgeId>) {
        let mut path_nodes = Vec::new();
        let mut path_edges = Vec::new();
        let mut current = target;

        path_nodes.push(current);

        while current != source {
            if let Some(&(prev, edge_id)) = predecessor.get(&current) {
                path_edges.push(edge_id);
                path_nodes.push(prev);
                current = prev;
            } else {
                break;
            }
        }

        path_nodes.reverse();
        path_edges.reverse();

        (path_nodes, path_edges)
    }

    /// 음수 사이클 추출
    fn extract_negative_cycle(
        &self,
        _edges: &[EdgeInfo],
        predecessor: &HashMap<NodeId, (NodeId, EdgeId)>,
        start: NodeId,
    ) -> Vec<NodeId> {
        let mut visited = HashMap::new();
        let mut current = start;

        // 사이클에 도달할 때까지 역추적
        for i in 0..predecessor.len() {
            visited.insert(current, i);
            if let Some(&(prev, _)) = predecessor.get(&current) {
                current = prev;
            } else {
                break;
            }
        }

        // 사이클 시작점 찾기
        let cycle_start = current;
        let mut cycle = vec![cycle_start];

        if let Some(&(mut prev, _)) = predecessor.get(&current) {
            while prev != cycle_start {
                cycle.push(prev);
                if let Some(&(p, _)) = predecessor.get(&prev) {
                    prev = p;
                } else {
                    break;
                }
            }
            cycle.push(cycle_start);
        }

        cycle.reverse();
        cycle
    }
}

/// 편의 함수: Bellman-Ford 최단 경로
pub async fn bellman_ford(
    engine: &GraphEngine,
    source: NodeId,
    target: NodeId,
) -> Result<BellmanFordResult> {
    BellmanFord::new(engine).find_path(source, target).await
}

/// 편의 함수: 가중치 프로퍼티 지정 Bellman-Ford
pub async fn bellman_ford_weighted(
    engine: &GraphEngine,
    source: NodeId,
    target: NodeId,
    weight_property: &str,
) -> Result<BellmanFordResult> {
    let config = BellmanFordConfig::default().with_weight_property(weight_property);
    BellmanFord::with_config(engine, config)
        .find_path(source, target)
        .await
}

/// 편의 함수: 음수 사이클 탐지
pub async fn detect_negative_cycle(
    engine: &GraphEngine,
    source: NodeId,
) -> Result<Option<Vec<NodeId>>> {
    BellmanFord::new(engine).detect_negative_cycle(source).await
}

/// 편의 함수: 가중치 프로퍼티 지정 음수 사이클 탐지
pub async fn detect_negative_cycle_weighted(
    engine: &GraphEngine,
    source: NodeId,
    weight_property: &str,
) -> Result<Option<Vec<NodeId>>> {
    let config = BellmanFordConfig::default().with_weight_property(weight_property);
    BellmanFord::with_config(engine, config)
        .detect_negative_cycle(source)
        .await
}
