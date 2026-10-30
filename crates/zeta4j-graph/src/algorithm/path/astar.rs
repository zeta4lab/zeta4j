//! A* 최단 경로 알고리즘
//!
//! 휴리스틱 기반 최단 경로 알고리즘입니다.
//! 목적지가 명확할 때 Dijkstra보다 효율적입니다.
//!
//! 시간복잡도: O(E) (좋은 휴리스틱), O(V²) (최악)

use crate::core::{EdgeId, NodeId, PropertyContainer, Value};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// 휴리스틱 함수 타입
#[derive(Debug, Clone, Default)]
pub enum Heuristic {
    /// 휴리스틱 없음 (Dijkstra와 동일)
    #[default]
    Zero,
    /// 유클리드 거리 (좌표 기반)
    Euclidean {
        x_property: String,
        y_property: String,
    },
    /// 맨해튼 거리 (격자 기반)
    Manhattan {
        x_property: String,
        y_property: String,
    },
}

/// A* 설정
#[derive(Debug, Clone)]
pub struct AStarConfig {
    /// 가중치 프로퍼티 이름
    pub weight_property: String,
    /// 기본 가중치
    pub default_weight: f64,
    /// 휴리스틱 함수
    pub heuristic: Heuristic,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl Default for AStarConfig {
    fn default() -> Self {
        Self {
            weight_property: "weight".to_string(),
            default_weight: 1.0,
            heuristic: Heuristic::Zero,
            relationship_types: None,
        }
    }
}

impl AStarConfig {
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

    /// 휴리스틱 설정
    pub fn with_heuristic(mut self, heuristic: Heuristic) -> Self {
        self.heuristic = heuristic;
        self
    }

    /// 유클리드 휴리스틱 설정
    pub fn with_euclidean(mut self, x_prop: &str, y_prop: &str) -> Self {
        self.heuristic = Heuristic::Euclidean {
            x_property: x_prop.to_string(),
            y_property: y_prop.to_string(),
        };
        self
    }

    /// 맨해튼 휴리스틱 설정
    pub fn with_manhattan(mut self, x_prop: &str, y_prop: &str) -> Self {
        self.heuristic = Heuristic::Manhattan {
            x_property: x_prop.to_string(),
            y_property: y_prop.to_string(),
        };
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// A* 결과
#[derive(Debug, Clone)]
pub struct AStarResult {
    /// 경로상의 노드들
    pub nodes: Vec<NodeId>,
    /// 경로상의 엣지들
    pub edges: Vec<EdgeId>,
    /// 총 비용 (g-cost)
    pub total_cost: f64,
    /// 탐색한 노드 수
    pub nodes_explored: usize,
    /// 소스 노드
    pub source: NodeId,
    /// 타겟 노드
    pub target: NodeId,
}

impl AStarResult {
    /// 경로가 존재하는지 확인
    pub fn path_exists(&self) -> bool {
        !self.nodes.is_empty()
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
            nodes_explored: 0,
            source,
            target,
        }
    }
}

/// 우선순위 큐 상태
#[derive(Debug, Clone)]
struct State {
    /// f-cost = g-cost + h-cost
    f_cost: f64,
    /// g-cost (실제 비용)
    g_cost: f64,
    /// 현재 노드
    node: NodeId,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.f_cost == other.f_cost && self.node == other.node
    }
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        // 최소 힙을 위해 순서 반전
        other
            .f_cost
            .partial_cmp(&self.f_cost)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A* 알고리즘
pub struct AStar<'a> {
    engine: &'a GraphEngine,
    config: AStarConfig,
}

impl<'a> AStar<'a> {
    /// 새로운 AStar 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: AStarConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: AStarConfig) -> Self {
        Self { engine, config }
    }

    /// 최단 경로 찾기
    pub async fn find_path(&self, source: NodeId, target: NodeId) -> Result<AStarResult> {
        if source == target {
            return Ok(AStarResult {
                nodes: vec![source],
                edges: Vec::new(),
                total_cost: 0.0,
                nodes_explored: 1,
                source,
                target,
            });
        }

        // 타겟 노드 좌표 (휴리스틱 계산용)
        let target_coords = self.get_node_coords(target).await?;

        let mut open_set = BinaryHeap::new();
        let mut closed_set = HashSet::new();
        let mut g_scores: HashMap<NodeId, f64> = HashMap::new();
        let mut came_from: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();
        let mut nodes_explored = 0;

        // 시작 노드
        let source_coords = self.get_node_coords(source).await?;
        let h_cost = self.calculate_heuristic(&source_coords, &target_coords);

        g_scores.insert(source, 0.0);
        open_set.push(State {
            f_cost: h_cost,
            g_cost: 0.0,
            node: source,
        });

        while let Some(current) = open_set.pop() {
            let current_node = current.node;

            if current_node == target {
                // 경로 재구성
                let (path_nodes, path_edges) = self.reconstruct_path(&came_from, source, target);
                return Ok(AStarResult {
                    nodes: path_nodes,
                    edges: path_edges,
                    total_cost: current.g_cost,
                    nodes_explored,
                    source,
                    target,
                });
            }

            if closed_set.contains(&current_node) {
                continue;
            }
            closed_set.insert(current_node);
            nodes_explored += 1;

            // 이웃 노드 탐색
            let neighbors = self.get_neighbors(current_node).await?;

            for (neighbor, edge_id, weight) in neighbors {
                if closed_set.contains(&neighbor) {
                    continue;
                }

                let tentative_g = current.g_cost + weight;

                if tentative_g < *g_scores.get(&neighbor).unwrap_or(&f64::INFINITY) {
                    came_from.insert(neighbor, (current_node, edge_id));
                    g_scores.insert(neighbor, tentative_g);

                    let neighbor_coords = self.get_node_coords(neighbor).await?;
                    let h_cost = self.calculate_heuristic(&neighbor_coords, &target_coords);

                    open_set.push(State {
                        f_cost: tentative_g + h_cost,
                        g_cost: tentative_g,
                        node: neighbor,
                    });
                }
            }
        }

        // 경로 없음
        Ok(AStarResult::empty(source, target))
    }

