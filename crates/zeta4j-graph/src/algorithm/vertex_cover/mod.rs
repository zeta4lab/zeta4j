// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Vertex Cover 알고리즘
//!
//! 정점 커버: 그래프의 모든 엣지가 적어도 하나의 정점에 인접하도록 하는 정점 집합
//!
//! # 알고리즘
//!
//! - **is_vertex_cover**: 주어진 노드 집합이 정점 커버인지 확인 O(E)
//! - **greedy_vertex_cover**: Greedy 2-근사 알고리즘 O(E)
//! - **minimum_vertex_cover**: Branch and Bound 최적해 O(2^k * n)
//!
//! # Vertex Cover 종류
//!
//! - **Vertex Cover**: 모든 엣지를 커버하는 정점 집합
//! - **Minimum Vertex Cover**: 가장 작은 정점 커버 (NP-Complete)
//! - **k-Vertex Cover**: 크기 k 이하의 정점 커버
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::vertex_cover::{is_vertex_cover, minimum_vertex_cover};
//!
//! // 정점 커버 검증
//! let nodes = vec![1, 2];
//! let is_valid = is_vertex_cover(&engine, &nodes)?;
//!
//! // 최소 정점 커버 찾기
//! let min_cover = minimum_vertex_cover(&engine)?;
//! ```

mod basic;
mod branch_bound;

pub use basic::{
    VertexCoverDetector, greedy_max_degree_cover, greedy_vertex_cover, is_vertex_cover,
    vertex_cover_number,
};

pub use branch_bound::{BranchAndBound, has_vertex_cover_of_size, minimum_vertex_cover};

/// Vertex Cover 알고리즘 설정
#[derive(Debug, Clone, Default)]
pub struct VertexCoverConfig {
    /// 최대 커버 크기 제한
    pub max_size: Option<usize>,
    /// 타임아웃 (밀리초)
    pub timeout_ms: Option<u64>,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl VertexCoverConfig {
    /// 최대 커버 크기 설정
    pub fn with_max_size(mut self, size: usize) -> Self {
        self.max_size = Some(size);
        self
    }

    /// 타임아웃 설정
    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Vertex Cover 결과
#[derive(Debug, Clone)]
pub struct VertexCoverResult {
    /// 정점 커버
    pub cover: Vec<crate::core::NodeId>,
    /// 커버 크기
    pub size: usize,
    /// 최소 여부 (근사 알고리즘인 경우 false)
    pub is_minimum: bool,
    /// 커버되지 않은 엣지 수 (검증용)
    pub uncovered_edges: usize,
}

impl VertexCoverResult {
    /// 빈 결과
    pub fn empty() -> Self {
        Self {
            cover: Vec::new(),
            size: 0,
            is_minimum: true,
            uncovered_edges: 0,
        }
    }
}
