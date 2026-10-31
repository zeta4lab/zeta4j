// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 그래프 구조 분석 알고리즘
//!
//! 그래프의 구조적 취약점과 핵심 구조를 분석합니다.
//!
//! - Articulation Points: 단절점 (제거 시 그래프 분리)
//! - Bridges: 단절선 (제거 시 그래프 분리)
//! - Biconnected Components: 이중 연결 성분
//! - GraphStructure: 통합 구조 분석

mod articulation;
mod biconnected;
mod bridge;
mod graph_structure;

pub use articulation::{
    ArticulationConfig, ArticulationPoints, ArticulationResult, find_articulation_points,
    is_articulation_point,
};
pub use biconnected::{
    BiconnectedComponents, BiconnectedConfig, BiconnectedResult, BlockCutNodeType,
    BlockCutTreeResult, build_block_cut_tree, find_biconnected_components,
};
pub use bridge::{BridgeConfig, BridgeResult, Bridges, find_bridges, is_bridge};
pub use graph_structure::{
    GraphStructure, GraphStructureConfig, GraphStructureResult, analyze_structure,
    analyze_structure_full, is_structurally_robust,
};
