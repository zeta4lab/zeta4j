//! Weakly Connected Components (WCC) 알고리즘
//!
//! 무방향 그래프에서 연결된 컴포넌트를 찾습니다.
//! Union-Find (Disjoint Set Union) 자료구조를 사용하여 효율적으로 구현합니다.
//!
//! 시간 복잡도: O(V + E * α(V)) where α is inverse Ackermann function (거의 상수)

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Union-Find 자료구조 (경로 압축 + 랭크 기반 합치기)
struct UnionFind {
    parent: HashMap<NodeId, NodeId>,
    rank: HashMap<NodeId, usize>,
}

impl UnionFind {
    /// 새로운 Union-Find 생성
    fn new() -> Self {
        Self {
            parent: HashMap::new(),
            rank: HashMap::new(),
        }
    }

    /// 노드 추가 (각 노드는 처음에 자기 자신이 부모)
    fn make_set(&mut self, node: NodeId) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.parent.entry(node) {
            e.insert(node);
            self.rank.insert(node, 0);
        }
    }

    /// 루트 노드 찾기 (경로 압축 적용)
    fn find(&mut self, node: NodeId) -> NodeId {
        let parent = *self.parent.get(&node).unwrap_or(&node);
        if parent != node {
            let root = self.find(parent);
            self.parent.insert(node, root);
            root
        } else {
            node
        }
    }

    /// 두 집합 합치기 (랭크 기반)
    fn union(&mut self, x: NodeId, y: NodeId) {
        let root_x = self.find(x);
        let root_y = self.find(y);

        if root_x == root_y {
            return;
        }

        let rank_x = *self.rank.get(&root_x).unwrap_or(&0);
        let rank_y = *self.rank.get(&root_y).unwrap_or(&0);

        if rank_x < rank_y {
            self.parent.insert(root_x, root_y);
        } else if rank_x > rank_y {
            self.parent.insert(root_y, root_x);
        } else {
            self.parent.insert(root_y, root_x);
            self.rank.insert(root_x, rank_x + 1);
        }
    }
}

/// Connected Components 설정
#[derive(Debug, Clone, Default)]
pub struct ConnectedComponentsConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub rel_types: Option<Vec<String>>,
}

impl ConnectedComponentsConfig {
    /// 관계 타입 필터 설정
    pub fn with_rel_types(mut self, rel_types: Vec<String>) -> Self {
        self.rel_types = Some(rel_types);
        self
    }
}

/// Connected Components 결과
#[derive(Debug, Clone)]
pub struct ConnectedComponentsResult {
    /// 노드별 컴포넌트 ID
    pub components: HashMap<NodeId, usize>,
    /// 컴포넌트 수
    pub component_count: usize,
    /// 컴포넌트별 노드 수
    pub component_sizes: HashMap<usize, usize>,
}

impl ConnectedComponentsResult {
    /// 특정 노드의 컴포넌트 ID 조회
    pub fn component_id(&self, node_id: NodeId) -> Option<usize> {
        self.components.get(&node_id).copied()
    }

    /// 가장 큰 컴포넌트의 ID와 크기 반환
    pub fn largest_component(&self) -> Option<(usize, usize)> {
        self.component_sizes
            .iter()
            .max_by_key(|&(_, &size)| size)
            .map(|(&id, &size)| (id, size))
    }

    /// 특정 컴포넌트에 속한 노드들 반환
    pub fn nodes_in_component(&self, component_id: usize) -> Vec<NodeId> {
        self.components
            .iter()
            .filter(|&(_, &cid)| cid == component_id)
            .map(|(&node_id, _)| node_id)
            .collect()
    }
}

/// Weakly Connected Components 계산기
pub struct ConnectedComponents<'a> {
    engine: &'a GraphEngine,
    config: ConnectedComponentsConfig,
}

impl<'a> ConnectedComponents<'a> {
    /// 새로운 Connected Components 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ConnectedComponentsConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: ConnectedComponentsConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 Connected Components 계산
    ///
    /// Union-Find 알고리즘 사용:
    /// 1. 각 노드를 개별 집합으로 초기화
    /// 2. 모든 엣지에 대해 양 끝 노드 union
    /// 3. 각 노드의 루트를 찾아 컴포넌트 ID 할당
    pub async fn calculate(&self) -> Result<ConnectedComponentsResult> {
        let mut uf = UnionFind::new();

        // 1. 모든 노드를 개별 집합으로 초기화
        let node_count = self.engine.node_count().await?;
        let mut nodes: Vec<NodeId> = Vec::new();

        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                uf.make_set(node_id);
                nodes.push(node_id);
            }
        }

        if nodes.is_empty() {
            return Ok(ConnectedComponentsResult {
                components: HashMap::new(),
                component_count: 0,
                component_sizes: HashMap::new(),
            });
        }

        // 2. 모든 엣지에 대해 union 수행
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

                // WCC는 방향 무시 - 양방향으로 union
                uf.union(from, to);
            }
        }

        // 3. 각 노드의 루트를 찾아 컴포넌트 ID 할당
        let mut root_to_id: HashMap<NodeId, usize> = HashMap::new();
        let mut components: HashMap<NodeId, usize> = HashMap::new();
        let mut current_id = 0;

        for &node in &nodes {
            let root = uf.find(node);
            let component_id = *root_to_id.entry(root).or_insert_with(|| {
                let id = current_id;
                current_id += 1;
                id
            });
            components.insert(node, component_id);
        }

        // 4. 컴포넌트별 크기 계산
        let mut component_sizes: HashMap<usize, usize> = HashMap::new();
        for &component_id in components.values() {
            *component_sizes.entry(component_id).or_insert(0) += 1;
        }

        Ok(ConnectedComponentsResult {
            components,
            component_count: current_id,
            component_sizes,
        })
    }
}