    /// 노드 좌표 가져오기
    async fn get_node_coords(&self, node_id: NodeId) -> Result<Option<(f64, f64)>> {
        let (x_prop, y_prop) = match &self.config.heuristic {
            Heuristic::Euclidean {
                x_property,
                y_property,
            }
            | Heuristic::Manhattan {
                x_property,
                y_property,
            } => (x_property.as_str(), y_property.as_str()),
            Heuristic::Zero => return Ok(None),
        };

        if let Some(node) = self.engine.get_node(node_id).await? {
            let x = node.get_property(x_prop).and_then(|v| match v {
                Value::Integer(i) => Some(*i as f64),
                Value::Float(f) => Some(*f),
                _ => None,
            });

            let y = node.get_property(y_prop).and_then(|v| match v {
                Value::Integer(i) => Some(*i as f64),
                Value::Float(f) => Some(*f),
                _ => None,
            });

            if let (Some(x), Some(y)) = (x, y) {
                return Ok(Some((x, y)));
            }
        }

        Ok(None)
    }

    /// 휴리스틱 계산
    fn calculate_heuristic(
        &self,
        from_coords: &Option<(f64, f64)>,
        to_coords: &Option<(f64, f64)>,
    ) -> f64 {
        match (&self.config.heuristic, from_coords, to_coords) {
            (Heuristic::Euclidean { .. }, Some((x1, y1)), Some((x2, y2))) => {
                ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt()
            }
            (Heuristic::Manhattan { .. }, Some((x1, y1)), Some((x2, y2))) => {
                (x2 - x1).abs() + (y2 - y1).abs()
            }
            _ => 0.0, // Zero 휴리스틱 또는 좌표 없음
        }
    }

    /// 이웃 노드 가져오기
    async fn get_neighbors(&self, node_id: NodeId) -> Result<Vec<(NodeId, EdgeId, f64)>> {
        let mut neighbors = Vec::new();
        let edge_ids = self.engine.find_edges_by_node(node_id)?;

        for edge_id in edge_ids {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 방향 확인 (from_node가 현재 노드인 경우만)
                if edge.from_node() != node_id {
                    continue;
                }

                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let to_node = edge.to_node();
                if to_node == node_id {
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

                neighbors.push((to_node, edge_id, weight));
            }
        }

        Ok(neighbors)
    }

    /// 경로 재구성
    fn reconstruct_path(
        &self,
        came_from: &HashMap<NodeId, (NodeId, EdgeId)>,
        source: NodeId,
        target: NodeId,
    ) -> (Vec<NodeId>, Vec<EdgeId>) {
        let mut path_nodes = Vec::new();
        let mut path_edges = Vec::new();
        let mut current = target;

        path_nodes.push(current);

        while current != source {
            if let Some(&(prev, edge_id)) = came_from.get(&current) {
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
}

/// 편의 함수: A* 최단 경로 (휴리스틱 없음)
pub async fn astar(engine: &GraphEngine, source: NodeId, target: NodeId) -> Result<AStarResult> {
    AStar::new(engine).find_path(source, target).await
}

/// 편의 함수: 가중치 프로퍼티 지정 A*
pub async fn astar_weighted(
    engine: &GraphEngine,
    source: NodeId,
    target: NodeId,
    weight_property: &str,
) -> Result<AStarResult> {
    let config = AStarConfig::default().with_weight_property(weight_property);
    AStar::with_config(engine, config)
        .find_path(source, target)
        .await
}

/// 편의 함수: 유클리드 휴리스틱 A*
pub async fn astar_euclidean(
    engine: &GraphEngine,
    source: NodeId,
    target: NodeId,
    weight_property: &str,
    x_property: &str,
    y_property: &str,
) -> Result<AStarResult> {
    let config = AStarConfig::default()
        .with_weight_property(weight_property)
        .with_euclidean(x_property, y_property);
    AStar::with_config(engine, config)
        .find_path(source, target)
        .await
}

/// 편의 함수: 맨해튼 휴리스틱 A*
pub async fn astar_manhattan(
    engine: &GraphEngine,
    source: NodeId,
    target: NodeId,
    weight_property: &str,
    x_property: &str,
    y_property: &str,
) -> Result<AStarResult> {
    let config = AStarConfig::default()
        .with_weight_property(weight_property)
        .with_manhattan(x_property, y_property);
    AStar::with_config(engine, config)
        .find_path(source, target)
        .await
}
