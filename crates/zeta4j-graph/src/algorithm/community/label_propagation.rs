//! Label Propagation Community Detection 알고리즘
//!
//! 빠르고 효율적인 커뮤니티 탐지 알고리즘입니다.
//! 각 노드가 이웃 노드들의 레이블을 기반으로 자신의 레이블을 업데이트하여
//! 자연스럽게 커뮤니티를 형성합니다.
//!
//! ## 알고리즘 개요
//! 1. 초기화: 각 노드에 고유한 레이블 할당
//! 2. 반복: 노드를 순회하며 가장 빈번한 이웃 레이블로 업데이트
//! 3. 수렴: 레이블 변경이 없을 때까지 반복
//!
//! ## 특징
//! - 빠름: O(E) 선형 시간 복잡도
//! - 파라미터 없음: 커뮤니티 수를 미리 지정할 필요 없음
//! - 비결정적: 노드 처리 순서에 따라 결과가 달라질 수 있음

use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Label Propagation 알고리즘 설정
#[derive(Debug, Clone)]
pub struct LabelPropagationConfig {
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 임계값 (변경 비율, 0.0이면 완전 수렴까지)
    pub convergence_threshold: f64,
    /// 비동기 모드 사용 (기본값: true, 더 빠른 수렴)
    pub async_update: bool,
    /// 관계 타입 필터 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 가중치 속성명 (None이면 가중치 미사용)
    pub weight_property: Option<String>,
    /// 시드 레이블 (노드ID -> 레이블, Semi-supervised 학습용)
    /// 시드 레이블이 지정된 노드는 레이블이 변경되지 않음
    pub seed_labels: Option<HashMap<NodeId, usize>>,
}

impl Default for LabelPropagationConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            convergence_threshold: 0.0,
            async_update: true,
            relationship_types: None,
            weight_property: None,
            seed_labels: None,
        }
    }
}

impl LabelPropagationConfig {
    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// 수렴 임계값 설정
    pub fn with_convergence_threshold(mut self, threshold: f64) -> Self {
        self.convergence_threshold = threshold;
        self
    }

    /// 비동기 모드 설정
    pub fn with_async_update(mut self, async_update: bool) -> Self {
        self.async_update = async_update;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 가중치 속성명 설정
    pub fn with_weight_property(mut self, property: String) -> Self {
        self.weight_property = Some(property);
        self
    }

    /// 시드 레이블 설정 (Semi-supervised 학습)
    /// 시드 레이블이 지정된 노드는 레이블이 변경되지 않음
    pub fn with_seed_labels(mut self, seeds: HashMap<NodeId, usize>) -> Self {
        self.seed_labels = Some(seeds);
        self
    }
}

/// Label Propagation 알고리즘 결과
#[derive(Debug, Clone)]
pub struct LabelPropagationResult {
    /// 노드별 커뮤니티 레이블
    pub labels: HashMap<NodeId, usize>,
    /// 커뮤니티 목록 (레이블 -> 노드 목록)
    pub communities: HashMap<usize, Vec<NodeId>>,
    /// 커뮤니티 수
    pub community_count: usize,
    /// 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 총 노드 수
    pub node_count: usize,
}

impl LabelPropagationResult {
    /// 특정 노드의 커뮤니티 레이블 조회
    pub fn label(&self, node_id: NodeId) -> Option<usize> {
        self.labels.get(&node_id).copied()
    }

    /// 두 노드가 같은 커뮤니티에 속하는지 확인
    pub fn same_community(&self, node1: NodeId, node2: NodeId) -> bool {
        match (self.label(node1), self.label(node2)) {
            (Some(l1), Some(l2)) => l1 == l2,
            _ => false,
        }
    }

    /// 가장 큰 커뮤니티의 레이블과 크기 반환
    pub fn largest_community(&self) -> Option<(usize, usize)> {
        self.communities
            .iter()
            .max_by_key(|(_, nodes)| nodes.len())
            .map(|(&label, nodes)| (label, nodes.len()))
    }

    /// 특정 커뮤니티에 속한 노드들 반환
    pub fn nodes_in_community(&self, label: usize) -> Vec<NodeId> {
        self.communities.get(&label).cloned().unwrap_or_default()
    }

