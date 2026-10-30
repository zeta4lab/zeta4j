//! 트리 분해 핵심 구조체 및 알고리즘

use super::heuristic::{Heuristic, min_degree_ordering, min_fill_ordering};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 트리 분해의 백 (노드)
#[derive(Debug, Clone)]
pub struct Bag {
    /// 백 ID
    pub id: usize,
    /// 백에 포함된 정점들
    pub vertices: HashSet<NodeId>,
}

impl Bag {
    /// 새 백 생성
    pub fn new(id: usize) -> Self {
        Self {
            id,
            vertices: HashSet::new(),
        }
    }

    /// 정점들로 백 생성
    pub fn with_vertices(id: usize, vertices: HashSet<NodeId>) -> Self {
        Self { id, vertices }
    }

    /// 백의 크기 (정점 수)
    pub fn size(&self) -> usize {
        self.vertices.len()
    }

    /// 정점 포함 여부
    pub fn contains(&self, v: NodeId) -> bool {
        self.vertices.contains(&v)
    }
}

/// 트리 분해 설정
#[derive(Debug, Clone)]
pub struct TreeDecompositionConfig {
    /// 사용할 휴리스틱 알고리즘
    pub heuristic: Heuristic,
    /// 특정 관계 타입만 고려
    pub relationship_types: Option<Vec<String>>,
}

impl Default for TreeDecompositionConfig {
    fn default() -> Self {
        Self {
            heuristic: Heuristic::MinDegree,
            relationship_types: None,
        }
    }
}

impl TreeDecompositionConfig {
    /// 휴리스틱 설정
    pub fn with_heuristic(mut self, heuristic: Heuristic) -> Self {
        self.heuristic = heuristic;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 트리 분해 결과
#[derive(Debug, Clone)]
pub struct TreeDecompositionResult {
    /// 백들의 목록
    pub bags: Vec<Bag>,
    /// 백 간의 연결 (트리 엣지)
    pub tree_edges: Vec<(usize, usize)>,
    /// 트리 너비 (가장 큰 백의 크기 - 1)
    pub width: usize,
    /// 제거 순서
    pub elimination_ordering: Vec<NodeId>,
}

impl TreeDecompositionResult {
    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            bags: Vec::new(),
            tree_edges: Vec::new(),
            width: 0,
            elimination_ordering: Vec::new(),
        }
    }

    /// 백 수
    pub fn bag_count(&self) -> usize {
        self.bags.len()
    }

    /// 가장 큰 백 찾기
    pub fn largest_bag(&self) -> Option<&Bag> {
        self.bags.iter().max_by_key(|b| b.size())
    }

    /// 정점이 속한 백들 찾기
    pub fn bags_containing(&self, v: NodeId) -> Vec<&Bag> {
        self.bags.iter().filter(|b| b.contains(v)).collect()
    }
}

/// 트리 분해 계산 구조체
pub struct TreeDecomposition {
    config: TreeDecompositionConfig,
}

impl TreeDecomposition {
    /// 새 트리 분해 계산기 생성
    pub fn new() -> Self {
        Self {
            config: TreeDecompositionConfig::default(),
        }
    }

    /// 설정으로 생성
    pub fn with_config(config: TreeDecompositionConfig) -> Self {
        Self { config }
    }

    /// 트리 분해 계산
    pub async fn decompose(&self, engine: &GraphEngine) -> Result<TreeDecompositionResult> {
        // 그래프 정보 수집
        let (vertices, adj) = self.build_adjacency(engine).await?;

        if vertices.is_empty() {
            return Ok(TreeDecompositionResult::empty());
        }

        // 제거 순서 계산
        let ordering = match self.config.heuristic {
            Heuristic::MinDegree => min_degree_ordering(&vertices, &adj),
            Heuristic::MinFill => min_fill_ordering(&vertices, &adj),
        };

        // 제거 순서로부터 트리 분해 구성
        self.build_decomposition(&vertices, &adj, &ordering)
    }

    /// 인접 리스트 구성
    async fn build_adjacency(
        &self,
        engine: &GraphEngine,
    ) -> Result<(Vec<NodeId>, HashMap<NodeId, HashSet<NodeId>>)> {
        let mut vertices: Vec<NodeId> = Vec::new();
        let mut adj: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

        // 모든 노드 수집
        let node_count = engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if engine.get_node(node_id).await?.is_some() {
                vertices.push(node_id);
                adj.insert(node_id, HashSet::new());
            }
        }

