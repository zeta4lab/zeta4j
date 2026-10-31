// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Clique Detection 알고리즘
//!
//! 클리크: 모든 노드 쌍이 서로 연결된 완전 부분 그래프
//!
//! # 알고리즘
//!
//! - **is_clique**: 주어진 노드 집합이 클리크인지 확인 O(k²)
//! - **find_cliques_of_size**: 특정 크기의 모든 클리크 찾기 O(n^k)
//! - **Bron-Kerbosch**: 모든 극대 클리크 찾기 O(3^(n/3))
//!
//! # 클리크 종류
//!
//! - **k-Clique**: 크기가 k인 클리크
//! - **Maximal Clique**: 더 이상 확장할 수 없는 클리크
//! - **Maximum Clique**: 가장 큰 클리크 (NP-Complete)
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::clique::{is_clique, find_maximum_clique};
//!
//! // 클리크 검증
//! let nodes = vec![1, 2, 3];
//! let is_valid = is_clique(&engine, &nodes)?;
//!
//! // 최대 클리크 찾기
//! let max_clique = find_maximum_clique(&engine)?;
//! ```

mod basic;
mod bron_kerbosch;

pub use basic::{CliqueDetector, clique_number, find_cliques_of_size, is_clique};

pub use bron_kerbosch::{BronKerbosch, find_all_maximal_cliques, find_maximum_clique};

/// Clique 알고리즘 설정
#[derive(Debug, Clone, Default)]
pub struct CliqueConfig {
    /// 최대 클리크 크기 제한
    pub max_clique_size: Option<usize>,
    /// 최대 클리크 수 제한
    pub max_cliques: Option<usize>,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl CliqueConfig {
    /// 최대 클리크 크기 설정
    pub fn with_max_clique_size(mut self, size: usize) -> Self {
        self.max_clique_size = Some(size);
        self
    }

    /// 최대 클리크 수 설정
    pub fn with_max_cliques(mut self, count: usize) -> Self {
        self.max_cliques = Some(count);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Clique 탐색 결과
#[derive(Debug, Clone)]
pub struct CliqueResult {
    /// 발견된 클리크들
    pub cliques: Vec<Vec<crate::core::NodeId>>,
    /// 최대 클리크 크기
    pub max_clique_size: usize,
    /// 클리크 수
    pub clique_count: usize,
}

impl CliqueResult {
    /// 빈 결과
    pub fn empty() -> Self {
        Self {
            cliques: Vec::new(),
            max_clique_size: 0,
            clique_count: 0,
        }
    }
}
