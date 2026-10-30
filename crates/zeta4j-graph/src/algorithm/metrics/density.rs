//! 그래프 밀도 (Graph Density)
//!
//! 그래프의 연결 밀도를 측정합니다.
//!
//! - 무방향 그래프: 2E / (V × (V-1))
//! - 방향 그래프: E / (V × (V-1))
//!
//! 범위: 0.0 (희소) ~ 1.0 (완전 그래프)

use crate::engine::GraphEngine;
use crate::error::Result;

/// 그래프 밀도 설정
#[derive(Debug, Clone, Default)]
pub struct DensityConfig {
    /// 방향 그래프 여부
    pub directed: bool,
}

impl DensityConfig {
    /// 방향 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }
}

/// 그래프 밀도 계산기
pub struct Density<'a> {
    engine: &'a GraphEngine,
    config: DensityConfig,
}

impl<'a> Density<'a> {
    /// 새로운 Density 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: DensityConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: DensityConfig) -> Self {
        Self { engine, config }
    }

    /// 그래프 밀도 계산
    ///
    /// # Returns
    /// - 밀도 값 (0.0 ~ 1.0)
    /// - 노드가 0 또는 1개면 0.0 반환
    pub async fn calculate(&self) -> Result<f64> {
        let node_count = self.engine.node_count().await? as u64;
        let edge_count = self.engine.edge_count().await as u64;

        // 노드가 0 또는 1개면 밀도는 정의되지 않음 (0.0 반환)
        if node_count <= 1 {
            return Ok(0.0);
        }

        let max_edges = node_count * (node_count - 1);

        if max_edges == 0 {
            return Ok(0.0);
        }

        let density = if self.config.directed {
            // 방향 그래프: E / (V × (V-1))
            edge_count as f64 / max_edges as f64
        } else {
            // 무방향 그래프: 2E / (V × (V-1))
            // 실제로는 각 엣지가 한 번만 저장되므로 E / (V × (V-1) / 2)
            (2 * edge_count) as f64 / max_edges as f64
        };

        Ok(density.min(1.0)) // 1.0을 초과하지 않도록
    }
}

/// 그래프 밀도 계산 (편의 함수)
pub async fn calculate_density(engine: &GraphEngine, directed: bool) -> Result<f64> {
    let config = DensityConfig { directed };
    Density::with_config(engine, config).calculate().await
}
