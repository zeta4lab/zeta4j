//! Tarjan's Strongly Connected Components Algorithm
//!
//! 단일 DFS로 방향 그래프의 모든 강연결요소(SCC)를 찾습니다.
//! Low-link 값을 활용하여 O(V + E) 시간복잡도를 달성합니다.

use super::SCCResult;
use crate::algorithm::dfs_context::SccContext;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Tarjan 알고리즘 설정
#[derive(Debug, Clone, Default)]
pub struct TarjanConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl TarjanConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Tarjan's SCC 알고리즘
pub struct TarjanSCC<'a> {
    engine: &'a GraphEngine,
    config: TarjanConfig,
}

impl<'a> TarjanSCC<'a> {
    /// 새로운 TarjanSCC 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: TarjanConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: TarjanConfig) -> Self {
        Self { engine, config }
    }

    /// SCC 찾기
    pub async fn find_scc(&self) -> Result<SCCResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(SCCResult::empty());
        }

        let mut ctx = SccContext::with_capacity(adjacency.len());
        let mut components: Vec<Vec<NodeId>> = Vec::new();

        // 모든 노드에 대해 DFS
        for &node in adjacency.keys() {
            if !ctx.is_visited(node) {
                self.strongconnect(node, &adjacency, &mut ctx, &mut components);
            }
        }

        // component_map 구축
        let mut component_map: HashMap<NodeId, usize> = HashMap::new();
        for (idx, component) in components.iter().enumerate() {
            for &node in component {
                component_map.insert(node, idx);
            }
        }

        let count = components.len();
        let node_count = adjacency.len();
        let is_strongly_connected = count == 1 && !components.is_empty();

        // 각 SCC 정렬
        for component in &mut components {
            component.sort();
        }

        Ok(SCCResult {
            components,
            component_map,
            count,
            is_strongly_connected,
            node_count,
            algorithm: "Tarjan".to_string(),
        })
    }

    /// Tarjan의 strongconnect 함수
    fn strongconnect(
        &self,
        node: NodeId,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        ctx: &mut SccContext,
        components: &mut Vec<Vec<NodeId>>,
    ) {
        // 노드에 인덱스 할당
        ctx.visit(node);

        // 인접 노드 탐색
        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                if !ctx.is_visited(neighbor) {
                    // 아직 방문하지 않은 노드
                    self.strongconnect(neighbor, adjacency, ctx, components);
                    let neighbor_low = ctx.low_link(neighbor);
                    ctx.update_low(node, neighbor_low);
                } else if ctx.is_on_stack(neighbor) {
                    // 스택에 있는 노드 (백 엣지)
                    let neighbor_idx = ctx.index(neighbor);
                    ctx.update_low(node, neighbor_idx);
                }
            }
        }

        // 루트 노드인 경우 SCC 생성
        if ctx.is_root(node) {
            let component = ctx.extract_scc(node);
            components.push(component);
        }
    }

    /// 인접 리스트 구축 (방향 그래프)
    async fn build_adjacency(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, Vec::new());
            }
        }

        // 엣지 수집 (방향 유지)
        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from != to {
                    adjacency.entry(from).or_default().push(to);
                }
            }
        }

        Ok(adjacency)
    }
}

/// 편의 함수: Tarjan SCC
pub async fn tarjan_scc(engine: &GraphEngine) -> Result<SCCResult> {
    TarjanSCC::new(engine).find_scc().await
}
