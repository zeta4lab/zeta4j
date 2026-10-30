//! Condensation Graph (압축 그래프)
//!
//! SCC를 노드로 압축하여 DAG를 생성합니다.
//! SCC 간의 의존 관계를 분석하는 데 사용됩니다.

use super::{SCCResult, tarjan_scc};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashSet;

/// Condensation Graph 결과
#[derive(Debug, Clone)]
pub struct CondensationGraph {
    /// SCC 인덱스들 (노드)
    pub scc_nodes: Vec<usize>,
    /// SCC 간 엣지 (from_scc, to_scc)
    pub scc_edges: Vec<(usize, usize)>,
    /// 각 SCC의 크기
    pub scc_sizes: Vec<usize>,
    /// 원본 SCC 결과
    pub scc_result: SCCResult,
}

impl CondensationGraph {
    /// 빈 Condensation Graph
    pub fn empty() -> Self {
        Self {
            scc_nodes: Vec::new(),
            scc_edges: Vec::new(),
            scc_sizes: Vec::new(),
            scc_result: SCCResult::empty(),
        }
    }

    /// SCC 개수
    pub fn scc_count(&self) -> usize {
        self.scc_nodes.len()
    }

    /// 엣지 개수
    pub fn edge_count(&self) -> usize {
        self.scc_edges.len()
    }

    /// 특정 SCC의 크기
    pub fn scc_size(&self, scc_idx: usize) -> Option<usize> {
        self.scc_sizes.get(scc_idx).copied()
    }

    /// 가장 큰 SCC 인덱스
    pub fn largest_scc_index(&self) -> Option<usize> {
        self.scc_sizes
            .iter()
            .enumerate()
            .max_by_key(|(_, size)| *size)
            .map(|(idx, _)| idx)
    }

    /// 가장 큰 SCC의 노드들
    pub fn largest_scc(&self) -> Vec<NodeId> {
        if let Some(idx) = self.largest_scc_index() {
            self.scc_result
                .components
                .get(idx)
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// 소스 SCC들 (들어오는 엣지 없음)
    pub fn source_sccs(&self) -> Vec<usize> {
        let mut has_incoming: HashSet<usize> = HashSet::new();
        for &(_, to) in &self.scc_edges {
            has_incoming.insert(to);
        }

        self.scc_nodes
            .iter()
            .filter(|&&idx| !has_incoming.contains(&idx))
            .copied()
            .collect()
    }

    /// 싱크 SCC들 (나가는 엣지 없음)
    pub fn sink_sccs(&self) -> Vec<usize> {
        let mut has_outgoing: HashSet<usize> = HashSet::new();
        for &(from, _) in &self.scc_edges {
            has_outgoing.insert(from);
        }

        self.scc_nodes
            .iter()
            .filter(|&&idx| !has_outgoing.contains(&idx))
            .copied()
            .collect()
    }
}

/// Condensation Graph 빌더
pub struct CondensationBuilder<'a> {
    engine: &'a GraphEngine,
}

impl<'a> CondensationBuilder<'a> {
    /// 새로운 빌더 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self { engine }
    }

    /// Condensation Graph 생성
    pub async fn build(&self) -> Result<CondensationGraph> {
        let scc_result = tarjan_scc(self.engine).await?;

        if scc_result.components.is_empty() {
            return Ok(CondensationGraph::empty());
        }

        let scc_nodes: Vec<usize> = (0..scc_result.count).collect();
        let scc_sizes: Vec<usize> = scc_result.components.iter().map(|c| c.len()).collect();

        // SCC 간 엣지 수집
        let mut scc_edges_set: HashSet<(usize, usize)> = HashSet::new();

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                let from = edge.from_node();
                let to = edge.to_node();

                if from == to {
                    continue;
                }

                if let (Some(&from_scc), Some(&to_scc)) = (
                    scc_result.component_map.get(&from),
                    scc_result.component_map.get(&to),
                ) && from_scc != to_scc
                {
                    scc_edges_set.insert((from_scc, to_scc));
                }
            }
        }

        let mut scc_edges: Vec<(usize, usize)> = scc_edges_set.into_iter().collect();
        scc_edges.sort();

        Ok(CondensationGraph {
            scc_nodes,
            scc_edges,
            scc_sizes,
            scc_result,
        })
    }
}

/// 편의 함수: Condensation Graph 생성
pub async fn condensation_graph(engine: &GraphEngine) -> Result<CondensationGraph> {
    CondensationBuilder::new(engine).build().await
}

/// 편의 함수: 그래프가 강연결인지 확인
pub async fn is_strongly_connected(engine: &GraphEngine) -> Result<bool> {
    let result = tarjan_scc(engine).await?;
    Ok(result.is_strongly_connected)
}

/// 편의 함수: 가장 큰 SCC 반환
pub async fn largest_scc(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    let graph = condensation_graph(engine).await?;
    Ok(graph.largest_scc())
}

/// 편의 함수: SCC 개수
pub async fn scc_count(engine: &GraphEngine) -> Result<usize> {
    let result = tarjan_scc(engine).await?;
    Ok(result.count)
}
