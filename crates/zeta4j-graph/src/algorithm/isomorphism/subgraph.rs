//! 서브그래프 동형 (Subgraph Isomorphism)
//!
//! VF2 기반 서브그래프 매칭 알고리즘
//!
//! 시간복잡도: O(n! × m) 최악, 패턴 크기에 따라 실제 성능 결정

use super::IsomorphismConfig;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 서브그래프 매칭기
pub struct SubgraphMatcher<'a> {
    pattern: &'a GraphEngine,
    target: &'a GraphEngine,
    config: IsomorphismConfig,
}

impl<'a> SubgraphMatcher<'a> {
    /// 새로운 SubgraphMatcher 생성
    pub fn new(pattern: &'a GraphEngine, target: &'a GraphEngine) -> Self {
        Self {
            pattern,
            target,
            config: IsomorphismConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(
        pattern: &'a GraphEngine,
        target: &'a GraphEngine,
        config: IsomorphismConfig,
    ) -> Self {
        Self {
            pattern,
            target,
            config,
        }
    }

    /// 패턴이 타겟의 서브그래프와 동형인지 확인
    pub async fn is_subgraph_isomorphic(&self) -> Result<bool> {
        let pattern_adj = self.build_adjacency(self.pattern).await?;
        let target_adj = self.build_adjacency(self.target).await?;

        // 패턴이 타겟보다 크면 서브그래프 불가
        if pattern_adj.len() > target_adj.len() {
            return Ok(false);
        }

        // 패턴 엣지가 타겟 엣지보다 많으면 불가
        let pattern_edges = self.count_edges(&pattern_adj);
        let target_edges = self.count_edges(&target_adj);
        if pattern_edges > target_edges {
            return Ok(false);
        }

        // 패턴이 비어있으면 항상 서브그래프
        if pattern_adj.is_empty() {
            return Ok(true);
        }

        let pattern_nodes: Vec<NodeId> = pattern_adj.keys().copied().collect();
        let target_nodes: Vec<NodeId> = target_adj.keys().copied().collect();

        let mut state = SubgraphState::new(&pattern_nodes, &target_nodes);
        Ok(self.vf2_subgraph_match(&pattern_adj, &target_adj, &mut state))
    }

    /// 서브그래프 매핑 찾기
    pub async fn find_subgraph(&self) -> Result<Option<HashMap<NodeId, NodeId>>> {
        let pattern_adj = self.build_adjacency(self.pattern).await?;
        let target_adj = self.build_adjacency(self.target).await?;

        if pattern_adj.len() > target_adj.len() {
            return Ok(None);
        }

        if pattern_adj.is_empty() {
            return Ok(Some(HashMap::new()));
        }

        let pattern_nodes: Vec<NodeId> = pattern_adj.keys().copied().collect();
        let target_nodes: Vec<NodeId> = target_adj.keys().copied().collect();

        let mut state = SubgraphState::new(&pattern_nodes, &target_nodes);
        let mut results = Vec::new();

        self.vf2_subgraph_find(&pattern_adj, &target_adj, &mut state, &mut results, Some(1));

        Ok(results.into_iter().next())
    }

    /// 모든 서브그래프 매핑 찾기
    pub async fn find_all_subgraphs(&self) -> Result<Vec<HashMap<NodeId, NodeId>>> {
        let pattern_adj = self.build_adjacency(self.pattern).await?;
        let target_adj = self.build_adjacency(self.target).await?;

        if pattern_adj.len() > target_adj.len() {
            return Ok(Vec::new());
        }

        if pattern_adj.is_empty() {
            return Ok(vec![HashMap::new()]);
        }

        let pattern_nodes: Vec<NodeId> = pattern_adj.keys().copied().collect();
        let target_nodes: Vec<NodeId> = target_adj.keys().copied().collect();

        let mut state = SubgraphState::new(&pattern_nodes, &target_nodes);
        let mut results = Vec::new();

        let max_mappings = self.config.max_mappings;
        self.vf2_subgraph_find(
            &pattern_adj,
            &target_adj,
            &mut state,
            &mut results,
            max_mappings,
        );

        Ok(results)
    }

    /// VF2 서브그래프 매칭 (존재 여부)
    fn vf2_subgraph_match(
        &self,
        pattern_adj: &HashMap<NodeId, HashSet<NodeId>>,
        target_adj: &HashMap<NodeId, HashSet<NodeId>>,
        state: &mut SubgraphState,
    ) -> bool {
        // 완전 매핑 발견 (모든 패턴 노드가 매핑됨)
        if state.mapping.len() == state.pattern_nodes.len() {
            return true;
        }

        // 후보 쌍 생성
        let candidates = self.get_candidates(pattern_adj, target_adj, state);

        for (p, t) in candidates {
            if self.is_feasible(pattern_adj, target_adj, state, p, t) {
                state.add_pair(p, t);

                if self.vf2_subgraph_match(pattern_adj, target_adj, state) {
                    return true;
                }

                state.remove_pair(p, t);
            }
        }

        false
    }

    /// VF2 서브그래프 매핑 찾기
    fn vf2_subgraph_find(
        &self,
        pattern_adj: &HashMap<NodeId, HashSet<NodeId>>,
        target_adj: &HashMap<NodeId, HashSet<NodeId>>,
        state: &mut SubgraphState,
        results: &mut Vec<HashMap<NodeId, NodeId>>,
        max_results: Option<usize>,
    ) {
        if let Some(max) = max_results
            && results.len() >= max
        {
            return;
        }

        // 완전 매핑 발견
        if state.mapping.len() == state.pattern_nodes.len() {
            results.push(state.mapping.clone());
            return;
        }

        let candidates = self.get_candidates(pattern_adj, target_adj, state);

        for (p, t) in candidates {
            if self.is_feasible(pattern_adj, target_adj, state, p, t) {
                state.add_pair(p, t);
                self.vf2_subgraph_find(pattern_adj, target_adj, state, results, max_results);
                state.remove_pair(p, t);

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
        pattern_adj: &HashMap<NodeId, HashSet<NodeId>>,
        target_adj: &HashMap<NodeId, HashSet<NodeId>>,
        state: &SubgraphState,
    ) -> Vec<(NodeId, NodeId)> {
        let mut candidates = Vec::new();

        // 패턴의 터미널 집합
        let t_pattern = self.get_terminal_set(pattern_adj, &state.mapped_pattern);
        let t_target = self.get_terminal_set(target_adj, &state.mapped_target);

        if !t_pattern.is_empty() {
            // 터미널 집합에서 최소 패턴 노드 선택
            let Some(p) = t_pattern.iter().min().copied() else {
                return candidates;
            };

            // 타겟의 터미널 집합에서 후보 생성
            if !t_target.is_empty() {
                for &t in &t_target {
                    candidates.push((p, t));
                }
            } else {
                // 타겟 터미널이 비어있으면 모든 미매핑 타겟 노드
                for &t in &state.target_nodes {
                    if !state.mapped_target.contains(&t) {
                        candidates.push((p, t));
                    }
                }
            }
        } else {
            // 터미널 집합이 비어있으면 미매핑 패턴 노드에서 선택
            for &p in &state.pattern_nodes {
                if !state.mapped_pattern.contains(&p) {
                    for &t in &state.target_nodes {
                        if !state.mapped_target.contains(&t) {
                            candidates.push((p, t));
                        }
                    }
                    break; // 첫 번째 미매핑 패턴 노드만
                }
            }
        }

        candidates
    }

    /// 터미널 집합 계산
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

    /// 가능성 검사 (서브그래프 버전)
    fn is_feasible(
        &self,
        pattern_adj: &HashMap<NodeId, HashSet<NodeId>>,
        target_adj: &HashMap<NodeId, HashSet<NodeId>>,
        state: &SubgraphState,
        p: NodeId,
        t: NodeId,
    ) -> bool {
        // 이미 매핑된 노드
        if state.mapped_pattern.contains(&p) || state.mapped_target.contains(&t) {
            return false;
        }

        let p_neighbors = pattern_adj.get(&p).cloned().unwrap_or_default();
        let t_neighbors = target_adj.get(&t).cloned().unwrap_or_default();

        // 패턴 노드의 차수가 타겟 노드 차수보다 크면 불가
        if p_neighbors.len() > t_neighbors.len() {
            return false;
        }

        // 구문적 일관성: 패턴의 매핑된 이웃은 타겟에서도 이웃이어야 함
        for (&mp, &mt) in &state.mapping {
            if p_neighbors.contains(&mp) && !t_neighbors.contains(&mt) {
                return false;
            }
        }

        // Look-ahead: 패턴의 미매핑 이웃 수 ≤ 타겟의 미매핑 이웃 수
        let p_unmapped_neighbors: usize = p_neighbors
            .iter()
            .filter(|n| !state.mapped_pattern.contains(n))
            .count();
        let t_unmapped_neighbors: usize = t_neighbors
            .iter()
            .filter(|n| !state.mapped_target.contains(n))
            .count();

        if p_unmapped_neighbors > t_unmapped_neighbors {
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

/// 서브그래프 상태
struct SubgraphState {
    pattern_nodes: Vec<NodeId>,
    target_nodes: Vec<NodeId>,
    mapping: HashMap<NodeId, NodeId>,
    mapped_pattern: HashSet<NodeId>,
    mapped_target: HashSet<NodeId>,
}

impl SubgraphState {
    fn new(pattern_nodes: &[NodeId], target_nodes: &[NodeId]) -> Self {
        Self {
            pattern_nodes: pattern_nodes.to_vec(),
            target_nodes: target_nodes.to_vec(),
            mapping: HashMap::new(),
            mapped_pattern: HashSet::new(),
            mapped_target: HashSet::new(),
        }
    }

    fn add_pair(&mut self, p: NodeId, t: NodeId) {
        self.mapping.insert(p, t);
        self.mapped_pattern.insert(p);
        self.mapped_target.insert(t);
    }

    fn remove_pair(&mut self, p: NodeId, t: NodeId) {
        self.mapping.remove(&p);
        self.mapped_pattern.remove(&p);
        self.mapped_target.remove(&t);
    }
}

/// 패턴이 타겟의 서브그래프와 동형인지 확인
pub async fn is_subgraph_isomorphic(pattern: &GraphEngine, target: &GraphEngine) -> Result<bool> {
    SubgraphMatcher::new(pattern, target)
        .is_subgraph_isomorphic()
        .await
}

/// 서브그래프 매핑 찾기
pub async fn find_subgraph(
    pattern: &GraphEngine,
    target: &GraphEngine,
) -> Result<Option<HashMap<NodeId, NodeId>>> {
    SubgraphMatcher::new(pattern, target).find_subgraph().await
}

/// 모든 서브그래프 매핑 찾기
pub async fn find_all_subgraphs(
    pattern: &GraphEngine,
    target: &GraphEngine,
) -> Result<Vec<HashMap<NodeId, NodeId>>> {
    SubgraphMatcher::new(pattern, target)
        .find_all_subgraphs()
        .await
}
