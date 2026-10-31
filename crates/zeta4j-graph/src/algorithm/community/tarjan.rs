// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Strongly Connected Components (SCC) - Tarjan 알고리즘
//!
//! 방향 그래프에서 강연결 컴포넌트를 찾습니다.
//! 강연결 컴포넌트란 모든 노드 쌍 사이에 양방향 경로가 존재하는 최대 부분그래프입니다.
//!
//! Tarjan 알고리즘 사용: 단일 DFS로 O(V + E) 시간 복잡도

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Tarjan 알고리즘 내부 상태
struct TarjanState {
    /// DFS 방문 순서 인덱스
    index: usize,
    /// 노드별 DFS 인덱스
    node_index: HashMap<NodeId, usize>,
    /// 노드별 low-link 값 (도달 가능한 최소 인덱스)
    low_link: HashMap<NodeId, usize>,
    /// 현재 스택에 있는지 여부
    on_stack: HashMap<NodeId, bool>,
    /// DFS 스택
    stack: Vec<NodeId>,
    /// 발견된 SCC들 (노드 목록)
    sccs: Vec<Vec<NodeId>>,
}

impl TarjanState {
    fn new() -> Self {
        Self {
            index: 0,
            node_index: HashMap::new(),
            low_link: HashMap::new(),
            on_stack: HashMap::new(),
            stack: Vec::new(),
            sccs: Vec::new(),
        }
    }
}

/// Strongly Connected Components 설정
#[derive(Debug, Clone, Default)]
pub struct StronglyConnectedConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub rel_types: Option<Vec<String>>,
}

impl StronglyConnectedConfig {
    /// 관계 타입 필터 설정
    pub fn with_rel_types(mut self, rel_types: Vec<String>) -> Self {
        self.rel_types = Some(rel_types);
        self
    }
}

/// Strongly Connected Components 결과
#[derive(Debug, Clone)]
pub struct StronglyConnectedResult {
    /// 노드별 SCC ID
    pub components: HashMap<NodeId, usize>,
    /// SCC 수
    pub component_count: usize,
    /// SCC별 노드 수
    pub component_sizes: HashMap<usize, usize>,
    /// SCC별 노드 목록 (위상 정렬 역순)
    pub scc_nodes: Vec<Vec<NodeId>>,
}

impl StronglyConnectedResult {
    /// 특정 노드의 SCC ID 조회
    pub fn component_id(&self, node_id: NodeId) -> Option<usize> {
        self.components.get(&node_id).copied()
    }

    /// 가장 큰 SCC의 ID와 크기 반환
    pub fn largest_component(&self) -> Option<(usize, usize)> {
        self.component_sizes
            .iter()
            .max_by_key(|&(_, &size)| size)
            .map(|(&id, &size)| (id, size))
    }

    /// 특정 SCC에 속한 노드들 반환
    pub fn nodes_in_component(&self, component_id: usize) -> Vec<NodeId> {
        self.scc_nodes
            .get(component_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 두 노드가 같은 SCC에 속하는지 확인
    pub fn same_component(&self, node1: NodeId, node2: NodeId) -> bool {
        match (self.component_id(node1), self.component_id(node2)) {
            (Some(c1), Some(c2)) => c1 == c2,
            _ => false,
        }
    }
}

/// Strongly Connected Components 계산기 (Tarjan 알고리즘)
pub struct StronglyConnectedComponents<'a> {
    engine: &'a GraphEngine,
    config: StronglyConnectedConfig,
}

impl<'a> StronglyConnectedComponents<'a> {
    /// 새로운 SCC 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: StronglyConnectedConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: StronglyConnectedConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 Strongly Connected Components 계산
    ///
    /// Tarjan 알고리즘:
    /// 1. 미방문 노드에서 DFS 시작
    /// 2. 각 노드에 인덱스와 low-link 값 할당
    /// 3. 스택에 푸시하고 자식 노드 탐색
    /// 4. low-link == index인 노드가 SCC의 루트
    /// 5. 루트에서 스택을 팝하여 SCC 구성
    pub async fn calculate(&self) -> Result<StronglyConnectedResult> {
        // 1. 그래프 구조 수집
        let (nodes, adjacency) = self.build_graph_structure().await?;

        if nodes.is_empty() {
            return Ok(StronglyConnectedResult {
                components: HashMap::new(),
                component_count: 0,
                component_sizes: HashMap::new(),
                scc_nodes: Vec::new(),
            });
        }

        let mut state = TarjanState::new();

        // 2. 모든 미방문 노드에서 DFS 실행
        for &node in &nodes {
            if !state.node_index.contains_key(&node) {
                self.strongconnect(node, &adjacency, &mut state);
            }
        }

        // 3. 결과 정리
        let mut components: HashMap<NodeId, usize> = HashMap::new();
        let mut component_sizes: HashMap<usize, usize> = HashMap::new();

        for (scc_id, scc_nodes) in state.sccs.iter().enumerate() {
            component_sizes.insert(scc_id, scc_nodes.len());
            for &node in scc_nodes {
                components.insert(node, scc_id);
            }
        }

        Ok(StronglyConnectedResult {
            components,
            component_count: state.sccs.len(),
            component_sizes,
            scc_nodes: state.sccs,
        })
    }

