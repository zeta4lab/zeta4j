// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Graph Coloring 알고리즘
//!
//! 인접한 노드가 같은 색을 갖지 않도록 노드에 색을 할당하는 알고리즘들입니다.
//!
//! ## 알고리즘
//!
//! - **Greedy**: 순차적 최소 색상 할당, O(V + E)
//! - **Welsh-Powell**: 차수 기반 정렬 후 색칠, O(V log V + E)
//! - **DSatur**: 채도 기반 우선순위 색칠, O(V²)
//!
//! ## 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::coloring::{graph_coloring, chromatic_number, is_k_colorable};
//!
//! // DSatur 알고리즘으로 색칠
//! let result = graph_coloring(&engine)?;
//! println!("색상 수: {}", result.num_colors);
//!
//! // k-색칠 가능 여부
//! let can_color_with_3 = is_k_colorable(&engine, 3)?;
//! ```

mod dsatur;
mod greedy;
mod welsh_powell;

pub use dsatur::{DSatur, chromatic_number, dsatur, graph_coloring, is_k_colorable};
pub use greedy::{GreedyColoring, greedy_coloring};
pub use welsh_powell::{WelshPowell, welsh_powell};

use crate::core::NodeId;
use std::collections::HashMap;

/// 색칠 설정
#[derive(Debug, Clone, Default)]
pub struct ColoringConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl ColoringConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 색칠 결과
#[derive(Debug, Clone)]
pub struct ColoringResult {
    /// 노드별 색상 (0부터 시작)
    pub colors: HashMap<NodeId, usize>,
    /// 사용된 색상 수
    pub num_colors: usize,
    /// 색상별 노드 그룹
    pub color_classes: Vec<Vec<NodeId>>,
    /// 사용된 알고리즘
    pub algorithm: String,
}

impl ColoringResult {
    /// 빈 결과
    pub fn empty(algorithm: &str) -> Self {
        Self {
            colors: HashMap::new(),
            num_colors: 0,
            color_classes: Vec::new(),
            algorithm: algorithm.to_string(),
        }
    }

    /// 특정 노드의 색상
    pub fn color_of(&self, node: NodeId) -> Option<usize> {
        self.colors.get(&node).copied()
    }

    /// 특정 색상의 노드들
    pub fn nodes_with_color(&self, color: usize) -> Option<&Vec<NodeId>> {
        self.color_classes.get(color)
    }

    /// 올바른 색칠인지 확인 (인접 노드가 같은 색이 아닌지)
    pub fn is_valid(&self, adjacency: &HashMap<NodeId, Vec<NodeId>>) -> bool {
        for (&node, &color) in &self.colors {
            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    if let Some(&neighbor_color) = self.colors.get(&neighbor)
                        && color == neighbor_color
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 가장 많은 노드를 가진 색상 클래스
    pub fn largest_color_class(&self) -> Option<(usize, &Vec<NodeId>)> {
        self.color_classes
            .iter()
            .enumerate()
            .max_by_key(|(_, nodes)| nodes.len())
    }
}
