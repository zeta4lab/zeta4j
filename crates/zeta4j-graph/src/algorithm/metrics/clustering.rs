//! 클러스터링 계수 (Clustering Coefficient)
//!
//! 노드의 이웃들이 서로 얼마나 연결되어 있는지를 측정합니다.
//!
//! - Local Clustering Coefficient: C(v) = 2T(v) / (k(v) × (k(v)-1))
//!   - T(v): 노드 v가 속한 삼각형 개수
//!   - k(v): 노드 v의 degree
//!
//! - Global Clustering Coefficient (Transitivity):
//!   C = 3 × triangles / connected_triples
//!
//! - Average Clustering Coefficient:
//!   C_avg = (1/n) × Σ C(v)

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashSet;

/// 클러스터링 계수 설정
#[derive(Debug, Clone, Default)]
pub struct ClusteringConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
}

impl ClusteringConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 클러스터링 계수 계산기
pub struct ClusteringCoefficient<'a> {
    engine: &'a GraphEngine,
    config: ClusteringConfig,
}

impl<'a> ClusteringCoefficient<'a> {
    /// 새로운 ClusteringCoefficient 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ClusteringConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: ClusteringConfig) -> Self {
        Self { engine, config }
    }

    /// 노드의 이웃 집합 가져오기
    async fn get_neighbors(&self, node_id: NodeId) -> Result<HashSet<NodeId>> {
        let mut neighbors = HashSet::new();
        let edge_ids = self.engine.get_connected_edges(node_id).await;

        for edge_id in edge_ids {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from == node_id {
                    neighbors.insert(to);
                } else if to == node_id {
                    neighbors.insert(from);
                }
            }
        }

        Ok(neighbors)
    }

    /// 모든 노드 ID 가져오기
    async fn get_all_node_ids(&self) -> Result<Vec<NodeId>> {
        let mut nodes = Vec::new();
        let node_count = self.engine.node_count().await?;

        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
            }
        }

        Ok(nodes)
    }

    /// 노드가 속한 삼각형 개수 계산
    async fn count_node_triangles(&self, node_id: NodeId) -> Result<u64> {
        let neighbors = self.get_neighbors(node_id).await?;
        let neighbor_list: Vec<_> = neighbors.iter().cloned().collect();

        let mut count = 0u64;

        for i in 0..neighbor_list.len() {
            for j in (i + 1)..neighbor_list.len() {
                let n1 = neighbor_list[i];
                let n2 = neighbor_list[j];

                let n1_neighbors = self.get_neighbors(n1).await?;
                if n1_neighbors.contains(&n2) {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// 로컬 클러스터링 계수
    ///
    /// C(v) = 2T(v) / (k(v) × (k(v)-1))
    ///
    /// # Returns
    /// - 클러스터링 계수 (0.0 ~ 1.0)
    /// - degree가 0 또는 1이면 0.0 반환
    pub async fn local(&self, node_id: NodeId) -> Result<f64> {
        let neighbors = self.get_neighbors(node_id).await?;
        let degree = neighbors.len() as u64;

        // degree가 0 또는 1이면 삼각형을 형성할 수 없음
        if degree <= 1 {
            return Ok(0.0);
        }

        let triangles = self.count_node_triangles(node_id).await?;
        let possible_triangles = degree * (degree - 1) / 2;

        if possible_triangles == 0 {
            return Ok(0.0);
        }

        Ok(triangles as f64 / possible_triangles as f64)
    }

    /// 글로벌 클러스터링 계수 (전이성, Transitivity)
    ///
    /// C = 3 × triangles / connected_triples
    ///
    /// connected_triples = Σ k(v) × (k(v)-1) / 2
    pub async fn global(&self) -> Result<f64> {
        let nodes = self.get_all_node_ids().await?;

        let mut total_triangles = 0u64;
        let mut connected_triples = 0u64;

        for node_id in &nodes {
            let neighbors = self.get_neighbors(*node_id).await?;
            let degree = neighbors.len() as u64;

            if degree >= 2 {
                connected_triples += degree * (degree - 1) / 2;
            }

            total_triangles += self.count_node_triangles(*node_id).await?;
        }

        // 각 삼각형이 3번 카운트됨
        total_triangles /= 3;

        if connected_triples == 0 {
            return Ok(0.0);
        }

        // 글로벌 클러스터링 계수 = 3 × triangles / triples
        Ok(3.0 * total_triangles as f64 / connected_triples as f64)
    }

    /// 평균 클러스터링 계수
    ///
    /// C_avg = (1/n) × Σ C(v)
    pub async fn average(&self) -> Result<f64> {
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(0.0);
        }

        let mut sum = 0.0;
        let mut count = 0;

        for node_id in nodes {
            let local_cc = self.local(node_id).await?;
            sum += local_cc;
            count += 1;
        }

        if count == 0 {
            return Ok(0.0);
        }

        Ok(sum / count as f64)
    }

    /// 모든 노드의 로컬 클러스터링 계수
    pub async fn all_local(&self) -> Result<Vec<(NodeId, f64)>> {
        let nodes = self.get_all_node_ids().await?;
        let mut results = Vec::new();

        for node_id in nodes {
            let cc = self.local(node_id).await?;
            results.push((node_id, cc));
        }

        // 클러스터링 계수 기준 내림차순 정렬
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(results)
    }
}

/// 로컬 클러스터링 계수 계산 (편의 함수)
pub async fn local_clustering_coefficient(engine: &GraphEngine, node_id: NodeId) -> Result<f64> {
    ClusteringCoefficient::new(engine).local(node_id).await
}

/// 글로벌 클러스터링 계수 계산 (편의 함수)
pub async fn global_clustering_coefficient(engine: &GraphEngine) -> Result<f64> {
    ClusteringCoefficient::new(engine).global().await
}

/// 평균 클러스터링 계수 계산 (편의 함수)
pub async fn average_clustering_coefficient(engine: &GraphEngine) -> Result<f64> {
    ClusteringCoefficient::new(engine).average().await
}
