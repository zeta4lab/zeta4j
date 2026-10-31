// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 커뮤니티 탐지 알고리즘
//!
//! - Connected Components (WCC): 무방향 그래프의 연결 컴포넌트
//! - Strongly Connected Components (SCC): 방향 그래프의 강연결 컴포넌트
//! - Louvain Algorithm: 모듈러리티 기반 커뮤니티 탐지
//! - Leiden Algorithm: Louvain 개선 버전, 연결성 보장 (MS GraphRAG)
//! - Modularity Optimization: 단순 모듈러리티 최적화
//! - Label Propagation: 빠른 레이블 전파 기반 커뮤니티 탐지

mod connected;
mod label_propagation;
mod leiden;
mod louvain;
mod modularity;
mod tarjan;

pub use connected::{ConnectedComponents, ConnectedComponentsConfig, ConnectedComponentsResult};

pub use tarjan::{StronglyConnectedComponents, StronglyConnectedConfig, StronglyConnectedResult};

pub use louvain::{Louvain, LouvainConfig, LouvainResult};

pub use modularity::{ModularityConfig, ModularityOptimization, ModularityResult};

pub use label_propagation::{
    LabelPropagation, LabelPropagationAnalyzer, LabelPropagationConfig, LabelPropagationResult,
    LabelPropagationStatistics, label_propagation, label_propagation_with_iterations,
};

pub use leiden::{Leiden, LeidenConfig, LeidenResult};
