//! 병렬 삼각형 계수 알고리즘
//!
//! Rayon을 사용하여 삼각형 계수 알고리즘을 병렬화합니다.
//! 각 노드별 삼각형 계수를 병렬로 실행하여 성능을 향상시킵니다.

use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

use super::config::ParallelConfig;
use crate::algorithm::metrics::TriangleConfig;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;

/// 병렬 삼각형 계수 결과
#[derive(Debug, Clone)]
pub struct ParallelTriangleResult {
    /// 전체 삼각형 개수
    pub total_triangles: usize,
    /// 노드별 삼각형 개수
    pub node_triangles: HashMap<NodeId, usize>,
}

impl ParallelTriangleResult {
    /// 특정 노드의 삼각형 수
    pub fn triangles_for_node(&self, node_id: NodeId) -> usize {
        self.node_triangles.get(&node_id).copied().unwrap_or(0)
    }
}

/// 병렬 삼각형 계수 계산기
pub struct ParallelTriangleCount<'a> {
    engine: &'a GraphEngine,
    config: TriangleConfig,
    par_config: ParallelConfig,
}

impl<'a> ParallelTriangleCount<'a> {
    /// 새로운 병렬 삼각형 계수 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: TriangleConfig::default(),
            par_config: ParallelConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: TriangleConfig) -> Self {
        Self {
            engine,
            config,
            par_config: ParallelConfig::default(),
        }
    }

    /// 병렬 설정 지정
    pub fn with_parallel_config(mut self, par_config: ParallelConfig) -> Self {
        self.par_config = par_config;
        self
    }

    /// 병렬 삼각형 계수 계산
    pub async fn calculate(&self) -> Result<ParallelTriangleResult> {
        triangle_count_parallel(self.engine, &self.config, &self.par_config).await
    }
}

/// 병렬 삼각형 계수 계산 함수
pub async fn triangle_count_parallel(
    engine: &GraphEngine,
    config: &TriangleConfig,
    par_config: &ParallelConfig,
) -> Result<ParallelTriangleResult> {
    // 1. 그래프 구조 빌드
    let (nodes, adjacency) = build_adjacency(engine, config).await?;

    if nodes.is_empty() {
        return Ok(ParallelTriangleResult {
            total_triangles: 0,
            node_triangles: HashMap::new(),
        });
    }

    let n = nodes.len();

    // 2. 노드별 삼각형 계수 (병렬 vs 순차)
    let node_triangles: HashMap<NodeId, usize> = if par_config.should_parallelize(n) {
        nodes
            .par_iter()
            .map(|&node_id| {
                let count = count_node_triangles(node_id, &adjacency);
                (node_id, count)
            })
            .collect()
    } else {
        nodes
            .iter()
            .map(|&node_id| {
                let count = count_node_triangles(node_id, &adjacency);
                (node_id, count)
            })
            .collect()
    };

    // 3. 전체 삼각형 수 계산 (각 삼각형은 3개 노드에서 세어지므로 3으로 나눔)
    let total: usize = node_triangles.values().sum();
    let total_triangles = total / 3;

    Ok(ParallelTriangleResult {
        total_triangles,
        node_triangles,
    })
}

/// 특정 노드가 속한 삼각형 수 계산
fn count_node_triangles(node_id: NodeId, adjacency: &HashMap<NodeId, HashSet<NodeId>>) -> usize {
    let neighbors = match adjacency.get(&node_id) {
        Some(n) => n,
        None => return 0,
    };

    let mut count = 0;

    // 이웃 쌍을 확인하여 서로 연결되어 있는지 확인
    let neighbor_list: Vec<NodeId> = neighbors.iter().copied().collect();

    for i in 0..neighbor_list.len() {
        for j in (i + 1)..neighbor_list.len() {
            let u = neighbor_list[i];
            let v = neighbor_list[j];

            // u와 v가 서로 연결되어 있으면 삼각형
            if let Some(u_neighbors) = adjacency.get(&u)
                && u_neighbors.contains(&v)
            {
                count += 1;
            }
        }
    }

    count
}

/// 인접 리스트 빌드
async fn build_adjacency(
    engine: &GraphEngine,
    config: &TriangleConfig,
) -> Result<(Vec<NodeId>, HashMap<NodeId, HashSet<NodeId>>)> {
    let mut nodes: Vec<NodeId> = Vec::new();
    let mut adjacency: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

    // 노드 수집
    let node_count = engine.node_count().await?;
    for node_id in 1..=node_count as u64 {
        if engine.get_node(node_id).await?.is_some() {
            nodes.push(node_id);
            adjacency.insert(node_id, HashSet::new());
        }
    }

    // 엣지 수집
    let edge_count = engine.edge_count().await;
    for edge_id in 1..=edge_count as u64 {
        if let Some(edge) = engine.get_edge(edge_id).await? {
            // 관계 타입 필터링
            if let Some(ref types) = config.relationship_types
                && !types.contains(&edge.relation_type().to_string())
            {
                continue;
            }

            let from = edge.from_node();
            let to = edge.to_node();

            // 무방향 그래프로 처리
            if let Some(set) = adjacency.get_mut(&from) {
                set.insert(to);
            }
            if let Some(set) = adjacency.get_mut(&to) {
                set.insert(from);
            }
        }
    }

    Ok((nodes, adjacency))
}
