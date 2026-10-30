//! 병렬 연결 컴포넌트 알고리즘
//!
//! Rayon을 사용하여 연결 컴포넌트 탐지를 병렬화합니다.
//! 노드별 BFS/DFS를 병렬로 실행하고 결과를 병합합니다.

use parking_lot::RwLock;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::config::ParallelConfig;
use crate::algorithm::community::ConnectedComponentsConfig;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;

/// 병렬 Connected Components 결과
#[derive(Debug, Clone)]
pub struct ParallelConnectedResult {
    /// 노드별 컴포넌트 ID
    pub components: HashMap<NodeId, usize>,
    /// 컴포넌트 수
    pub component_count: usize,
    /// 컴포넌트별 노드 수
    pub component_sizes: HashMap<usize, usize>,
}

impl ParallelConnectedResult {
    /// 특정 노드의 컴포넌트 ID
    pub fn component_id(&self, node_id: NodeId) -> Option<usize> {
        self.components.get(&node_id).copied()
    }

    /// 가장 큰 컴포넌트
    pub fn largest_component(&self) -> Option<(usize, usize)> {
        self.component_sizes
            .iter()
            .max_by_key(|&(_, &size)| size)
            .map(|(&id, &size)| (id, size))
    }

    /// 두 노드가 같은 컴포넌트에 있는지 확인
    pub fn same_component(&self, node1: NodeId, node2: NodeId) -> bool {
        match (self.component_id(node1), self.component_id(node2)) {
            (Some(c1), Some(c2)) => c1 == c2,
            _ => false,
        }
    }
}

/// 병렬 Union-Find (Concurrent)
struct ParallelUnionFind {
    parent: RwLock<HashMap<NodeId, NodeId>>,
    rank: RwLock<HashMap<NodeId, usize>>,
}

impl ParallelUnionFind {
    fn new() -> Self {
        Self {
            parent: RwLock::new(HashMap::new()),
            rank: RwLock::new(HashMap::new()),
        }
    }

    fn make_set(&self, node: NodeId) {
        let mut parent = self.parent.write();
        let mut rank = self.rank.write();
        if let std::collections::hash_map::Entry::Vacant(e) = parent.entry(node) {
            e.insert(node);
            rank.insert(node, 0);
        }
    }

    fn find(&self, node: NodeId) -> NodeId {
        // 읽기 락으로 먼저 시도
        {
            let parent = self.parent.read();
            let p = *parent.get(&node).unwrap_or(&node);
            if p == node {
                return node;
            }
        }

        // 경로 압축 필요 - 쓰기 락
        let parent = self.parent.write();
        let p = *parent.get(&node).unwrap_or(&node);
        if p == node {
            return node;
        }

        // 재귀적으로 루트 찾기 (락 해제 후)
        drop(parent);
        let root = self.find(p);

        // 경로 압축
        let mut parent = self.parent.write();
        parent.insert(node, root);
        root
    }

    fn union(&self, x: NodeId, y: NodeId) {
        let root_x = self.find(x);
        let root_y = self.find(y);

        if root_x == root_y {
            return;
        }

        let mut parent = self.parent.write();
        let mut rank = self.rank.write();

        let rank_x = *rank.get(&root_x).unwrap_or(&0);
        let rank_y = *rank.get(&root_y).unwrap_or(&0);

        if rank_x < rank_y {
            parent.insert(root_x, root_y);
        } else if rank_x > rank_y {
            parent.insert(root_y, root_x);
        } else {
            parent.insert(root_y, root_x);
            rank.insert(root_x, rank_x + 1);
        }
    }
}

/// 병렬 Connected Components 계산기
pub struct ParallelConnectedComponents<'a> {
    engine: &'a GraphEngine,
    config: ConnectedComponentsConfig,
    par_config: ParallelConfig,
}

