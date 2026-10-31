// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! VF2 알고리즘
//!
//! 그래프 동형 검사를 위한 VF2 상태 공간 탐색 알고리즘
//!
//! 시간복잡도: 최악 O(n! × n), 실제 대부분 O(n²)

use super::IsomorphismConfig;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// VF2 그래프 동형 검사기
pub struct VF2<'a> {
    graph1: &'a GraphEngine,
    graph2: &'a GraphEngine,
    config: IsomorphismConfig,
}

impl<'a> VF2<'a> {
    /// 새로운 VF2 검사기 생성
    pub fn new(graph1: &'a GraphEngine, graph2: &'a GraphEngine) -> Self {
        Self {
            graph1,
            graph2,
            config: IsomorphismConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(
        graph1: &'a GraphEngine,
        graph2: &'a GraphEngine,
        config: IsomorphismConfig,
    ) -> Self {
        Self {
            graph1,
            graph2,
            config,
        }
    }

    /// 두 그래프가 동형인지 확인
    pub async fn is_isomorphic(&self) -> Result<bool> {
        let adj1 = self.build_adjacency(self.graph1).await?;
        let adj2 = self.build_adjacency(self.graph2).await?;

        // 기본 검사: 노드/엣지 수가 다르면 동형 불가
        if adj1.len() != adj2.len() {
            return Ok(false);
        }

        let edge_count1 = self.count_edges(&adj1);
        let edge_count2 = self.count_edges(&adj2);
        if edge_count1 != edge_count2 {
            return Ok(false);
        }

        // 차수 시퀀스 검사
        let mut degrees1: Vec<usize> = adj1.values().map(|v| v.len()).collect();
        let mut degrees2: Vec<usize> = adj2.values().map(|v| v.len()).collect();
        degrees1.sort();
        degrees2.sort();
        if degrees1 != degrees2 {
            return Ok(false);
        }

        // VF2 탐색
        let nodes1: Vec<NodeId> = adj1.keys().copied().collect();
        let nodes2: Vec<NodeId> = adj2.keys().copied().collect();

        let mut state = VF2State::new(&nodes1, &nodes2);
        Ok(self.vf2_match(&adj1, &adj2, &mut state))
    }

    /// 동형 매핑 찾기
    pub async fn find_isomorphism(&self) -> Result<Option<HashMap<NodeId, NodeId>>> {
        let adj1 = self.build_adjacency(self.graph1).await?;
        let adj2 = self.build_adjacency(self.graph2).await?;

        // 기본 검사
        if adj1.len() != adj2.len() {
            return Ok(None);
        }

        let edge_count1 = self.count_edges(&adj1);
        let edge_count2 = self.count_edges(&adj2);
        if edge_count1 != edge_count2 {
            return Ok(None);
        }

        let nodes1: Vec<NodeId> = adj1.keys().copied().collect();
        let nodes2: Vec<NodeId> = adj2.keys().copied().collect();

        let mut state = VF2State::new(&nodes1, &nodes2);
        let mut result = Vec::new();

        self.vf2_find(&adj1, &adj2, &mut state, &mut result, Some(1));

        Ok(result.into_iter().next())
    }

    /// 모든 동형 매핑 찾기
    pub async fn find_all_isomorphisms(&self) -> Result<Vec<HashMap<NodeId, NodeId>>> {
        let adj1 = self.build_adjacency(self.graph1).await?;
        let adj2 = self.build_adjacency(self.graph2).await?;

        // 기본 검사
        if adj1.len() != adj2.len() {
            return Ok(Vec::new());
        }

        let edge_count1 = self.count_edges(&adj1);
        let edge_count2 = self.count_edges(&adj2);
        if edge_count1 != edge_count2 {
            return Ok(Vec::new());
        }

        let nodes1: Vec<NodeId> = adj1.keys().copied().collect();
        let nodes2: Vec<NodeId> = adj2.keys().copied().collect();

        let mut state = VF2State::new(&nodes1, &nodes2);
        let mut result = Vec::new();

        let max_mappings = self.config.max_mappings;
        self.vf2_find(&adj1, &adj2, &mut state, &mut result, max_mappings);

        Ok(result)
    }

    /// VF2 매칭 (존재 여부만)
    fn vf2_match(
        &self,
        adj1: &HashMap<NodeId, HashSet<NodeId>>,
        adj2: &HashMap<NodeId, HashSet<NodeId>>,
        state: &mut VF2State,
    ) -> bool {
        // 완전 매핑 발견
        if state.mapping.len() == state.nodes1.len() {
            return true;
        }

        // 후보 쌍 생성
        let candidates = self.get_candidates(adj1, adj2, state);

        for (n1, n2) in candidates {
            if self.is_feasible(adj1, adj2, state, n1, n2) {
                // 상태 추가
                state.add_pair(n1, n2);

                if self.vf2_match(adj1, adj2, state) {
                    return true;
                }

                // 상태 복원
                state.remove_pair(n1, n2);
            }
        }

        false
    }

    /// VF2 매핑 찾기
    fn vf2_find(
        &self,
        adj1: &HashMap<NodeId, HashSet<NodeId>>,
        adj2: &HashMap<NodeId, HashSet<NodeId>>,
        state: &mut VF2State,
        results: &mut Vec<HashMap<NodeId, NodeId>>,
        max_results: Option<usize>,
    ) {
        // 최대 결과 수 도달
        if let Some(max) = max_results
            && results.len() >= max
        {
            return;
        }

        // 완전 매핑 발견
        if state.mapping.len() == state.nodes1.len() {
            results.push(state.mapping.clone());
            return;
        }

        // 후보 쌍 생성
        let candidates = self.get_candidates(adj1, adj2, state);

        for (n1, n2) in candidates {
            if self.is_feasible(adj1, adj2, state, n1, n2) {
                state.add_pair(n1, n2);
                self.vf2_find(adj1, adj2, state, results, max_results);
                state.remove_pair(n1, n2);

                if let Some(max) = max_results
                    && results.len() >= max
                {
                    return;
                }
            }
        }
    }

    /// 후보 쌍 생성
    fn get_candidates(
        &self,
        adj1: &HashMap<NodeId, HashSet<NodeId>>,
        adj2: &HashMap<NodeId, HashSet<NodeId>>,
        state: &VF2State,
    ) -> Vec<(NodeId, NodeId)> {
        let mut candidates = Vec::new();

        // T1_out, T2_out 집합 (매핑된 노드의 이웃 중 미매핑)
        let t1_out = self.get_terminal_set(adj1, &state.mapped1);
        let t2_out = self.get_terminal_set(adj2, &state.mapped2);

        if !t1_out.is_empty() && !t2_out.is_empty() {
            // T1_out에서 최소 노드 선택
            let Some(n1) = t1_out.iter().min().copied() else {
                return candidates;
            };
            for &n2 in &t2_out {
                candidates.push((n1, n2));
            }
        } else {
            // T sets가 비어있으면, 미매핑 노드에서 선택
            let unmapped1: Vec<NodeId> = state
                .nodes1
                .iter()
                .filter(|n| !state.mapped1.contains(n))
                .copied()
                .collect();
            let unmapped2: Vec<NodeId> = state
                .nodes2
                .iter()
                .filter(|n| !state.mapped2.contains(n))
                .copied()
                .collect();

            if let Some(&n1) = unmapped1.first() {
                for &n2 in &unmapped2 {
                    candidates.push((n1, n2));
                }
            }
        }

        candidates
    }

    /// Terminal set 계산 (매핑된 노드의 이웃 중 미매핑 노드)
    fn get_terminal_set(
        &self,
        adj: &HashMap<NodeId, HashSet<NodeId>>,
        mapped: &HashSet<NodeId>,
    ) -> HashSet<NodeId> {
        let mut terminal = HashSet::new();

        for &node in mapped {
            if let Some(neighbors) = adj.get(&node) {
                for &neighbor in neighbors {
                    if !mapped.contains(&neighbor) {
                        terminal.insert(neighbor);
                    }
                }
            }
        }

        terminal
    }

    /// 가능성 검사 (Feasibility)
    fn is_feasible(
        &self,
        adj1: &HashMap<NodeId, HashSet<NodeId>>,
        adj2: &HashMap<NodeId, HashSet<NodeId>>,
        state: &VF2State,
        n1: NodeId,
        n2: NodeId,
    ) -> bool {
        // 이미 매핑된 노드는 제외
        if state.mapped1.contains(&n1) || state.mapped2.contains(&n2) {
            return false;
        }

        // 구문적 일관성: 이웃 관계 보존
        let neighbors1 = adj1.get(&n1).cloned().unwrap_or_default();
        let neighbors2 = adj2.get(&n2).cloned().unwrap_or_default();

        // 매핑된 이웃 검사
        for (&m1, &m2) in &state.mapping {
            // n1의 이웃 m1이 매핑됨 → n2의 이웃에 m2가 있어야 함
            if neighbors1.contains(&m1) && !neighbors2.contains(&m2) {
                return false;
            }
            if neighbors2.contains(&m2) && !neighbors1.contains(&m1) {
                return false;
            }
        }

        // Look-ahead: 터미널 집합 크기 비교
        let t1_out = self.get_terminal_set(adj1, &state.mapped1);
        let t2_out = self.get_terminal_set(adj2, &state.mapped2);

        // n1, n2의 이웃 중 터미널 집합에 있는 것의 수
        let pred1: usize = neighbors1.iter().filter(|n| t1_out.contains(n)).count();
        let pred2: usize = neighbors2.iter().filter(|n| t2_out.contains(n)).count();
        if pred1 != pred2 {
            return false;
        }

        // 미매핑 이웃 수 검사
        let new1: usize = neighbors1
            .iter()
            .filter(|n| !state.mapped1.contains(n) && !t1_out.contains(n))
            .count();
        let new2: usize = neighbors2
            .iter()
            .filter(|n| !state.mapped2.contains(n) && !t2_out.contains(n))
            .count();
        if new1 != new2 {
            return false;
        }

        true
    }

    /// 인접 리스트 구축
    async fn build_adjacency(
        &self,
        engine: &GraphEngine,
    ) -> Result<HashMap<NodeId, HashSet<NodeId>>> {
        let mut adjacency: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

        let node_count = engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, HashSet::new());
            }
        }

