// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Feedback Set 알고리즘
//!
//! 그래프에서 사이클을 제거하기 위한 Feedback Vertex Set과 Feedback Arc Set을 찾습니다.
//!
//! ## Feedback Set이란?
//!
//! - **Feedback Vertex Set (FVS)**: 삭제하면 그래프가 비순환(DAG)이 되는 정점 집합
//! - **Feedback Arc Set (FAS)**: 삭제하면 그래프가 비순환이 되는 엣지 집합
//!
//! ## 알고리즘
//!
//! - **Greedy FVS**: 사이클에 가장 많이 참여하는 정점 반복 제거
//! - **Greedy FAS**: DFS back edge 수집
//! - **Branch and Bound**: 정확한 최소 FVS/FAS
//!
//! ## 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::feedback::*;
//!
//! // 단순 사이클에서 하나의 정점만 제거하면 DAG
//! let fvs = minimum_fvs(&engine)?;
//! assert_eq!(fvs.len(), 1);
//!
//! // FVS 검증
//! assert!(is_feedback_vertex_set(&engine, &fvs)?);
//! ```

mod basic;
mod minimum;

pub use basic::{
    FeedbackSetDetector, greedy_fas, greedy_fvs, is_feedback_arc_set, is_feedback_vertex_set,
};
pub use minimum::{MinimumFeedbackSet, has_fas_of_size, has_fvs_of_size, minimum_fas, minimum_fvs};

use crate::core::NodeId;

/// Feedback Set 설정
#[derive(Debug, Clone, Default)]
pub struct FeedbackSetConfig {
    /// 최대 집합 크기 제한
    pub max_size: Option<usize>,
    /// 타임아웃 (밀리초)
    pub timeout_ms: Option<u64>,
    /// 특정 관계 타입만 고려
    pub relationship_types: Option<Vec<String>>,
}

impl FeedbackSetConfig {
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

/// Feedback Vertex Set 결과
#[derive(Debug, Clone)]
pub struct FeedbackVertexSetResult {
    /// FVS의 정점들
    pub set: Vec<NodeId>,
    /// FVS 크기
    pub size: usize,
    /// 최소 FVS인지 여부
    pub is_minimum: bool,
}

impl FeedbackVertexSetResult {
    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            set: Vec::new(),
            size: 0,
            is_minimum: true,
        }
    }
}

/// Feedback Arc Set 결과
#[derive(Debug, Clone)]
pub struct FeedbackArcSetResult {
    /// FAS의 엣지들
    pub edges: Vec<(NodeId, NodeId)>,
    /// FAS 크기
    pub size: usize,
    /// 최소 FAS인지 여부
    pub is_minimum: bool,
}

impl FeedbackArcSetResult {
    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            edges: Vec::new(),
            size: 0,
            is_minimum: true,
        }
    }
}
