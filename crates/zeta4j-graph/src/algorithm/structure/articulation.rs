//! Articulation Points (단절점) 알고리즘
//!
//! 제거 시 그래프가 분리되는 노드를 찾습니다.
//! Tarjan의 DFS 기반 알고리즘을 사용합니다.

use crate::algorithm::dfs_context::DfsContext;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashSet;

/// 단절점 탐지 설정
#[derive(Debug, Clone, Default)]
pub struct ArticulationConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
}

impl ArticulationConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 단절점 탐지 결과
#[derive(Debug, Clone)]
pub struct ArticulationResult {
    /// 단절점 목록
    pub points: Vec<NodeId>,
    /// 단절점 개수
    pub count: usize,
}

/// 단절점 탐지기
pub struct ArticulationPoints<'a> {
    engine: &'a GraphEngine,
    config: ArticulationConfig,
}

impl<'a> ArticulationPoints<'a> {
    /// 새로운 ArticulationPoints 탐지기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ArticulationConfig::default(),
        }
    }

    /// 설정과 함께 탐지기 생성
    pub fn with_config(engine: &'a GraphEngine, config: ArticulationConfig) -> Self {
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

    /// 단절점 탐지 (Tarjan 알고리즘)
    pub async fn find(&self) -> Result<ArticulationResult> {
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(ArticulationResult {
                points: Vec::new(),
                count: 0,
            });
        }

        let mut ctx = DfsContext::with_capacity(nodes.len());
        let mut ap_set = HashSet::new(); // 단절점 집합

        // 모든 컴포넌트에 대해 DFS 수행
        for &start in &nodes {
            if !ctx.is_visited(start) {
                self.dfs_articulation(start, &mut ctx, &mut ap_set).await?;
            }
        }

        let mut points: Vec<NodeId> = ap_set.into_iter().collect();
        points.sort();

        Ok(ArticulationResult {
            count: points.len(),
            points,
        })
    }

    /// DFS로 단절점 탐지
    async fn dfs_articulation(
        &self,
        u: NodeId,
        ctx: &mut DfsContext,
        ap_set: &mut HashSet<NodeId>,
    ) -> Result<()> {
        // 반복적 DFS를 위한 스택
        // (node, neighbors_iter_state, children_count, is_processed)
        let mut stack: Vec<(NodeId, Vec<NodeId>, usize, bool)> = Vec::new();

        ctx.visit(u, None);

        let neighbors: Vec<NodeId> = self.get_neighbors(u).await?.into_iter().collect();
        stack.push((u, neighbors, 0, false));

        while let Some((current, neighbors, children, processed)) = stack.pop() {
            if processed {
                // 후처리: 자식으로부터 low 값 업데이트
                ctx.propagate_low_to_parent(current);
                continue;
            }

            let mut new_children = children;
            let mut i = 0;
            let mut should_continue = false;

            for neighbor in &neighbors {
                i += 1;
                if i <= children {
                    continue; // 이미 처리한 이웃 건너뛰기
                }

                let v = *neighbor;

                if !ctx.is_visited(v) {
                    new_children += 1;
                    ctx.visit(v, Some(current));

                    // 현재 노드를 스택에 다시 넣고 (상태 저장)
                    stack.push((current, neighbors.clone(), new_children, false));

                    // 자식 노드 처리
                    let child_neighbors: Vec<NodeId> =
                        self.get_neighbors(v).await?.into_iter().collect();
                    stack.push((v, child_neighbors, 0, false));

                    should_continue = true;
                    break;
                } else if ctx.get_parent(current) != Some(v) {
                    // 역방향 엣지: low 값 업데이트
                    let v_disc = ctx.discovery_time(v);
                    ctx.update_low(current, v_disc);
                }
            }

            if should_continue {
                continue;
            }

            // 모든 이웃 처리 완료
            // 후처리를 위해 스택에 추가
            // 단절점 판단
            let current_parent = ctx.get_parent(current);
            let current_disc = ctx.discovery_time(current);

            if current_parent.is_none() {
                // 루트 노드: 자식이 2개 이상이면 단절점
                if new_children > 1 {
                    ap_set.insert(current);
                }
            } else {
                // 비루트 노드: 자식 v에 대해 low[v] >= disc[current]이면 단절점
                for neighbor in &neighbors {
                    let v = *neighbor;
                    if ctx.get_parent(v) == Some(current) {
                        let v_low = ctx.low_link(v);
                        if v_low >= current_disc {
                            ap_set.insert(current);
                            break;
                        }
                    }
                }
            }

            stack.push((current, neighbors, new_children, true));
        }

        Ok(())
    }

    /// 특정 노드가 단절점인지 확인
    pub async fn is_articulation_point(&self, node_id: NodeId) -> Result<bool> {
        let result = self.find().await?;
        Ok(result.points.contains(&node_id))
    }
}

/// 단절점 탐지 (편의 함수)
pub async fn find_articulation_points(engine: &GraphEngine) -> Result<ArticulationResult> {
    ArticulationPoints::new(engine).find().await
}

/// 특정 노드가 단절점인지 확인 (편의 함수)
pub async fn is_articulation_point(engine: &GraphEngine, node_id: NodeId) -> Result<bool> {
    ArticulationPoints::new(engine)
        .is_articulation_point(node_id)
        .await
}