        // 모든 엣지 수집
        let edge_count = engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = engine.get_edge(edge_id).await? {
                // 관계 타입 필터
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // 무방향 그래프로 처리
                adj.entry(from).or_default().insert(to);
                adj.entry(to).or_default().insert(from);
            }
        }

        Ok((vertices, adj))
    }

    /// 제거 순서로부터 트리 분해 구성
    #[expect(clippy::needless_range_loop)]
    fn build_decomposition(
        &self,
        vertices: &[NodeId],
        original_adj: &HashMap<NodeId, HashSet<NodeId>>,
        ordering: &[NodeId],
    ) -> Result<TreeDecompositionResult> {
        if vertices.is_empty() {
            return Ok(TreeDecompositionResult::empty());
        }

        // 작업용 인접 리스트 복사
        let mut adj = original_adj.clone();
        let mut bags: Vec<Bag> = Vec::new();
        let mut vertex_to_bag: HashMap<NodeId, usize> = HashMap::new();
        let mut tree_edges: Vec<(usize, usize)> = Vec::new();
        let mut width: usize = 0;

        // 제거 순서대로 백 생성
        for (i, &v) in ordering.iter().enumerate() {
            // 현재 정점과 이웃들로 백 구성
            let neighbors: HashSet<NodeId> = adj
                .get(&v)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|&n| !ordering[..i].contains(&n)) // 아직 제거되지 않은 이웃만
                .collect();

            let mut bag_vertices: HashSet<NodeId> = neighbors.clone();
            bag_vertices.insert(v);

            let bag = Bag::with_vertices(i, bag_vertices.clone());
            width = width.max(bag.size().saturating_sub(1));

            bags.push(bag);
            vertex_to_bag.insert(v, i);

            // 이웃들을 클릭으로 연결 (fill edges)
            let neighbor_list: Vec<NodeId> = neighbors.iter().copied().collect();
            for j in 0..neighbor_list.len() {
                for k in (j + 1)..neighbor_list.len() {
                    let u = neighbor_list[j];
                    let w = neighbor_list[k];
                    adj.entry(u).or_default().insert(w);
                    adj.entry(w).or_default().insert(u);
                }
            }
        }

        // 트리 엣지 구성: 각 백을 적절한 부모 백과 연결
        // 정점 v의 백과 v를 포함하는 다음 백을 연결
        for i in 0..ordering.len() {
            let v = ordering[i];

            // v가 포함된 다음 백 찾기 (제거 순서에서 나중에 오는)
            for j in (i + 1)..ordering.len() {
                if bags[j].vertices.contains(&v) {
                    tree_edges.push((i, j));
                    break;
                }
            }
        }

        // 중복 제거 및 연결된 트리 보장
        let tree_edges = self.build_tree_structure(&bags, tree_edges);

        Ok(TreeDecompositionResult {
            bags,
            tree_edges,
            width,
            elimination_ordering: ordering.to_vec(),
        })
    }

    /// 트리 구조 구성 (연결 보장)
    fn build_tree_structure(
        &self,
        bags: &[Bag],
        mut edges: Vec<(usize, usize)>,
    ) -> Vec<(usize, usize)> {
        if bags.is_empty() {
            return vec![];
        }

        // 중복 제거
        edges.sort();
        edges.dedup();

        // 간단한 접근: 순차적으로 연결
        // 각 백을 다음 백과 연결 (공유 정점이 있는 경우)
        let mut result: Vec<(usize, usize)> = Vec::new();
        let mut connected: HashSet<usize> = HashSet::new();
        connected.insert(0);

        for i in 1..bags.len() {
            // 이미 연결된 백 중 공유 정점이 있는 백 찾기
            let mut best_parent: Option<usize> = None;
            let mut best_overlap = 0;

            for &parent in &connected {
                let overlap = bags[i]
                    .vertices
                    .intersection(&bags[parent].vertices)
                    .count();
                if overlap > best_overlap {
                    best_overlap = overlap;
                    best_parent = Some(parent);
                }
            }

            if let Some(parent) = best_parent {
                result.push((parent, i));
            } else if !connected.is_empty() {
                // 공유 정점이 없으면 마지막 연결된 백과 연결
                let last = *connected.iter().max().unwrap_or(&0);
                result.push((last, i));
            }

            connected.insert(i);
        }

        result
    }
}

impl Default for TreeDecomposition {
    fn default() -> Self {
        Self::new()
    }
}

/// 트리 분해 계산 헬퍼 함수
pub async fn tree_decomposition(
    engine: &GraphEngine,
    heuristic: Heuristic,
) -> Result<TreeDecompositionResult> {
    let config = TreeDecompositionConfig::default().with_heuristic(heuristic);
    TreeDecomposition::with_config(config)
        .decompose(engine)
        .await
}

/// 트리 너비만 계산
pub async fn treewidth(engine: &GraphEngine) -> Result<usize> {
    let result = tree_decomposition(engine, Heuristic::MinDegree).await?;
    Ok(result.width)
}
