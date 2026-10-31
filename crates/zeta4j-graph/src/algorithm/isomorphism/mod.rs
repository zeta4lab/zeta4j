// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Graph Isomorphism 알고리즘
//!
//! 두 그래프가 구조적으로 동일한지 판별하는 알고리즘을 제공합니다.
//!
//! ## Graph Isomorphism이란?
//!
//! - **Graph Isomorphism**: G1과 G2가 동형 ⟺ 전단사 함수 f가 존재하여 간선 관계 보존
//! - **Subgraph Isomorphism**: 패턴이 대상 그래프의 부분그래프와 동형 (NP-Complete)
//!
//! ## 알고리즘
//!
//! - **VF2**: 상태 공간 탐색 기반 정확한 동형 검사
//! - **Weisfeiler-Lehman**: 색상 정제 기반 빠른 해시 (근사)
//!
//! ## 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::isomorphism::*;
//!
//! // 두 그래프가 동형인지 확인
//! let result = is_isomorphic(&g1, &g2)?;
//!
//! // 동형 매핑 찾기
//! if let Some(mapping) = find_isomorphism(&g1, &g2)? {
//!     println!("Isomorphic with mapping: {:?}", mapping);
//! }
//! ```

mod subgraph;
mod vf2;
mod weisfeiler_lehman;

pub use subgraph::{SubgraphMatcher, find_all_subgraphs, find_subgraph, is_subgraph_isomorphic};
pub use vf2::{VF2, find_all_isomorphisms, find_isomorphism, is_isomorphic};
pub use weisfeiler_lehman::{
    WLConfig, WeisfeilerLehman, are_potentially_isomorphic, wl_hash, wl_hash_with_iterations,
};

use crate::core::NodeId;
use std::collections::HashMap;

/// Isomorphism 설정
#[derive(Debug, Clone, Default)]
pub struct IsomorphismConfig {
    /// 최대 매핑 수 제한
    pub max_mappings: Option<usize>,
    /// 타임아웃 (밀리초)
    pub timeout_ms: Option<u64>,
    /// 노드 레이블 매칭 여부
    pub match_node_labels: bool,
    /// 엣지 타입 매칭 여부
    pub match_edge_types: bool,
    /// 특정 관계 타입만 고려
    pub relationship_types: Option<Vec<String>>,
}

impl IsomorphismConfig {
    /// 최대 매핑 수 설정
    pub fn with_max_mappings(mut self, count: usize) -> Self {
        self.max_mappings = Some(count);
        self
    }

    /// 타임아웃 설정
    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// 노드 레이블 매칭 설정
    pub fn with_node_label_matching(mut self, enabled: bool) -> Self {
        self.match_node_labels = enabled;
        self
    }

    /// 엣지 타입 매칭 설정
    pub fn with_edge_type_matching(mut self, enabled: bool) -> Self {
        self.match_edge_types = enabled;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Isomorphism 결과
#[derive(Debug, Clone)]
pub struct IsomorphismResult {
    /// 동형 여부
    pub is_isomorphic: bool,
    /// 매핑 (g1 노드 -> g2 노드)
    pub mapping: Option<HashMap<NodeId, NodeId>>,
    /// 모든 매핑 (find_all인 경우)
    pub all_mappings: Vec<HashMap<NodeId, NodeId>>,
}

impl IsomorphismResult {
    /// 동형이 아닌 결과 생성
    pub fn not_isomorphic() -> Self {
        Self {
            is_isomorphic: false,
            mapping: None,
            all_mappings: Vec::new(),
        }
    }

    /// 동형인 결과 생성 (매핑 포함)
    pub fn isomorphic(mapping: HashMap<NodeId, NodeId>) -> Self {
        Self {
            is_isomorphic: true,
            mapping: Some(mapping.clone()),
            all_mappings: vec![mapping],
        }
    }

    /// 모든 매핑이 포함된 결과 생성
    pub fn with_all_mappings(mappings: Vec<HashMap<NodeId, NodeId>>) -> Self {
        Self {
            is_isomorphic: !mappings.is_empty(),
            mapping: mappings.first().cloned(),
            all_mappings: mappings,
        }
    }
}

/// Subgraph Isomorphism 결과
#[derive(Debug, Clone)]
pub struct SubgraphResult {
    /// 서브그래프 동형 여부
    pub found: bool,
    /// 매핑 (패턴 노드 -> 타겟 노드)
    pub mapping: Option<HashMap<NodeId, NodeId>>,
    /// 모든 매핑
    pub all_mappings: Vec<HashMap<NodeId, NodeId>>,
    /// 발견된 매핑 수
    pub count: usize,
}

impl SubgraphResult {
    /// 서브그래프 없음
    pub fn not_found() -> Self {
        Self {
            found: false,
            mapping: None,
            all_mappings: Vec::new(),
            count: 0,
        }
    }

    /// 서브그래프 발견
    pub fn found(mapping: HashMap<NodeId, NodeId>) -> Self {
        Self {
            found: true,
            mapping: Some(mapping.clone()),
            all_mappings: vec![mapping],
            count: 1,
        }
    }

    /// 모든 서브그래프 발견
    pub fn with_all(mappings: Vec<HashMap<NodeId, NodeId>>) -> Self {
        let count = mappings.len();
        Self {
            found: !mappings.is_empty(),
            mapping: mappings.first().cloned(),
            all_mappings: mappings,
            count,
        }
    }
}
