// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Independent Set 알고리즘
//!
//! 독립 집합: 서로 인접하지 않은 정점들의 집합
//!
//! # 알고리즘
//!
//! - **is_independent_set**: 주어진 노드 집합이 독립 집합인지 확인 O(k²)
//! - **greedy_independent_set**: Greedy 근사 알고리즘 O(V + E)
//! - **maximum_independent_set**: Vertex Cover 활용 최적해 O(2^k * n)
//!
//! # Independent Set 종류
//!
//! - **Independent Set**: 서로 인접하지 않은 정점 집합
//! - **Maximal Independent Set**: 더 이상 확장 불가능한 독립 집합
//! - **Maximum Independent Set**: 가장 큰 독립 집합 (NP-Complete)
//!
//! # 관련 문제
//!
//! - V - Independent Set = Vertex Cover
//! - 보완 그래프에서 Clique = Independent Set
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::independent_set::{is_independent_set, maximum_independent_set};
//!
//! // 독립 집합 검증
//! let nodes = vec![1, 3, 5];
//! let is_valid = is_independent_set(&engine, &nodes)?;
//!
//! // 최대 독립 집합 찾기
//! let max_set = maximum_independent_set(&engine)?;
//! ```

mod basic;
mod maximum;

pub use basic::{
    IndependentSetDetector, greedy_independent_set, independence_number, is_independent_set,
};

pub use maximum::{
    MaximumIndependentSet, find_all_maximal_independent_sets, has_independent_set_of_size,
    maximum_independent_set,
};

/// Independent Set 알고리즘 설정
#[derive(Debug, Clone, Default)]
pub struct IndependentSetConfig {
    /// 최대 집합 크기 제한
    pub max_size: Option<usize>,
    /// 타임아웃 (밀리초)
    pub timeout_ms: Option<u64>,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl IndependentSetConfig {
    /// 최대 집합 크기 설정
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

/// Independent Set 결과
#[derive(Debug, Clone)]
pub struct IndependentSetResult {
    /// 독립 집합
    pub set: Vec<crate::core::NodeId>,
    /// 집합 크기
    pub size: usize,
    /// 최대 여부 (근사 알고리즘인 경우 false)
    pub is_maximum: bool,
}

impl IndependentSetResult {
    /// 빈 결과
    pub fn empty() -> Self {
        Self {
            set: Vec::new(),
            size: 0,
            is_maximum: true,
        }
    }
}
