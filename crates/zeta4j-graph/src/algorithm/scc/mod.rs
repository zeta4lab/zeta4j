// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Strongly Connected Components (SCC) 알고리즘
//!
//! 방향 그래프에서 강연결요소를 찾는 알고리즘들을 제공합니다.
//!
//! ## 알고리즘
//!
//! - **Tarjan**: 단일 DFS, O(V + E)
//! - **Kosaraju**: 두 번의 DFS, O(V + E)
//!
//! ## 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::scc::{tarjan_scc, kosaraju_scc, is_strongly_connected};
//!
//! // Tarjan 알고리즘으로 SCC 찾기
//! let result = tarjan_scc(&engine)?;
//! println!("SCC 개수: {}", result.count);
//!
//! // 강연결 여부 확인
//! let is_sc = is_strongly_connected(&engine)?;
//! ```

mod condensation;
mod kosaraju;
mod tarjan;

pub use condensation::{
    CondensationBuilder, CondensationGraph, condensation_graph, is_strongly_connected, largest_scc,
    scc_count,
};
pub use kosaraju::{KosarajuConfig, KosarajuSCC, kosaraju_scc};
pub use tarjan::{TarjanConfig, TarjanSCC, tarjan_scc};

use crate::core::NodeId;
use std::collections::HashMap;

/// SCC 설정 (공통)
#[derive(Debug, Clone, Default)]
pub struct SCCConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl SCCConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// SCC 결과
#[derive(Debug, Clone)]
pub struct SCCResult {
    /// 각 SCC의 노드 목록
    pub components: Vec<Vec<NodeId>>,
    /// 노드 → SCC 인덱스 맵
    pub component_map: HashMap<NodeId, usize>,
    /// SCC 개수
    pub count: usize,
    /// 전체 그래프가 강연결인지
    pub is_strongly_connected: bool,
    /// 전체 노드 수
    pub node_count: usize,
    /// 사용된 알고리즘
    pub algorithm: String,
}

impl SCCResult {
    /// 빈 결과
    pub fn empty() -> Self {
        Self {
            components: Vec::new(),
            component_map: HashMap::new(),
            count: 0,
            is_strongly_connected: false,
            node_count: 0,
            algorithm: String::new(),
        }
    }

    /// 특정 노드가 속한 SCC 인덱스
    pub fn scc_of(&self, node: NodeId) -> Option<usize> {
        self.component_map.get(&node).copied()
    }

    /// 특정 노드가 속한 SCC의 노드들
    pub fn scc_nodes_of(&self, node: NodeId) -> Option<&Vec<NodeId>> {
        self.scc_of(node).and_then(|idx| self.components.get(idx))
    }

    /// 가장 큰 SCC
    pub fn largest_scc(&self) -> Option<&Vec<NodeId>> {
        self.components.iter().max_by_key(|c| c.len())
    }

    /// 가장 큰 SCC의 크기
    pub fn largest_scc_size(&self) -> usize {
        self.largest_scc().map(|c| c.len()).unwrap_or(0)
    }

    /// 두 노드가 같은 SCC에 있는지
    pub fn same_scc(&self, node1: NodeId, node2: NodeId) -> bool {
        match (self.scc_of(node1), self.scc_of(node2)) {
            (Some(scc1), Some(scc2)) => scc1 == scc2,
            _ => false,
        }
    }
}