    /// 커뮤니티별 크기 반환
    pub fn community_sizes(&self) -> HashMap<usize, usize> {
        self.communities
            .iter()
            .map(|(&label, nodes)| (label, nodes.len()))
            .collect()
    }
}

/// 이웃 정보 (가중치 포함)
type WeightedNeighbors = HashMap<NodeId, Vec<(NodeId, f64)>>;

/// Label Propagation 알고리즘 계산기
pub struct LabelPropagation<'a> {
    engine: &'a GraphEngine,
    config: LabelPropagationConfig,
}

impl<'a> LabelPropagation<'a> {
    /// 새로운 LabelPropagation 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: LabelPropagationConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: LabelPropagationConfig) -> Self {
        Self { engine, config }
    }

    /// 커뮤니티 탐지 실행
    pub async fn detect(&self) -> Result<LabelPropagationResult> {
        // 1. 모든 노드 수집
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(LabelPropagationResult {
                labels: HashMap::new(),
                communities: HashMap::new(),
                community_count: 0,
                iterations: 0,
                converged: true,
                node_count: 0,
            });
        }

        // 2. 이웃 정보 미리 계산 (가중치 포함)
        let neighbors_map = self.build_weighted_neighbors_map(&nodes).await?;

        // 3. 초기 레이블 할당
        let mut labels: HashMap<NodeId, usize> = self.initialize_labels(&nodes);

        // 4. 시드 레이블이 있는 노드 집합 (이 노드들은 업데이트하지 않음)
        let seed_nodes: std::collections::HashSet<NodeId> = self
            .config
            .seed_labels
            .as_ref()
            .map(|seeds| seeds.keys().copied().collect())
            .unwrap_or_default();

        // 5. 반복 수행
        let mut iterations = 0;
        let mut converged = false;
        let node_count = nodes.len();

        while iterations < self.config.max_iterations && !converged {
            iterations += 1;

            let changes = if self.config.async_update {
                self.async_update_weighted(&nodes, &mut labels, &neighbors_map, &seed_nodes)
            } else {
                self.sync_update_weighted(&nodes, &mut labels, &neighbors_map, &seed_nodes)
            };

            // 수렴 체크
            let change_ratio = changes as f64 / node_count as f64;
            if changes == 0 || change_ratio <= self.config.convergence_threshold {
                converged = true;
            }
        }

        // 6. 커뮤니티 빌드 및 레이블 정규화
        let (normalized_labels, communities) = self.build_communities(&labels);

        Ok(LabelPropagationResult {
            labels: normalized_labels,
            community_count: communities.len(),
            communities,
            iterations,
            converged,
            node_count,
        })
    }

    /// 초기 레이블 할당
    /// 시드 레이블이 있으면 해당 레이블 사용, 없으면 고유 레이블 할당
    fn initialize_labels(&self, nodes: &[NodeId]) -> HashMap<NodeId, usize> {
        let mut labels = HashMap::new();

        // 시드 레이블에서 사용된 최대 레이블 찾기
        let max_seed_label = self
            .config
            .seed_labels
            .as_ref()
            .map(|seeds| seeds.values().copied().max().unwrap_or(0))
            .unwrap_or(0);

        let mut next_label = max_seed_label + 1;

        for &node in nodes {
            // 시드 레이블이 있으면 사용
            if let Some(ref seeds) = self.config.seed_labels
                && let Some(&seed_label) = seeds.get(&node)
            {
                labels.insert(node, seed_label);
                continue;
            }
            // 없으면 새 레이블 할당
            labels.insert(node, next_label);
            next_label += 1;
        }

        labels
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

    /// 가중치 포함 이웃 정보 맵 빌드 (무방향)
    async fn build_weighted_neighbors_map(&self, nodes: &[NodeId]) -> Result<WeightedNeighbors> {
        let mut neighbors_map: WeightedNeighbors = HashMap::new();

        for &node in nodes {
            neighbors_map.insert(node, Vec::new());
        }

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // 가중치 추출
                let weight = self.get_edge_weight(&edge);

                // 양쪽 노드가 모두 유효한지 확인
                if neighbors_map.contains_key(&from) && neighbors_map.contains_key(&to) {
                    // 무방향 그래프로 처리
                    if from != to {
                        if let Some(from_neighbors) = neighbors_map.get_mut(&from) {
                            from_neighbors.push((to, weight));
                        }
                        if let Some(to_neighbors) = neighbors_map.get_mut(&to) {
                            to_neighbors.push((from, weight));
                        }
                    }
                }
            }
        }

        Ok(neighbors_map)
    }

    /// 엣지에서 가중치 추출
    fn get_edge_weight(&self, edge: &crate::core::Edge) -> f64 {
        if let Some(ref prop_name) = self.config.weight_property {
            let props = edge.properties();
            if let Some(value) = props.get(prop_name) {
                return match *value {
                    crate::core::Value::Float(f) => f,
                    crate::core::Value::Integer(i) => i as f64,
                    _ => 1.0,
                };
            }
        }
        1.0 // 기본 가중치
    }

    /// 동기식 업데이트 (가중치 지원)
    fn sync_update_weighted(
        &self,
        nodes: &[NodeId],
        labels: &mut HashMap<NodeId, usize>,
        neighbors_map: &WeightedNeighbors,
        seed_nodes: &std::collections::HashSet<NodeId>,
    ) -> usize {
        let mut new_labels: HashMap<NodeId, usize> = HashMap::new();
        let mut changes = 0;

        // 모든 노드에 대해 새 레이블 계산
        for &node in nodes {
            // 시드 노드는 레이블 변경하지 않음
            if seed_nodes.contains(&node) {
                new_labels.insert(node, labels[&node]);
                continue;
            }
            let new_label = self.weighted_most_frequent_label(node, labels, neighbors_map);
            new_labels.insert(node, new_label);
        }

        // 일괄 적용
        for &node in nodes {
            if seed_nodes.contains(&node) {
                continue;
            }
            let old_label = labels[&node];
            let new_label = new_labels[&node];
            if old_label != new_label {
                labels.insert(node, new_label);
                changes += 1;
            }
        }

        changes
    }

    /// 비동기식 업데이트 (가중치 지원)
    fn async_update_weighted(
        &self,
        nodes: &[NodeId],
        labels: &mut HashMap<NodeId, usize>,
        neighbors_map: &WeightedNeighbors,
        seed_nodes: &std::collections::HashSet<NodeId>,
    ) -> usize {
        let mut changes = 0;

        for &node in nodes {
            // 시드 노드는 레이블 변경하지 않음
            if seed_nodes.contains(&node) {
                continue;
            }

            let old_label = labels[&node];
            let new_label = self.weighted_most_frequent_label(node, labels, neighbors_map);

            if old_label != new_label {
                labels.insert(node, new_label);
                changes += 1;
            }
        }

        changes
    }

    /// 가중치 기반 가장 빈번한 이웃 레이블 선택
    /// 동률인 경우 가장 작은 레이블 선택 (결정적 결과)
    fn weighted_most_frequent_label(
        &self,
        node: NodeId,
        labels: &HashMap<NodeId, usize>,
        neighbors_map: &WeightedNeighbors,
    ) -> usize {
        let neighbors = match neighbors_map.get(&node) {
            Some(n) => n,
            None => return labels.get(&node).copied().unwrap_or(0),
        };

        if neighbors.is_empty() {
            // 이웃이 없으면 현재 레이블 유지
            return labels.get(&node).copied().unwrap_or(0);
        }

        // 가중치 기반 레이블 점수 계산
        let mut label_scores: HashMap<usize, f64> = HashMap::new();
        for &(neighbor, weight) in neighbors {
            if let Some(&label) = labels.get(&neighbor) {
                *label_scores.entry(label).or_insert(0.0) += weight;
            }
        }

        // 가장 높은 점수의 레이블 찾기 (동률이면 가장 작은 레이블)
        label_scores
            .into_iter()
            .max_by(|(label1, score1), (label2, score2)| {
                score1
                    .partial_cmp(score2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| label2.cmp(label1))
            })
            .map(|(label, _)| label)
            .unwrap_or_else(|| labels.get(&node).copied().unwrap_or(0))
    }

    /// 커뮤니티 빌드 및 레이블 정규화
    fn build_communities(
        &self,
        labels: &HashMap<NodeId, usize>,
    ) -> (HashMap<NodeId, usize>, HashMap<usize, Vec<NodeId>>) {
        // 커뮤니티 그룹화
        let mut temp_communities: HashMap<usize, Vec<NodeId>> = HashMap::new();
        for (&node, &label) in labels {
            temp_communities.entry(label).or_default().push(node);
        }

        // 레이블 정규화 (0부터 연속된 숫자로)
        let mut label_remap: HashMap<usize, usize> = HashMap::new();

        // 정렬된 순서로 처리 (결정적 결과)
        let mut sorted_labels: Vec<_> = temp_communities.keys().copied().collect();
        sorted_labels.sort();

        for (next_label, old_label) in sorted_labels.into_iter().enumerate() {
            label_remap.insert(old_label, next_label);
        }

        // 정규화된 레이블 적용
        let normalized_labels: HashMap<NodeId, usize> = labels
            .iter()
            .map(|(&node, &label)| (node, label_remap[&label]))
            .collect();

        // 정규화된 커뮤니티 빌드
        let mut communities: HashMap<usize, Vec<NodeId>> = HashMap::new();
        for (&node, &label) in &normalized_labels {
            communities.entry(label).or_default().push(node);
        }

        // 각 커뮤니티의 노드 목록 정렬
        for nodes in communities.values_mut() {
            nodes.sort();
        }

        (normalized_labels, communities)
    }
}

