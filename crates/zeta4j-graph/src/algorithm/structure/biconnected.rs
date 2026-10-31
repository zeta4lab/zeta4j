// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Biconnected Components (이중 연결 성분) 알고리즘
//!
//! 단절점 없이 연결된 최대 부분 그래프를 찾습니다.
//! Tarjan의 DFS 기반 알고리즘 + 스택을 사용합니다.

use crate::algorithm::dfs_context::DfsContext;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 이중 연결 성분 탐지 설정
#[derive(Debug, Clone, Default)]
pub struct BiconnectedConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
}

impl BiconnectedConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 이중 연결 성분 탐지 결과
#[derive(Debug, Clone)]
pub struct BiconnectedResult {
    /// 각 컴포넌트의 엣지 목록 [(from, to), ...]
    pub components: Vec<Vec<(NodeId, NodeId)>>,
    /// 각 컴포넌트의 노드 집합
    pub component_nodes: Vec<HashSet<NodeId>>,
    /// 컴포넌트 개수
    pub count: usize,
}

/// Block-Cut Tree 노드 타입
#[derive(Debug, Clone, PartialEq)]
pub enum BlockCutNodeType {
    /// Block (Biconnected Component)
    Block(usize),
    /// Cut Vertex (Articulation Point)
    CutVertex(NodeId),
}

/// Block-Cut Tree 결과
#[derive(Debug, Clone)]
pub struct BlockCutTreeResult {
    /// Block-Cut Tree 노드 목록
    pub nodes: Vec<BlockCutNodeType>,
    /// Block-Cut Tree 엣지 (트리 구조)
    pub edges: Vec<(usize, usize)>,
    /// 단절점 목록
    pub cut_vertices: Vec<NodeId>,
    /// Biconnected Components
    pub components: BiconnectedResult,
}

/// 이중 연결 성분 탐지기
pub struct BiconnectedComponents<'a> {
    engine: &'a GraphEngine,
    config: BiconnectedConfig,
}

