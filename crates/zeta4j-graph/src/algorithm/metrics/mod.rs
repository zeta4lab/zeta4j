// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 그래프 메트릭 (Graph Metrics)
//!
//! 그래프 전체의 구조적 특성을 분석하는 메트릭들을 제공합니다.
//!
//! - Density: 그래프 밀도
//! - Triangle Count: 삼각형 개수
//! - Clustering Coefficient: 클러스터링 계수
//! - Distance Metrics: 직경, 반경, 평균 경로 길이

mod clustering;
mod density;
mod distance;
mod triangle;

pub use clustering::{
    ClusteringCoefficient, ClusteringConfig, average_clustering_coefficient,
    global_clustering_coefficient, local_clustering_coefficient,
};
pub use density::{Density, DensityConfig, calculate_density};
pub use distance::{
    DistanceConfig, DistanceMetrics, average_path_length, diameter, eccentricity, radius,
};
pub use triangle::{TriangleConfig, TriangleCount, count_node_triangles, count_triangles};

use crate::engine::GraphEngine;
use crate::error::Result;

/// 그래프 메트릭 통합 결과 (기본)
#[derive(Debug, Clone)]
pub struct GraphMetricsResult {
    /// 노드 수
    pub node_count: u64,
    /// 엣지 수
    pub edge_count: u64,
    /// 그래프 밀도
    pub density: f64,
    /// 삼각형 개수
    pub triangle_count: u64,
    /// 글로벌 클러스터링 계수
    pub global_clustering_coefficient: f64,
    /// 평균 클러스터링 계수
    pub average_clustering_coefficient: f64,
}

/// 그래프 메트릭 전체 결과 (거리 메트릭 포함)
#[derive(Debug, Clone)]
pub struct GraphMetricsFullResult {
    /// 기본 메트릭
    pub basic: GraphMetricsResult,
    /// 그래프 직경 (비연결 시 None)
    pub diameter: Option<u64>,
    /// 그래프 반경 (비연결 시 None)
    pub radius: Option<u64>,
    /// 평균 경로 길이 (비연결 시 None)
    pub average_path_length: Option<f64>,
}

/// 그래프 메트릭 통합 계산기
pub struct GraphMetrics<'a> {
    engine: &'a GraphEngine,
    /// 방향 그래프 여부
    pub directed: bool,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl<'a> GraphMetrics<'a> {
    /// 새로운 GraphMetrics 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            directed: false,
            relationship_types: None,
        }
    }

    /// 방향 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 모든 기본 메트릭 계산
    pub async fn calculate_all(&self) -> Result<GraphMetricsResult> {
        let node_count = self.engine.node_count().await? as u64;
        let edge_count = self.engine.edge_count().await as u64;

        let density_config = DensityConfig {
            directed: self.directed,
        };
        let density = Density::with_config(self.engine, density_config)
            .calculate()
            .await?;

        let triangle_config = TriangleConfig {
            relationship_types: self.relationship_types.clone(),
        };
        let triangle_count = TriangleCount::with_config(self.engine, triangle_config)
            .total_count()
            .await?;

        let clustering_config = ClusteringConfig {
            relationship_types: self.relationship_types.clone(),
        };
        let cc = ClusteringCoefficient::with_config(self.engine, clustering_config);
        let global_cc = cc.global().await?;
        let avg_cc = cc.average().await?;

        Ok(GraphMetricsResult {
            node_count,
            edge_count,
            density,
            triangle_count,
            global_clustering_coefficient: global_cc,
            average_clustering_coefficient: avg_cc,
        })
    }

    /// 밀도만 계산
    pub async fn density(&self) -> Result<f64> {
        calculate_density(self.engine, self.directed).await
    }

    /// 삼각형 개수만 계산
    pub async fn triangle_count(&self) -> Result<u64> {
        let config = TriangleConfig {
            relationship_types: self.relationship_types.clone(),
        };
        TriangleCount::with_config(self.engine, config)
            .total_count()
            .await
    }

    /// 글로벌 클러스터링 계수만 계산
    pub async fn global_clustering_coefficient(&self) -> Result<f64> {
        let config = ClusteringConfig {
            relationship_types: self.relationship_types.clone(),
        };
        ClusteringCoefficient::with_config(self.engine, config)
            .global()
            .await
    }

    /// 평균 클러스터링 계수만 계산
    pub async fn average_clustering_coefficient(&self) -> Result<f64> {
        let config = ClusteringConfig {
            relationship_types: self.relationship_types.clone(),
        };
        ClusteringCoefficient::with_config(self.engine, config)
            .average()
            .await
    }

    /// 그래프 직경 계산
    pub async fn diameter(&self) -> Result<Option<u64>> {
        let config =
            DistanceConfig::default().with_relationship_types_opt(self.relationship_types.clone());
        DistanceMetrics::with_config(self.engine, config)
            .diameter()
            .await
    }

    /// 그래프 반경 계산
    pub async fn radius(&self) -> Result<Option<u64>> {
        let config =
            DistanceConfig::default().with_relationship_types_opt(self.relationship_types.clone());
        DistanceMetrics::with_config(self.engine, config)
            .radius()
            .await
    }

    /// 평균 경로 길이 계산
    pub async fn average_path_length(&self) -> Result<Option<f64>> {
        let config =
            DistanceConfig::default().with_relationship_types_opt(self.relationship_types.clone());
        DistanceMetrics::with_config(self.engine, config)
            .average_path_length()
            .await
    }

    /// 모든 메트릭 계산 (거리 메트릭 포함, 대규모 그래프에서 느릴 수 있음)
    pub async fn calculate_full(&self) -> Result<GraphMetricsFullResult> {
        let basic = self.calculate_all().await?;

        let distance_config =
            DistanceConfig::default().with_relationship_types_opt(self.relationship_types.clone());
        let dm = DistanceMetrics::with_config(self.engine, distance_config);

        Ok(GraphMetricsFullResult {
            basic,
            diameter: dm.diameter().await?,
            radius: dm.radius().await?,
            average_path_length: dm.average_path_length().await?,
        })
    }
}
