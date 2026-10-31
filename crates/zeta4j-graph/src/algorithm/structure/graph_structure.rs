// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GraphStructure - 통합 그래프 구조 분석기
//!
//! 단절점, 단절선, 이중 연결 성분을 한 번에 분석합니다.

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashSet;

use super::{
    ArticulationConfig, ArticulationPoints, ArticulationResult, BiconnectedComponents,
    BiconnectedConfig, BiconnectedResult, BlockCutTreeResult, BridgeConfig, BridgeResult, Bridges,
};

/// 통합 구조 분석 설정
#[derive(Debug, Clone)]
pub struct GraphStructureConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 단절점 분석 포함
    pub include_articulation_points: bool,
    /// 단절선 분석 포함
    pub include_bridges: bool,
    /// 이중 연결 성분 분석 포함
    pub include_biconnected: bool,
    /// Block-Cut Tree 생성 포함
    pub include_block_cut_tree: bool,
}

impl Default for GraphStructureConfig {
    fn default() -> Self {
        Self {
            relationship_types: None,
            include_articulation_points: true,
            include_bridges: true,
            include_biconnected: true,
            include_block_cut_tree: false,
        }
    }
}

impl GraphStructureConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 단절점 분석 포함 여부
    pub fn with_articulation_points(mut self, include: bool) -> Self {
        self.include_articulation_points = include;
        self
    }

    /// 단절선 분석 포함 여부
    pub fn with_bridges(mut self, include: bool) -> Self {
        self.include_bridges = include;
        self
    }

    /// 이중 연결 성분 분석 포함 여부
    pub fn with_biconnected(mut self, include: bool) -> Self {
        self.include_biconnected = include;
        self
    }

    /// Block-Cut Tree 생성 포함 여부
    pub fn with_block_cut_tree(mut self, include: bool) -> Self {
        self.include_block_cut_tree = include;
        self
    }

    /// 모든 분석 포함
    pub fn full() -> Self {
        Self {
            relationship_types: None,
            include_articulation_points: true,
            include_bridges: true,
            include_biconnected: true,
            include_block_cut_tree: true,
        }
    }

    /// 최소 분석 (단절점만)
    pub fn minimal() -> Self {
        Self {
            relationship_types: None,
            include_articulation_points: true,
            include_bridges: false,
            include_biconnected: false,
            include_block_cut_tree: false,
        }
    }
}

/// 통합 구조 분석 결과
#[derive(Debug, Clone)]
pub struct GraphStructureResult {
    /// 단절점 결과 (요청된 경우)
    pub articulation_points: Option<ArticulationResult>,
    /// 단절선 결과 (요청된 경우)
    pub bridges: Option<BridgeResult>,
    /// 이중 연결 성분 결과 (요청된 경우)
    pub biconnected: Option<BiconnectedResult>,
    /// Block-Cut Tree 결과 (요청된 경우)
    pub block_cut_tree: Option<BlockCutTreeResult>,
    /// 그래프가 이중 연결인지 (단절점이 없는지)
    pub is_biconnected: bool,
    /// 그래프가 다리 없는지 (단절선이 없는지)
    pub is_bridge_free: bool,
}

impl GraphStructureResult {
    /// 그래프가 구조적으로 견고한지 (단절점과 단절선이 모두 없는지)
    pub fn is_structurally_robust(&self) -> bool {
        self.is_biconnected && self.is_bridge_free
    }

    /// 모든 단절점 노드 ID 반환
    pub fn get_critical_nodes(&self) -> Vec<NodeId> {
        self.articulation_points
            .as_ref()
            .map(|ap| ap.points.clone())
            .unwrap_or_default()
    }

    /// 모든 단절선 반환
    pub fn get_critical_edges(&self) -> Vec<(NodeId, NodeId)> {
        self.bridges
            .as_ref()
            .map(|br| br.bridges.clone())
            .unwrap_or_default()
    }

    /// 이중 연결 성분 개수
    pub fn biconnected_count(&self) -> usize {
        self.biconnected.as_ref().map(|bc| bc.count).unwrap_or(0)
    }
}

/// 통합 그래프 구조 분석기
pub struct GraphStructure<'a> {
    engine: &'a GraphEngine,
    config: GraphStructureConfig,
}