/// Label Propagation 커뮤니티 탐지 (편의 함수)
pub async fn label_propagation(engine: &GraphEngine) -> Result<LabelPropagationResult> {
    LabelPropagation::new(engine).detect().await
}

/// 최대 반복 횟수 지정 Label Propagation (편의 함수)
pub async fn label_propagation_with_iterations(
    engine: &GraphEngine,
    max_iterations: usize,
) -> Result<LabelPropagationResult> {
    let config = LabelPropagationConfig::default().with_max_iterations(max_iterations);
    LabelPropagation::with_config(engine, config).detect().await
}

/// Label Propagation 분석기
///
/// LPA 결과에 대한 다양한 통계 및 분석 기능을 제공합니다.
pub struct LabelPropagationAnalyzer<'a> {
    engine: &'a GraphEngine,
    result: LabelPropagationResult,
}

impl<'a> LabelPropagationAnalyzer<'a> {
    /// 새로운 분석기 생성 (기본 설정으로 LPA 실행)
    pub async fn new(engine: &'a GraphEngine) -> Result<Self> {
        let result = LabelPropagation::new(engine).detect().await?;
        Ok(Self { engine, result })
    }

    /// 설정과 함께 분석기 생성
    pub async fn with_config(
        engine: &'a GraphEngine,
        config: LabelPropagationConfig,
    ) -> Result<Self> {
        let result = LabelPropagation::with_config(engine, config)
            .detect()
            .await?;
        Ok(Self { engine, result })
    }