    /// Tarjan DFS 재귀 (스택 기반으로 변환하여 깊은 재귀 방지)
    fn strongconnect(
        &self,
        start: NodeId,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        state: &mut TarjanState,
    ) {
        // 반복적 DFS를 위한 스택 (재귀 깊이 문제 방지)
        // (node, neighbor_index, phase)
        // phase 0: 초기화, phase 1: 자식 처리 중, phase 2: 완료
        let mut call_stack: Vec<(NodeId, usize, u8)> = vec![(start, 0, 0)];

        while let Some((node, neighbor_idx, phase)) = call_stack.pop() {
            match phase {
                0 => {
                    // 초기화 단계
                    state.node_index.insert(node, state.index);
                    state.low_link.insert(node, state.index);
                    state.index += 1;
                    state.stack.push(node);
                    state.on_stack.insert(node, true);

                    // 자식 처리 단계로 전환
                    call_stack.push((node, 0, 1));
                }
                1 => {
                    // 자식 처리 단계
                    let neighbors = adjacency.get(&node).cloned().unwrap_or_default();

                    if neighbor_idx < neighbors.len() {
                        let neighbor = neighbors[neighbor_idx];

                        if !state.node_index.contains_key(&neighbor) {
                            // 미방문 노드 - 재귀 호출
                            call_stack.push((node, neighbor_idx + 1, 1));
                            call_stack.push((neighbor, 0, 0));
                        } else if *state.on_stack.get(&neighbor).unwrap_or(&false) {
                            // 스택에 있는 노드 - low_link 업데이트
                            let neighbor_index = *state.node_index.get(&neighbor).unwrap_or(&0);
                            let current_low = *state.low_link.get(&node).unwrap_or(&0);
                            state.low_link.insert(node, current_low.min(neighbor_index));
                            call_stack.push((node, neighbor_idx + 1, 1));
                        } else {
                            // 이미 처리된 노드 - 다음 이웃으로
                            call_stack.push((node, neighbor_idx + 1, 1));
                        }
                    } else {
                        // 모든 자식 처리 완료 - 완료 단계로
                        call_stack.push((node, 0, 2));
                    }
                }
                2 => {
                    // 완료 단계 - 부모의 low_link 업데이트 및 SCC 확인

                    // 부모 노드가 있으면 low_link 전파
                    if let Some(&(parent, parent_idx, _)) = call_stack.last() {
                        let neighbors = adjacency.get(&parent).cloned().unwrap_or_default();
                        if parent_idx > 0 && parent_idx <= neighbors.len() {
                            let child_low = *state.low_link.get(&node).unwrap_or(&0);
                            let parent_low = *state.low_link.get(&parent).unwrap_or(&0);
                            state.low_link.insert(parent, parent_low.min(child_low));
                        }
                    }

                    // SCC 루트인지 확인
                    let node_index = *state.node_index.get(&node).unwrap_or(&0);
                    let low_link = *state.low_link.get(&node).unwrap_or(&0);

                    if low_link == node_index {
                        // 이 노드가 SCC의 루트
                        let mut scc = Vec::new();
                        while let Some(w) = state.stack.pop() {
                            state.on_stack.insert(w, false);
                            scc.push(w);
                            if w == node {
                                break;
                            }
                        }
                        state.sccs.push(scc);
                    }
                }
                _ => {}
            }
        }
    }

    /// 그래프 구조 빌드 (노드 목록, 인접 리스트)
    async fn build_graph_structure(&self) -> Result<(Vec<NodeId>, HashMap<NodeId, Vec<NodeId>>)> {
        let mut nodes: Vec<NodeId> = Vec::new();
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                adjacency.insert(node_id, Vec::new());
            }
        }

        // 엣지 수집 (방향 그래프 - 방향 유지)
        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.rel_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // SCC는 방향 그래프 - 단방향만 추가
                adjacency.entry(from).or_default().push(to);
            }
        }

        Ok((nodes, adjacency))
    }
}