impl<'a> GraphStructure<'a> {
    /// 새로운 GraphStructure 분석기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: GraphStructureConfig::default(),
        }
    }

    /// 설정과 함께 분석기 생성
    pub fn with_config(engine: &'a GraphEngine, config: GraphStructureConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 구조 분석 수행
    pub async fn analyze(&self) -> Result<GraphStructureResult> {
        let mut result = GraphStructureResult {
            articulation_points: None,
            bridges: None,
            biconnected: None,
            block_cut_tree: None,
            is_biconnected: true,
            is_bridge_free: true,
        };

        // 단절점 분석
        if self.config.include_articulation_points {
            let ap_config = ArticulationConfig {
                relationship_types: self.config.relationship_types.clone(),
            };
            let ap = ArticulationPoints::with_config(self.engine, ap_config);
            let ap_result = ap.find().await?;
            result.is_biconnected = ap_result.count == 0;
            result.articulation_points = Some(ap_result);
        }

        // 단절선 분석
        if self.config.include_bridges {
            let br_config = BridgeConfig {
                relationship_types: self.config.relationship_types.clone(),
            };
            let br = Bridges::with_config(self.engine, br_config);
            let br_result = br.find().await?;
            result.is_bridge_free = br_result.count == 0;
            result.bridges = Some(br_result);
        }

        // 이중 연결 성분 분석
        if self.config.include_biconnected || self.config.include_block_cut_tree {
            let bc_config = BiconnectedConfig {
                relationship_types: self.config.relationship_types.clone(),
            };
            let bc = BiconnectedComponents::with_config(self.engine, bc_config);

            if self.config.include_block_cut_tree {
                let bct_result = bc.build_block_cut_tree().await?;
                result.biconnected = Some(bct_result.components.clone());
                result.block_cut_tree = Some(bct_result);
            } else {
                result.biconnected = Some(bc.find().await?);
            }
        }

        Ok(result)
    }

    /// 그래프가 이중 연결인지 빠르게 확인
    pub async fn is_biconnected(&self) -> Result<bool> {
        let ap_config = ArticulationConfig {
            relationship_types: self.config.relationship_types.clone(),
        };
        let ap = ArticulationPoints::with_config(self.engine, ap_config);
        let result = ap.find().await?;
        Ok(result.count == 0)
    }

    /// 그래프가 다리 없는지 빠르게 확인
    pub async fn is_bridge_free(&self) -> Result<bool> {
        let br_config = BridgeConfig {
            relationship_types: self.config.relationship_types.clone(),
        };
        let br = Bridges::with_config(self.engine, br_config);
        let result = br.find().await?;
        Ok(result.count == 0)
    }

    /// 그래프가 구조적으로 견고한지 빠르게 확인
    pub async fn is_structurally_robust(&self) -> Result<bool> {
        Ok(self.is_biconnected().await? && self.is_bridge_free().await?)
    }

    /// 특정 노드가 핵심 노드인지 확인 (단절점 또는 단절선의 끝점)
    pub async fn is_critical_node(&self, node_id: NodeId) -> Result<bool> {
        // 단절점 확인
        let ap_config = ArticulationConfig {
            relationship_types: self.config.relationship_types.clone(),
        };
        let ap = ArticulationPoints::with_config(self.engine, ap_config);
        if ap.is_articulation_point(node_id).await? {
            return Ok(true);
        }

        // 단절선의 끝점인지 확인
        let br_config = BridgeConfig {
            relationship_types: self.config.relationship_types.clone(),
        };
        let br = Bridges::with_config(self.engine, br_config);
        let bridges = br.find().await?;
        for (from, to) in bridges.bridges {
            if from == node_id || to == node_id {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// 모든 핵심 노드 찾기 (단절점 + 단절선 끝점)
    pub async fn find_critical_nodes(&self) -> Result<Vec<NodeId>> {
        let mut critical = HashSet::new();

        // 단절점 추가
        let ap_config = ArticulationConfig {
            relationship_types: self.config.relationship_types.clone(),
        };
        let ap = ArticulationPoints::with_config(self.engine, ap_config);
        let ap_result = ap.find().await?;
        for node in ap_result.points {
            critical.insert(node);
        }

        // 단절선 끝점 추가
        let br_config = BridgeConfig {
            relationship_types: self.config.relationship_types.clone(),
        };
        let br = Bridges::with_config(self.engine, br_config);
        let br_result = br.find().await?;
        for (from, to) in br_result.bridges {
            critical.insert(from);
            critical.insert(to);
        }

        let mut result: Vec<NodeId> = critical.into_iter().collect();
        result.sort();
        Ok(result)
    }
}

/// 전체 구조 분석 수행 (편의 함수)
pub async fn analyze_structure(engine: &GraphEngine) -> Result<GraphStructureResult> {
    GraphStructure::new(engine).analyze().await
}

/// 전체 구조 분석 수행 (모든 옵션 포함)
pub async fn analyze_structure_full(engine: &GraphEngine) -> Result<GraphStructureResult> {
    GraphStructure::with_config(engine, GraphStructureConfig::full())
        .analyze()
        .await
}

/// 그래프가 구조적으로 견고한지 확인 (편의 함수)
pub async fn is_structurally_robust(engine: &GraphEngine) -> Result<bool> {
    GraphStructure::new(engine).is_structurally_robust().await
}
