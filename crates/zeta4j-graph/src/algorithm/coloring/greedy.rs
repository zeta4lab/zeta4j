//! Greedy Graph Coloring Algorithm
//!
//! 순차적으로 노드를 방문하며 가능한 최소 색상을 할당합니다.
//! 시간복잡도: O(V + E)

use super::{ColoringConfig, ColoringResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// Greedy Coloring 알고리즘
pub struct GreedyColoring<'a> {
    engine: &'a GraphEngine,
    config: ColoringConfig,
}

impl<'a> GreedyColoring<'a> {
    /// 새로운 GreedyColoring 생성
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
            return Ok(ColoringResult::empty("Greedy"));
        }

        let mut colors: HashMap<NodeId, usize> = HashMap::new();
        let mut num_colors = 0usize;

        // 노드 ID 순서대로 색칠
        let mut nodes: Vec<NodeId> = adjacency.keys().copied().collect();
        nodes.sort();

        for node in nodes {
            // 인접 노드들의 색상 수집
            let neighbor_colors: HashSet<usize> = adjacency
                .get(&node)
                .map(|neighbors| {
                    neighbors
                        .iter()
                        .filter_map(|n| colors.get(n).copied())
                        .collect()
                })
                .unwrap_or_default();

            // 가능한 최소 색상 찾기
            let mut color = 0;
            while neighbor_colors.contains(&color) {
                color += 1;
            }

            colors.insert(node, color);
            num_colors = num_colors.max(color + 1);
        }

        // 색상 클래스 구축
        let color_classes = Self::build_color_classes(&colors, num_colors);

        Ok(ColoringResult {
            colors,
            num_colors,
            color_classes,
            algorithm: "Greedy".to_string(),
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

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, Vec::new());
            }
        }

        // 엣지 수집 (무방향으로 처리)
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

/// 편의 함수: Greedy Coloring
pub async fn greedy_coloring(engine: &GraphEngine) -> Result<ColoringResult> {
    GreedyColoring::new(engine).color().await
}