impl<'a> BiconnectedComponents<'a> {
    /// 새로운 BiconnectedComponents 탐지기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: BiconnectedConfig::default(),
        }
    }

    /// 설정과 함께 탐지기 생성
    pub fn with_config(engine: &'a GraphEngine, config: BiconnectedConfig) -> Self {
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

    /// 이중 연결 성분 탐지 (Tarjan 알고리즘 + 스택)
    pub async fn find(&self) -> Result<BiconnectedResult> {
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(BiconnectedResult {
                components: Vec::new(),
                component_nodes: Vec::new(),
                count: 0,
            });
        }

        let mut ctx = DfsContext::with_capacity(nodes.len());

        // 엣지 스택: (from, to)
        let mut edge_stack: Vec<(NodeId, NodeId)> = Vec::new();
        let mut components: Vec<Vec<(NodeId, NodeId)>> = Vec::new();

        // 모든 컴포넌트에 대해 DFS 수행
        for &start in &nodes {
            if !ctx.is_visited(start) {
                self.dfs_biconnected(start, &mut ctx, &mut edge_stack, &mut components)
                    .await?;

                // 남은 엣지가 있으면 마지막 컴포넌트
                if !edge_stack.is_empty() {
                    components.push(std::mem::take(&mut edge_stack));
                }
            }
        }

        // 각 컴포넌트의 노드 집합 계산
        let component_nodes: Vec<HashSet<NodeId>> = components
            .iter()
            .map(|edges| {
                let mut nodes = HashSet::new();
                for &(from, to) in edges {
                    nodes.insert(from);
                    nodes.insert(to);
                }
                nodes
            })
            .collect();

        let count = components.len();

        Ok(BiconnectedResult {
            components,
            component_nodes,
            count,
        })
    }

    /// DFS로 이중 연결 성분 탐지
    async fn dfs_biconnected(
        &self,
        u: NodeId,
        ctx: &mut DfsContext,
        edge_stack: &mut Vec<(NodeId, NodeId)>,
        components: &mut Vec<Vec<(NodeId, NodeId)>>,
    ) -> Result<()> {
        // 반복적 DFS를 위한 스택
        // (node, neighbors, neighbor_index, children_count)
        let mut stack: Vec<(NodeId, Vec<NodeId>, usize, usize)> = Vec::new();

        ctx.visit(u, None);

        let neighbors: Vec<NodeId> = self.get_neighbors(u).await?.into_iter().collect();
        stack.push((u, neighbors, 0, 0));

        while let Some((current, neighbors, idx, children)) = stack.pop() {
            if idx < neighbors.len() {
                let v = neighbors[idx];

                // 다음 인덱스로 현재 상태 저장
                stack.push((current, neighbors.clone(), idx + 1, children));

                if !ctx.is_visited(v) {
                    // 트리 엣지
                    edge_stack.push((current, v));

                    ctx.visit(v, Some(current));

                    let v_neighbors: Vec<NodeId> =
                        self.get_neighbors(v).await?.into_iter().collect();
                    stack.push((v, v_neighbors, 0, 0));
                } else if ctx.get_parent(current) != Some(v) {
                    // 역방향 엣지 (부모가 아닌 방문된 노드)
                    let v_disc = ctx.discovery_time(v);
                    let current_disc = ctx.discovery_time(current);

                    // 이미 처리한 엣지가 아닌 경우만 스택에 추가
                    if v_disc < current_disc {
                        edge_stack.push((current, v));
                    }

                    ctx.update_low(current, v_disc);
                }
            } else {
                // 모든 이웃 처리 완료 - 후처리
                let current_parent = ctx.get_parent(current);

                if let Some(p) = current_parent {
                    // low 값 업데이트
                    let current_low = ctx.low_link(current);
                    ctx.update_low(p, current_low);

                    // 단절점 확인: low[current] >= disc[parent]
                    let p_disc = ctx.discovery_time(p);
                    if current_low >= p_disc {
                        // 새 biconnected component 발견
                        let mut component = Vec::new();
                        while let Some(&(from, to)) = edge_stack.last() {
                            if (from == p && to == current) || (from == current && to == p) {
                                if let Some(e) = edge_stack.pop() {
                                    component.push(e);
                                }
                                break;
                            }
                            if let Some(e) = edge_stack.pop() {
                                component.push(e);
                            }
                        }
                        if !component.is_empty() {
                            components.push(component);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Block-Cut Tree 생성
    pub async fn build_block_cut_tree(&self) -> Result<BlockCutTreeResult> {
        let bc_result = self.find().await?;

        // 단절점 찾기
        let mut cut_vertices = HashSet::new();

        // 노드가 2개 이상의 컴포넌트에 속하면 단절점
        let mut node_component_count: HashMap<NodeId, usize> = HashMap::new();
        for component_nodes in &bc_result.component_nodes {
            for &node in component_nodes {
                *node_component_count.entry(node).or_insert(0) += 1;
            }
        }

        for (&node, &count) in &node_component_count {
            if count > 1 {
                cut_vertices.insert(node);
            }
        }

        // Block-Cut Tree 구성
        let mut tree_nodes: Vec<BlockCutNodeType> = Vec::new();
        let mut tree_edges: Vec<(usize, usize)> = Vec::new();
        let mut cut_vertex_indices: HashMap<NodeId, usize> = HashMap::new();

        // Block 노드 추가
        for i in 0..bc_result.count {
            tree_nodes.push(BlockCutNodeType::Block(i));
        }

        // Cut Vertex 노드 추가
        for &cv in &cut_vertices {
            let idx = tree_nodes.len();
            tree_nodes.push(BlockCutNodeType::CutVertex(cv));
            cut_vertex_indices.insert(cv, idx);
        }

        // Block과 Cut Vertex 사이 엣지 추가
        for (block_idx, component_nodes) in bc_result.component_nodes.iter().enumerate() {
            for &node in component_nodes {
                if let Some(&cv_idx) = cut_vertex_indices.get(&node) {
                    tree_edges.push((block_idx, cv_idx));
                }
            }
        }

        let cut_vertices_vec: Vec<NodeId> = cut_vertices.into_iter().collect();

        Ok(BlockCutTreeResult {
            nodes: tree_nodes,
            edges: tree_edges,
            cut_vertices: cut_vertices_vec,
            components: bc_result,
        })
    }
}

/// 이중 연결 성분 탐지 (편의 함수)
pub async fn find_biconnected_components(engine: &GraphEngine) -> Result<BiconnectedResult> {
    BiconnectedComponents::new(engine).find().await
}

/// Block-Cut Tree 생성 (편의 함수)
pub async fn build_block_cut_tree(engine: &GraphEngine) -> Result<BlockCutTreeResult> {
    BiconnectedComponents::new(engine)
        .build_block_cut_tree()
        .await
}
