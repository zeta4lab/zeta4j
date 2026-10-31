// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Eulerian Path/Circuit 알고리즘
//!
//! 모든 엣지를 정확히 한 번씩 방문하는 경로/회로를 찾습니다.
//!
//! # 알고리즘
//!
//! - **Eulerian Circuit**: 모든 엣지를 한 번씩 방문하고 시작점으로 돌아옴
//! - **Eulerian Path**: 모든 엣지를 한 번씩 방문 (시작점 != 끝점 가능)
//!
//! # 조건
//!
//! ## 무방향 그래프
//! - 오일러 회로: 모든 정점의 차수가 짝수
//! - 오일러 경로: 홀수 차수 정점이 0개 또는 2개
//!
//! ## 방향 그래프
//! - 오일러 회로: 모든 정점의 in-degree = out-degree
//! - 오일러 경로: 최대 1개 정점이 out-in=1, 최대 1개가 in-out=1
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::eulerian::{eulerian_path, has_eulerian_path};
//!
//! // 오일러 경로 존재 여부 확인
//! let has_path = has_eulerian_path(&engine)?;
//!
//! // 오일러 경로 찾기
//! if let Some(result) = eulerian_path(&engine)? {
//!     println!("Path: {:?}", result.path);
//! }
//! ```

mod checker;
mod hierholzer;

pub use checker::{
    EulerianCheckResult, EulerianChecker, eulerian_check, has_eulerian_circuit,
    has_eulerian_circuit_directed, has_eulerian_path, has_eulerian_path_directed,
};

pub use hierholzer::{
    EulerianResult, Hierholzer, eulerian_circuit, eulerian_circuit_directed, eulerian_path,
    eulerian_path_directed,
};

/// Eulerian 알고리즘 설정
#[derive(Debug, Clone, Default)]
pub struct EulerianConfig {
    /// 방향 그래프 여부
    pub directed: bool,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl EulerianConfig {
    /// 방향 그래프로 설정
    pub fn directed(mut self) -> Self {
        self.directed = true;
        self
    }

    /// 무방향 그래프로 설정
    pub fn undirected(mut self) -> Self {
        self.directed = false;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}
