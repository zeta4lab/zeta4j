// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Overlap Coefficient 알고리즘
//!
//! 두 노드의 이웃 집합 간 Overlap Coefficient를 계산합니다.
//!
//! Overlap(A, B) = |N(A) ∩ N(B)| / min(|N(A)|, |N(B)|)
//!
//! - 범위: 0.0 (교집합 없음) ~ 1.0 (한 집합이 다른 집합의 부분집합)
//! - 포함 관계 탐지에 적합 (크기 차이가 큰 집합 간 유사도)

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;

use super::{SimilarityResult, get_all_node_ids, get_neighbors};

/// Overlap Coefficient 설정
#[derive(Debug, Clone)]
pub struct OverlapConfig {
    /// 최소 유사도 임계값 (0.0 ~ 1.0)
    pub threshold: f64,
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 방향 고려 여부 (false면 양방향)
    pub directed: bool,
}

impl Default for OverlapConfig {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            relationship_types: None,
            directed: false,
        }
    }
}

impl OverlapConfig {
    /// 임계값 설정
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 방향 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }
}

/// Overlap Coefficient 계산기
pub struct OverlapCoefficient<'a> {
    engine: &'a GraphEngine,
    config: OverlapConfig,
}

impl<'a> OverlapCoefficient<'a> {
    /// 새로운 Overlap Coefficient 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: OverlapConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: OverlapConfig) -> Self {
        Self { engine, config }
    }

    /// 두 노드 간 Overlap Coefficient 계산
    ///
    /// # Arguments
    /// * `node1` - 첫 번째 노드 ID
    /// * `node2` - 두 번째 노드 ID
    ///
    /// # Returns
    /// Overlap Coefficient (0.0 ~ 1.0)
    pub async fn coefficient(&self, node1: NodeId, node2: NodeId) -> Result<f64> {
        let neighbors1 = get_neighbors(
            self.engine,
            node1,
            &self.config.relationship_types,
            self.config.directed,
        )
        .await?;
        let neighbors2 = get_neighbors(
            self.engine,
            node2,
            &self.config.relationship_types,
            self.config.directed,
        )
        .await?;

        // 둘 중 하나라도 이웃이 없으면 0.0 반환
        let min_size = neighbors1.len().min(neighbors2.len());
        if min_size == 0 {
            return Ok(0.0);
        }

        let intersection = neighbors1.intersection(&neighbors2).count();

        Ok(intersection as f64 / min_size as f64)
    }

    /// 전체 노드 쌍의 Overlap Coefficient 계산 (임계값 이상)
    ///
    /// # Returns
    /// 임계값 이상의 Overlap을 가진 모든 노드 쌍 (유사도 내림차순)
    pub async fn calculate_all(&self) -> Result<Vec<SimilarityResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut results = Vec::new();

        // 모든 노드 쌍에 대해 계산 (중복 제외: i < j)
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let coefficient = self.coefficient(nodes[i], nodes[j]).await?;

                if coefficient >= self.config.threshold {
                    results.push(SimilarityResult::new(nodes[i], nodes[j], coefficient));
                }
            }
        }

        // 유사도 내림차순 정렬
        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(Ordering::Equal)
        });

        Ok(results)
    }

    /// 특정 노드와 가장 높은 Overlap을 가진 top-K 노드 찾기
    ///
    /// # Arguments
    /// * `target_node` - 대상 노드 ID
    /// * `top_k` - 반환할 최대 노드 수
    ///
    /// # Returns
    /// 가장 높은 Overlap을 가진 top-K 노드들 (유사도 내림차순)
    pub async fn top_k_similar(
        &self,
        target_node: NodeId,
        top_k: usize,
    ) -> Result<Vec<SimilarityResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut results = Vec::new();

        for &node in &nodes {
            if node == target_node {
                continue;
            }

            let coefficient = self.coefficient(target_node, node).await?;

            if coefficient >= self.config.threshold {
                results.push(SimilarityResult::new(target_node, node, coefficient));
            }
        }

        // 유사도 내림차순 정렬
        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(Ordering::Equal)
        });

        // top-K만 반환
        results.truncate(top_k);

        Ok(results)
    }
}
