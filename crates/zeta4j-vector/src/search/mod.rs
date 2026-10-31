// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 벡터 검색 모듈
//!
//! Vector, Fulltext, Graph 후보의 hybrid 병합을 제공합니다.
//!
//! # 개요
//!
//! ## Hybrid Search
//! - `HybridMerger`: 벡터 + 그래프 결과 병합
//! - `HybridQueryConfig`: 하이브리드 검색 설정
//! - `HybridResult`: 통합 검색 결과
//!

mod hybrid;

pub use hybrid::{
    FulltextCandidate, GraphCandidate, HybridMerger, HybridQueryConfig, HybridQueryStrategy,
    HybridResult, MergeMode, VectorCandidate,
};
