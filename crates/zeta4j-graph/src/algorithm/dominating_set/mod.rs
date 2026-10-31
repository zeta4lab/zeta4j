// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Dominating Set 알고리즘
//!
//! 그래프에서 지배 집합을 찾는 알고리즘들을 제공합니다.
//!
//! ## 지배 집합이란?
//!
//! 그래프 G=(V,E)에서 정점 부분집합 S ⊆ V가 지배 집합이려면:
//! - 모든 정점 v ∈ V에 대해: v ∈ S 이거나 v의 이웃 중 하나가 S에 속함
//!
//! ## 알고리즘
//!
//! - **Greedy**: O(V²) 근사 알고리즘, 근사비 O(ln Δ)
//! - **Branch and Bound**: 정확한 최소 지배 집합, 지수 시간
//! - **Connected Dominating Set**: 연결된 지배 집합
//!
//! ## 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::dominating_set::*;
//!
//! // 별 그래프에서 중심 노드 하나가 지배 집합
//! let ds = minimum_dominating_set(&engine)?;
//! assert_eq!(ds.len(), 1);
//!
//! // 지배 집합 검증
//! assert!(is_dominating_set(&engine, &[center_node])?);
//! ```

mod basic;
mod minimum;

pub use basic::{
    DominatingSetDetector, domination_number, greedy_dominating_set, is_dominating_set,
};
pub use minimum::{
    MinimumDominatingSet, connected_dominating_set, has_dominating_set_of_size,
    minimum_dominating_set,
};

use crate::core::NodeId;

/// Dominating Set 설정
#[derive(Debug, Clone, Default)]
pub struct DominatingSetConfig {
    /// 최대 집합 크기 제한
    pub max_size: Option<usize>,
    /// 타임아웃 (밀리초)
    pub timeout_ms: Option<u64>,
    /// 특정 관계 타입만 고려
    pub relationship_types: Option<Vec<String>>,
}

impl DominatingSetConfig {
    /// 최대 크기 설정
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

/// Dominating Set 결과
#[derive(Debug, Clone)]
pub struct DominatingSetResult {
    /// 지배 집합의 노드들
    pub set: Vec<NodeId>,
    /// 지배 집합 크기
    pub size: usize,
    /// 최소 지배 집합인지 여부
    pub is_minimum: bool,
}

impl DominatingSetResult {
    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            set: Vec::new(),
            size: 0,
            is_minimum: true,
        }
    }
}
