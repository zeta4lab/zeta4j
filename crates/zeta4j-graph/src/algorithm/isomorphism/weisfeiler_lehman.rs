//! Weisfeiler-Lehman 알고리즘
//!
//! 그래프의 canonical hash를 계산하여 빠른 동형 검사 수행
//!
//! 시간복잡도: O(k × m) where k = 반복 횟수, m = 엣지 수

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// Weisfeiler-Lehman 설정
#[derive(Debug, Clone)]
pub struct WLConfig {
    /// 반복 횟수 (기본: 3)
    pub iterations: usize,
    /// 특정 관계 타입만 고려
    pub relationship_types: Option<Vec<String>>,
}

impl Default for WLConfig {
    fn default() -> Self {
        Self {
            iterations: 3,
            relationship_types: None,
        }
    }
}

impl WLConfig {
    /// 반복 횟수 설정
    pub fn with_iterations(mut self, k: usize) -> Self {
        self.iterations = k;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Weisfeiler-Lehman 해시 계산기
pub struct WeisfeilerLehman<'a> {
    engine: &'a GraphEngine,
    config: WLConfig,
}

impl<'a> WeisfeilerLehman<'a> {
    /// 새로운 WeisfeilerLehman 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: WLConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: WLConfig) -> Self {
        Self { engine, config }
    }

    /// WL 해시 계산
    pub async fn compute_hash(&self) -> Result<String> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok("empty".to_string());
        }

        // 초기 색상: 모든 노드에 차수 기반 초기 색상
        let mut colors: HashMap<NodeId, u64> = adjacency
            .iter()
            .map(|(&node, neighbors)| (node, neighbors.len() as u64))
            .collect();

        // k번 반복
        for _ in 0..self.config.iterations {
            colors = self.refine_colors(&adjacency, &colors);
        }

        // 최종 해시: 색상 분포의 canonical form
        self.compute_graph_hash(&colors)
    }

    /// 노드별 색상 반환
    pub async fn compute_node_colors(&self) -> Result<HashMap<NodeId, u64>> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(HashMap::new());
        }

        let mut colors: HashMap<NodeId, u64> = adjacency
            .iter()
            .map(|(&node, neighbors)| (node, neighbors.len() as u64))
            .collect();

        for _ in 0..self.config.iterations {
            colors = self.refine_colors(&adjacency, &colors);
        }

        Ok(colors)
    }

    /// 색상 정제 (한 단계)
    fn refine_colors(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        colors: &HashMap<NodeId, u64>,
    ) -> HashMap<NodeId, u64> {
        let mut new_colors: HashMap<NodeId, u64> = HashMap::new();
        let mut label_map: HashMap<(u64, Vec<u64>), u64> = HashMap::new();
        let mut next_label: u64 = 0;

        for (&node, neighbors) in adjacency {
            let current_color = colors.get(&node).copied().unwrap_or(0);

            // 이웃 색상을 정렬하여 multiset 생성
            let mut neighbor_colors: Vec<u64> = neighbors
                .iter()
                .filter_map(|n| colors.get(n).copied())
                .collect();
            neighbor_colors.sort();

            // (현재 색상, 이웃 색상들)을 새 색상으로 매핑
            let key = (current_color, neighbor_colors);

            let new_color = *label_map.entry(key).or_insert_with(|| {
                let label = next_label;
                next_label += 1;
                label
            });

            new_colors.insert(node, new_color);
        }

        new_colors
    }

    /// 그래프 전체 해시 계산
    fn compute_graph_hash(&self, colors: &HashMap<NodeId, u64>) -> Result<String> {
        // 색상별 카운트를 정렬하여 canonical form 생성
        let mut color_counts: BTreeMap<u64, usize> = BTreeMap::new();

        for &color in colors.values() {
            *color_counts.entry(color).or_insert(0) += 1;
        }

        // 해시 계산
        let mut hasher = DefaultHasher::new();

        // 노드 수
        colors.len().hash(&mut hasher);

        // 색상 분포
        for (color, count) in &color_counts {
            color.hash(&mut hasher);
            count.hash(&mut hasher);
        }

        let hash = hasher.finish();
        Ok(format!("{:016x}", hash))
    }

    /// 인접 리스트 구축
    async fn build_adjacency(&self) -> Result<HashMap<NodeId, HashSet<NodeId>>> {
        let mut adjacency: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, HashSet::new());
            }
        }

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
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
}

/// WL 해시 계산 (기본 설정)
pub async fn wl_hash(engine: &GraphEngine) -> Result<String> {
    WeisfeilerLehman::new(engine).compute_hash().await
}

/// WL 해시 계산 (반복 횟수 지정)
pub async fn wl_hash_with_iterations(engine: &GraphEngine, iterations: usize) -> Result<String> {
    let config = WLConfig::default().with_iterations(iterations);
    WeisfeilerLehman::with_config(engine, config)
        .compute_hash()
        .await
}

/// 두 그래프가 잠재적으로 동형인지 빠르게 확인
///
/// WL 해시가 다르면 확실히 비동형
/// WL 해시가 같으면 동형일 가능성 있음 (확실하지 않음)
pub async fn are_potentially_isomorphic(g1: &GraphEngine, g2: &GraphEngine) -> Result<bool> {
    // 노드/엣지 수 비교
    if g1.node_count().await? != g2.node_count().await?
        || g1.edge_count().await != g2.edge_count().await
    {
        return Ok(false);
    }

    let hash1 = wl_hash(g1).await?;
    let hash2 = wl_hash(g2).await?;

    Ok(hash1 == hash2)
}