    /// 기존 결과로 분석기 생성
    pub fn from_result(engine: &'a GraphEngine, result: LabelPropagationResult) -> Self {
        Self { engine, result }
    }

    /// 결과 참조 반환
    pub fn result(&self) -> &LabelPropagationResult {
        &self.result
    }

    /// 결과 소유권 반환
    pub fn into_result(self) -> LabelPropagationResult {
        self.result
    }

    /// 평균 커뮤니티 크기
    pub fn average_community_size(&self) -> f64 {
        if self.result.community_count == 0 {
            return 0.0;
        }
        self.result.node_count as f64 / self.result.community_count as f64
    }

    /// 커뮤니티 크기 분산
    pub fn community_size_variance(&self) -> f64 {
        if self.result.community_count == 0 {
            return 0.0;
        }

        let mean = self.average_community_size();
        let sizes: Vec<f64> = self
            .result
            .communities
            .values()
            .map(|nodes| nodes.len() as f64)
            .collect();

        let variance: f64 =
            sizes.iter().map(|&size| (size - mean).powi(2)).sum::<f64>() / sizes.len() as f64;

        variance
    }

    /// 커뮤니티 크기 표준편차
    pub fn community_size_std_dev(&self) -> f64 {
        self.community_size_variance().sqrt()
    }

    /// 가장 작은 커뮤니티
    pub fn smallest_community(&self) -> Option<(usize, usize)> {
        self.result
            .communities
            .iter()
            .min_by_key(|(_, nodes)| nodes.len())
            .map(|(&label, nodes)| (label, nodes.len()))
    }

