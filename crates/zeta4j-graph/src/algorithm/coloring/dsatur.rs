//! DSatur (Saturation Degree) Graph Coloring Algorithm
//!
//! 채도(saturation degree)가 가장 높은 노드를 우선 색칠합니다.
//! 채도: 인접 노드에 할당된 서로 다른 색상의 수
//!
//! 시간복잡도: O(V² log V) - 우선순위 큐 사용 시 O(V² + E log V)

use super::{ColoringConfig, ColoringResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// 채도 정보를 담은 노드
#[derive(Clone, Eq, PartialEq)]
struct SaturationNode {
    node: NodeId,
    saturation: usize, // 인접한 서로 다른 색상 수
    degree: usize,     // 차수 (동점 시 사용)
}

impl Ord for SaturationNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // 채도 내림차순, 동점 시 차수 내림차순, 동점 시 노드 ID 오름차순
        self.saturation
            .cmp(&other.saturation)
            .then(self.degree.cmp(&other.degree))
            .then(other.node.cmp(&self.node))
    }
}

impl PartialOrd for SaturationNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// DSatur 알고리즘
pub struct DSatur<'a> {
    engine: &'a GraphEngine,
    config: ColoringConfig,
}

impl<'a> DSatur<'a> {
    /// 새로운 DSatur 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ColoringConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: ColoringConfig) -> Self {
        Self { engine, config }
    }

    /// 그래프 색칠 실행
    pub async fn color(&self) -> Result<ColoringResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(ColoringResult::empty("DSatur"));
        }

        let mut colors: HashMap<NodeId, usize> = HashMap::new();
        let mut adjacent_colors: HashMap<NodeId, HashSet<usize>> = HashMap::new();
        let mut num_colors = 0usize;

        // 초기화
        for &node in adjacency.keys() {
            adjacent_colors.insert(node, HashSet::new());
        }

        // 우선순위 큐 초기화 (채도=0, 차수로 정렬)
        let mut heap: BinaryHeap<SaturationNode> = adjacency
            .iter()
            .map(|(&node, neighbors)| SaturationNode {
                node,
                saturation: 0,
                degree: neighbors.len(),
            })
            .collect();

        let mut colored: HashSet<NodeId> = HashSet::new();

        while let Some(sat_node) = heap.pop() {
            if colored.contains(&sat_node.node) {
                continue;
            }

            let node = sat_node.node;

            // 인접 노드들의 색상
            let empty_set = HashSet::new();
            let forbidden = adjacent_colors.get(&node).unwrap_or(&empty_set);

            // 가능한 최소 색상 찾기
            let mut color = 0;
            while forbidden.contains(&color) {
                color += 1;
            }

            colors.insert(node, color);
            colored.insert(node);
            num_colors = num_colors.max(color + 1);

            // 인접 노드들의 채도 업데이트
            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    if !colored.contains(&neighbor)
                        && let Some(adj_colors) = adjacent_colors.get_mut(&neighbor)
                    {
                        adj_colors.insert(color);

                        // 업데이트된 채도로 힙에 다시 추가
                        heap.push(SaturationNode {
                            node: neighbor,
                            saturation: adj_colors.len(),
                            degree: adjacency.get(&neighbor).map(|n| n.len()).unwrap_or(0),
                        });
                    }
                }
            }
        }

        // 색상 클래스 구축
        let color_classes = Self::build_color_classes(&colors, num_colors);

        Ok(ColoringResult {
            colors,
            num_colors,
            color_classes,
            algorithm: "DSatur".to_string(),
        })
    }

    /// 색상 클래스 구축
    fn build_color_classes(colors: &HashMap<NodeId, usize>, num_colors: usize) -> Vec<Vec<NodeId>> {
        let mut classes: Vec<Vec<NodeId>> = vec![Vec::new(); num_colors];

        for (&node, &color) in colors {
            if color < num_colors {
                classes[color].push(node);
            }
        }

        for class in &mut classes {
            class.sort();
        }

        classes
    }

    /// 인접 리스트 구축 (무방향)
    async fn build_adjacency(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, Vec::new());
            }
        }

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
                    adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok(adjacency)
    }
}

/// 편의 함수: DSatur Coloring
pub async fn dsatur(engine: &GraphEngine) -> Result<ColoringResult> {
    DSatur::new(engine).color().await
}

/// 편의 함수: 기본 그래프 색칠 (DSatur 사용)
pub async fn graph_coloring(engine: &GraphEngine) -> Result<ColoringResult> {
    dsatur(engine).await
}

/// 편의 함수: 색상 수 (근사값)
pub async fn chromatic_number(engine: &GraphEngine) -> Result<usize> {
    let result = dsatur(engine).await?;
    Ok(result.num_colors)
}

/// 편의 함수: k-색칠 가능 여부
pub async fn is_k_colorable(engine: &GraphEngine, k: usize) -> Result<bool> {
    let result = dsatur(engine).await?;
    Ok(result.num_colors <= k)
}