impl<'a> ParallelConnectedComponents<'a> {
    /// 새로운 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ConnectedComponentsConfig::default(),
            par_config: ParallelConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: ConnectedComponentsConfig) -> Self {
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

    /// 병렬 연결 컴포넌트 계산
    pub async fn calculate(&self) -> Result<ParallelConnectedResult> {
        connected_components_parallel(self.engine, &self.config, &self.par_config).await
    }
}

/// 병렬 연결 컴포넌트 계산 함수
pub async fn connected_components_parallel(
    engine: &GraphEngine,
    config: &ConnectedComponentsConfig,
    par_config: &ParallelConfig,
) -> Result<ParallelConnectedResult> {
    // 1. 노드와 엣지 수집
    let (nodes, edges) = collect_graph_data(engine, config).await?;

    if nodes.is_empty() {
        return Ok(ParallelConnectedResult {
            components: HashMap::new(),
            component_count: 0,
            component_sizes: HashMap::new(),
        });
    }

    let n = nodes.len();
    let uf = ParallelUnionFind::new();

    // 2. 모든 노드 초기화
    for &node in &nodes {
        uf.make_set(node);
    }

    // 3. 엣지 처리 (병렬 vs 순차)
    if par_config.should_parallelize(n) {
        edges.par_iter().for_each(|&(from, to)| {
            uf.union(from, to);
        });
    } else {
        for &(from, to) in &edges {
            uf.union(from, to);
        }
    }

    // 4. 컴포넌트 할당
    let component_id = AtomicUsize::new(0);
    let root_to_component: RwLock<HashMap<NodeId, usize>> = RwLock::new(HashMap::new());

    let components: HashMap<NodeId, usize> = if par_config.should_parallelize(n) {
        nodes
            .par_iter()
            .map(|&node| {
                let root = uf.find(node);

                // 루트의 컴포넌트 ID 확인/할당
                let comp_id = {
                    let map = root_to_component.read();
                    if let Some(&id) = map.get(&root) {
                        id
                    } else {
                        drop(map);
                        let mut map = root_to_component.write();
                        *map.entry(root)
                            .or_insert_with(|| component_id.fetch_add(1, Ordering::SeqCst))
                    }
                };

                (node, comp_id)
            })
            .collect()
    } else {
        let mut map: HashMap<NodeId, usize> = HashMap::new();
        let mut root_map: HashMap<NodeId, usize> = HashMap::new();
        let mut next_id = 0;

        for &node in &nodes {
            let root = uf.find(node);
            let comp_id = *root_map.entry(root).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
            map.insert(node, comp_id);
        }
        map
    };

    // 5. 컴포넌트별 크기 계산
    let mut component_sizes: HashMap<usize, usize> = HashMap::new();
    for &comp in components.values() {
        *component_sizes.entry(comp).or_insert(0) += 1;
    }

    let component_count = component_sizes.len();

    Ok(ParallelConnectedResult {
        components,
        component_count,
        component_sizes,
    })
}

/// 그래프 데이터 수집
async fn collect_graph_data(
    engine: &GraphEngine,
    config: &ConnectedComponentsConfig,
) -> Result<(Vec<NodeId>, Vec<(NodeId, NodeId)>)> {
    let mut nodes: Vec<NodeId> = Vec::new();
    let mut edges: Vec<(NodeId, NodeId)> = Vec::new();

    // 노드 수집
    let node_count = engine.node_count().await?;
    for node_id in 1..=node_count as u64 {
        if engine.get_node(node_id).await?.is_some() {
            nodes.push(node_id);
        }
    }

    // 엣지 수집
    let edge_count = engine.edge_count().await;
    for edge_id in 1..=edge_count as u64 {
        if let Some(edge) = engine.get_edge(edge_id).await? {
            // 관계 타입 필터링
            if let Some(ref types) = config.rel_types
                && !types.contains(&edge.relation_type().to_string())
            {
                continue;
            }

            edges.push((edge.from_node(), edge.to_node()));
        }
    }

    Ok((nodes, edges))
}