    /// 커뮤니티 크기 분포 (크기 -> 해당 크기의 커뮤니티 수)
    pub fn size_distribution(&self) -> HashMap<usize, usize> {
        let mut distribution: HashMap<usize, usize> = HashMap::new();
        for nodes in self.result.communities.values() {
            *distribution.entry(nodes.len()).or_insert(0) += 1;
        }
        distribution
    }

    /// 싱글톤 커뮤니티 수 (크기가 1인 커뮤니티)
    pub fn singleton_count(&self) -> usize {
        self.result
            .communities
            .values()
            .filter(|nodes| nodes.len() == 1)
            .count()
    }

    /// 노드의 커뮤니티 내 이웃 비율 (커뮤니티 강도)
    /// 해당 노드의 이웃 중 같은 커뮤니티에 속한 비율
    pub async fn node_community_strength(&self, node_id: NodeId) -> Result<f64> {
        let node_label = match self.result.label(node_id) {
            Some(l) => l,
            None => return Ok(0.0),
        };

        // 노드의 모든 이웃 찾기
        let mut total_neighbors = 0;
        let mut same_community_neighbors = 0;

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                let (from, to) = (edge.from_node(), edge.to_node());
                let neighbor = if from == node_id {
                    to
                } else if to == node_id {
                    from
                } else {
                    continue;
                };

                total_neighbors += 1;
                if let Some(neighbor_label) = self.result.label(neighbor)
                    && neighbor_label == node_label
                {
                    same_community_neighbors += 1;
                }
            }
        }

        if total_neighbors == 0 {
            Ok(1.0) // 이웃이 없으면 완벽한 커뮤니티 강도
        } else {
            Ok(same_community_neighbors as f64 / total_neighbors as f64)
        }
    }

    /// 전체 그래프의 평균 커뮤니티 강도
    pub async fn average_community_strength(&self) -> Result<f64> {
        if self.result.node_count == 0 {
            return Ok(0.0);
        }

        let mut total_strength = 0.0;
        for &node_id in self.result.labels.keys() {
            total_strength += self.node_community_strength(node_id).await?;
        }

        Ok(total_strength / self.result.node_count as f64)
    }

    /// 커버리지: 커뮤니티 내부 엣지 비율
    pub async fn coverage(&self) -> Result<f64> {
        let edge_count = self.engine.edge_count().await;
        if edge_count == 0 {
            return Ok(0.0);
        }

        let mut intra_community_edges = 0;
        let mut total_edges = 0;

        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                total_edges += 1;
                let from_label = self.result.label(edge.from_node());
                let to_label = self.result.label(edge.to_node());

                if let (Some(l1), Some(l2)) = (from_label, to_label)
                    && l1 == l2
                {
                    intra_community_edges += 1;
                }
            }
        }

        if total_edges == 0 {
            Ok(0.0)
        } else {
            Ok(intra_community_edges as f64 / total_edges as f64)
        }
    }

    /// 종합 통계 반환
    pub async fn statistics(&self) -> Result<LabelPropagationStatistics> {
        Ok(LabelPropagationStatistics {
            community_count: self.result.community_count,
            node_count: self.result.node_count,
            iterations: self.result.iterations,
            converged: self.result.converged,
            average_community_size: self.average_community_size(),
            community_size_std_dev: self.community_size_std_dev(),
            largest_community: self.result.largest_community(),
            smallest_community: self.smallest_community(),
            singleton_count: self.singleton_count(),
            coverage: self.coverage().await?,
            average_community_strength: self.average_community_strength().await?,
        })
    }
}

/// Label Propagation 통계 정보
#[derive(Debug, Clone)]
pub struct LabelPropagationStatistics {
    /// 커뮤니티 수
    pub community_count: usize,
    /// 노드 수
    pub node_count: usize,
    /// 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 평균 커뮤니티 크기
    pub average_community_size: f64,
    /// 커뮤니티 크기 표준편차
    pub community_size_std_dev: f64,
    /// 가장 큰 커뮤니티 (레이블, 크기)
    pub largest_community: Option<(usize, usize)>,
    /// 가장 작은 커뮤니티 (레이블, 크기)
    pub smallest_community: Option<(usize, usize)>,
    /// 싱글톤 커뮤니티 수
    pub singleton_count: usize,
    /// 커버리지 (커뮤니티 내부 엣지 비율)
    pub coverage: f64,
    /// 평균 커뮤니티 강도
    pub average_community_strength: f64,
}
