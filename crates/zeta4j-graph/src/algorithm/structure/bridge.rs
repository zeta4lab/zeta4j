//! Bridges (단절선) 알고리즘
//!
//! 제거 시 그래프가 분리되는 엣지를 찾습니다.
//! Tarjan의 DFS 기반 알고리즘을 사용합니다.

use crate::algorithm::dfs_context::DfsContext;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashSet;

/// 단절선 탐지 설정
#[derive(Debug, Clone, Default)]
pub struct BridgeConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
}

impl BridgeConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 단절선 탐지 결과
#[derive(Debug, Clone)]
pub struct BridgeResult {
    /// 단절선 목록 (from, to) - 정렬된 형태
    pub bridges: Vec<(NodeId, NodeId)>,
    /// 단절선 개수
    pub count: usize,
}

/// 단절선 탐지기
pub struct Bridges<'a> {
    engine: &'a GraphEngine,
    config: BridgeConfig,
}

impl<'a> Bridges<'a> {
    /// 새로운 Bridges 탐지기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: BridgeConfig::default(),
        }
    }

    /// 설정과 함께 탐지기 생성
    pub fn with_config(engine: &'a GraphEngine, config: BridgeConfig) -> Self {
        Self { engine, config }
    }

    /// 노드의 이웃 가져오기 (무방향)
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

    /// 단절선 탐지 (Tarjan 알고리즘)
    pub async fn find(&self) -> Result<BridgeResult> {
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(BridgeResult {
                bridges: Vec::new(),
                count: 0,
            });
        }

        let mut ctx = DfsContext::with_capacity(nodes.len());
        let mut bridge_set = HashSet::new();

        // 모든 컴포넌트에 대해 DFS 수행
        for &start in &nodes {
            if !ctx.is_visited(start) {
                self.dfs_bridge(start, &mut ctx, &mut bridge_set).await?;
            }
        }

        let mut bridges: Vec<(NodeId, NodeId)> = bridge_set
            .into_iter()
            .map(|(a, b)| if a < b { (a, b) } else { (b, a) })
            .collect();
        bridges.sort();

        Ok(BridgeResult {
            count: bridges.len(),
            bridges,
        })
    }

    /// DFS로 단절선 탐지
    async fn dfs_bridge(
        &self,
        start: NodeId,
        ctx: &mut DfsContext,
        bridge_set: &mut HashSet<(NodeId, NodeId)>,
    ) -> Result<()> {
        // 반복적 DFS를 위한 스택
        let mut stack: Vec<(NodeId, Vec<NodeId>, usize, bool)> = Vec::new();

        ctx.visit(start, None);

        let neighbors: Vec<NodeId> = self.get_neighbors(start).await?.into_iter().collect();
        stack.push((start, neighbors, 0, false));

        while let Some((current, neighbors, processed_count, is_postprocess)) = stack.pop() {
            if is_postprocess {
                // 후처리: 부모의 low 값 업데이트 및 브릿지 확인
                if let Some(p) = ctx.get_parent(current) {
                    let current_low = ctx.low_link(current);
                    ctx.update_low(p, current_low);

                    // 브릿지 조건: low[current] > disc[parent]
                    let parent_disc = ctx.discovery_time(p);
                    if current_low > parent_disc {
                        bridge_set.insert((p, current));
                    }
                }
                continue;
            }

            let mut new_processed = processed_count;
            let mut should_recurse = false;

            for (i, neighbor) in neighbors.iter().enumerate() {
                if i < processed_count {
                    continue;
                }

                let v = *neighbor;
                new_processed = i + 1;

                if !ctx.is_visited(v) {
                    ctx.visit(v, Some(current));

                    // 현재 상태 저장
                    stack.push((current, neighbors.clone(), new_processed, false));

                    // 자식 노드 처리
                    let child_neighbors: Vec<NodeId> =
                        self.get_neighbors(v).await?.into_iter().collect();
                    stack.push((v, child_neighbors, 0, false));

                    should_recurse = true;
                    break;
                } else if ctx.get_parent(current) != Some(v) {
                    // 역방향 엣지
                    let v_disc = ctx.discovery_time(v);
                    ctx.update_low(current, v_disc);
                }
            }

            if !should_recurse {
                // 모든 이웃 처리 완료, 후처리 예약
                stack.push((current, neighbors, new_processed, true));
            }
        }

        Ok(())
    }

    /// 특정 엣지가 단절선인지 확인
    pub async fn is_bridge(&self, from: NodeId, to: NodeId) -> Result<bool> {
        let result = self.find().await?;
        let edge = if from < to { (from, to) } else { (to, from) };
        Ok(result.bridges.contains(&edge))
    }
}

/// 단절선 탐지 (편의 함수)
pub async fn find_bridges(engine: &GraphEngine) -> Result<BridgeResult> {
    Bridges::new(engine).find().await
}

/// 특정 엣지가 단절선인지 확인 (편의 함수)
pub async fn is_bridge(engine: &GraphEngine, from: NodeId, to: NodeId) -> Result<bool> {
    Bridges::new(engine).is_bridge(from, to).await
}