        let edge_count = engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = engine.get_edge(edge_id).await? {
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from != to {
                    adjacency.entry(from).or_default().insert(to);
                    // 무방향 그래프로 처리 (방향 그래프면 이 줄 제거)
                    adjacency.entry(to).or_default().insert(from);
                }
            }
        }

        Ok(adjacency)
    }

    /// 엣지 수 계산
    fn count_edges(&self, adj: &HashMap<NodeId, HashSet<NodeId>>) -> usize {
        adj.values().map(|v| v.len()).sum::<usize>() / 2
    }
}

/// VF2 상태
struct VF2State {
    nodes1: Vec<NodeId>,
    nodes2: Vec<NodeId>,
    mapping: HashMap<NodeId, NodeId>,
    mapped1: HashSet<NodeId>,
    mapped2: HashSet<NodeId>,
}

impl VF2State {
    fn new(nodes1: &[NodeId], nodes2: &[NodeId]) -> Self {
        Self {
            nodes1: nodes1.to_vec(),
            nodes2: nodes2.to_vec(),
            mapping: HashMap::new(),
            mapped1: HashSet::new(),
            mapped2: HashSet::new(),
        }
    }

    fn add_pair(&mut self, n1: NodeId, n2: NodeId) {
        self.mapping.insert(n1, n2);
        self.mapped1.insert(n1);
        self.mapped2.insert(n2);
    }

    fn remove_pair(&mut self, n1: NodeId, n2: NodeId) {
        self.mapping.remove(&n1);
        self.mapped1.remove(&n1);
        self.mapped2.remove(&n2);
    }
}

/// 두 그래프가 동형인지 확인
pub async fn is_isomorphic(g1: &GraphEngine, g2: &GraphEngine) -> Result<bool> {
    VF2::new(g1, g2).is_isomorphic().await
}

/// 동형 매핑 찾기
pub async fn find_isomorphism(
    g1: &GraphEngine,
    g2: &GraphEngine,
) -> Result<Option<HashMap<NodeId, NodeId>>> {
    VF2::new(g1, g2).find_isomorphism().await
}

/// 모든 동형 매핑 찾기
pub async fn find_all_isomorphisms(
    g1: &GraphEngine,
    g2: &GraphEngine,
) -> Result<Vec<HashMap<NodeId, NodeId>>> {
    VF2::new(g1, g2).find_all_isomorphisms().await
}
