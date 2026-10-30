//! 중심성 알고리즘 스트리밍 구현
//!
//! 대규모 그래프에서 메모리 효율적인 중심성 계산

use super::config::{AlgorithmPhase, StreamingConfig};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};

/// 중심성 점수
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CentralityScore {
    /// 노드 ID
    pub node_id: NodeId,
    /// 중심성 점수
    pub score: f64,
}

impl CentralityScore {
    /// 새 중심성 점수 생성
    pub fn new(node_id: NodeId, score: f64) -> Self {
        Self { node_id, score }
    }
}

impl Eq for CentralityScore {}

impl Ord for CentralityScore {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for CentralityScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ============================================================================
// Streaming PageRank
// ============================================================================

/// 스트리밍 PageRank 설정
#[derive(Debug, Clone)]
pub struct StreamingPageRankConfig {
    /// 감쇠 계수 (damping factor)
    pub damping_factor: f64,
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 임계값
    pub tolerance: f64,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
    /// 최소 점수 (이보다 작은 점수는 결과에서 제외)
    pub min_score: f64,
    /// 상위 N개만 반환 (None이면 모두)
    pub top_n: Option<usize>,
}

impl Default for StreamingPageRankConfig {
    fn default() -> Self {
        Self {
            damping_factor: 0.85,
            max_iterations: 20,
            tolerance: 1e-6,
            relationship_types: None,
            min_score: 0.0,
            top_n: None,
        }
    }
}

impl StreamingPageRankConfig {
    /// 감쇠 계수 설정
    pub fn with_damping_factor(mut self, factor: f64) -> Self {
        self.damping_factor = factor;
        self
    }

    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, iterations: usize) -> Self {
        self.max_iterations = iterations;
        self
    }

    /// 수렴 임계값 설정
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// 최소 점수 설정 (필터)
    pub fn with_min_score(mut self, min: f64) -> Self {
        self.min_score = min;
        self
    }

    /// 상위 N개만 반환
    pub fn with_top_n(mut self, n: usize) -> Self {
        self.top_n = Some(n);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// PageRank 점수 반복자
pub struct PageRankScoreIterator {
    /// 정렬된 점수 큐
    scores: VecDeque<CentralityScore>,
    /// 총 노드 수
    total_nodes: usize,
    /// 반복 횟수
    iterations: usize,
    /// 수렴 여부
    converged: bool,
}

impl PageRankScoreIterator {
    /// 반복 횟수
    pub fn iterations(&self) -> usize {
        self.iterations
    }

    /// 수렴 여부
    pub fn converged(&self) -> bool {
        self.converged
    }

    /// 총 노드 수
    pub fn total_nodes(&self) -> usize {
        self.total_nodes
    }

    /// 남은 결과 수
    pub fn remaining(&self) -> usize {
        self.scores.len()
    }
}

impl Iterator for PageRankScoreIterator {
    type Item = CentralityScore;

    fn next(&mut self) -> Option<Self::Item> {
        self.scores.pop_front()
    }
}

/// 스트리밍 PageRank 계산기
pub struct StreamingPageRank<'a> {
    engine: &'a GraphEngine,
    config: StreamingPageRankConfig,
    streaming_config: StreamingConfig,
}

impl<'a> StreamingPageRank<'a> {
    /// 새 스트리밍 PageRank 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: StreamingPageRankConfig::default(),
            streaming_config: StreamingConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(
        engine: &'a GraphEngine,
        config: StreamingPageRankConfig,
        streaming_config: StreamingConfig,
    ) -> Self {
        Self {
            engine,
            config,
            streaming_config,
        }
    }

    /// PageRank 계산 및 스트리밍 반복자 반환
    pub async fn compute(self) -> Result<PageRankScoreIterator> {
        // 1. 그래프 구조 수집
        self.streaming_config
            .report_progress(0.0, AlgorithmPhase::CollectingGraph, 0);

        let (nodes, outgoing, incoming) = self.build_graph_structure().await?;

        if nodes.is_empty() {
            return Ok(PageRankScoreIterator {
                scores: VecDeque::new(),
                total_nodes: 0,
                iterations: 0,
                converged: true,
            });
        }

        let n = nodes.len();
        let initial_score = 1.0 / n as f64;
        let damping = self.config.damping_factor;
        let teleport = (1.0 - damping) / n as f64;

        // 2. 초기 점수 설정
        let mut scores: HashMap<NodeId, f64> =
            nodes.iter().map(|&id| (id, initial_score)).collect();

        let mut iterations = 0;
        let mut converged = false;

        // 3. 반복 계산
        for iter in 0..self.config.max_iterations {
            iterations = iter + 1;

            // 진행 상황 보고
            let progress = iter as f64 / self.config.max_iterations as f64;
            self.streaming_config
                .report_progress(progress, AlgorithmPhase::Computing, iter);

            // 조기 종료 체크
            if self.streaming_config.should_stop() {
                break;
            }

            let mut new_scores: HashMap<NodeId, f64> = HashMap::new();
            let mut max_diff: f64 = 0.0;

            // 댕글링 노드 처리
            let dangling_sum: f64 = nodes
                .iter()
                .filter(|&&id| outgoing.get(&id).is_none_or(|v| v.is_empty()))
                .map(|&id| scores.get(&id).copied().unwrap_or(0.0))
                .sum();
            let dangling_contribution = damping * dangling_sum / n as f64;

            for &node_id in &nodes {
                let incoming_contribution: f64 = incoming
                    .get(&node_id)
                    .map(|sources| {
                        sources
                            .iter()
                            .map(|&src| {
                                let src_score = scores.get(&src).copied().unwrap_or(0.0);
                                let src_out_degree = outgoing.get(&src).map_or(0, |v| v.len());
                                if src_out_degree > 0 {
                                    src_score / src_out_degree as f64
                                } else {
                                    0.0
                                }
                            })
                            .sum()
                    })
                    .unwrap_or(0.0);

                let new_score = teleport + damping * incoming_contribution + dangling_contribution;

                let old_score = scores.get(&node_id).copied().unwrap_or(0.0);
                max_diff = max_diff.max((new_score - old_score).abs());

                new_scores.insert(node_id, new_score);
            }

            scores = new_scores;

            if max_diff < self.config.tolerance {
                converged = true;
                break;
            }
        }

        // 4. 점수 정규화
        let total: f64 = scores.values().sum();
        if total > 0.0 {
            for score in scores.values_mut() {
                *score /= total;
            }
        }

        self.streaming_config
            .report_progress(1.0, AlgorithmPhase::Finalizing, n);

        // 5. 결과 필터링 및 정렬
        let mut result_scores: Vec<CentralityScore> = scores
            .into_iter()
            .filter(|(_, score)| *score >= self.config.min_score)
            .map(|(node_id, score)| CentralityScore::new(node_id, score))
            .collect();

        // 점수로 정렬 (내림차순)
        result_scores.sort();

        // top_n 적용
        if let Some(top_n) = self.config.top_n {
            result_scores.truncate(top_n);
        }

        Ok(PageRankScoreIterator {
            scores: result_scores.into_iter().collect(),
            total_nodes: n,
            iterations,
            converged,
        })
    }

    /// 그래프 구조 빌드
    async fn build_graph_structure(
        &self,
    ) -> Result<(
        Vec<NodeId>,
        HashMap<NodeId, Vec<NodeId>>,
        HashMap<NodeId, Vec<NodeId>>,
    )> {
        let mut nodes: Vec<NodeId> = Vec::new();
        let mut outgoing: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut incoming: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                outgoing.insert(node_id, Vec::new());
                incoming.insert(node_id, Vec::new());
            }
        }

        // 엣지 수집
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

                outgoing.entry(from).or_default().push(to);
                incoming.entry(to).or_default().push(from);
            }
        }

        Ok((nodes, outgoing, incoming))
    }
}

// ============================================================================
// Streaming Betweenness Centrality
// ============================================================================

/// 스트리밍 Betweenness 설정
#[derive(Debug, Clone)]
pub struct StreamingBetweennessConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
    /// 정규화 여부
    pub normalized: bool,
    /// 방향 무시 여부
    pub undirected: bool,
    /// 샘플링할 노드 수 (근사 계산용, None이면 전체)
    pub sample_size: Option<usize>,
    /// 최소 점수 (이보다 작은 점수는 결과에서 제외)
    pub min_score: f64,
    /// 상위 N개만 반환
    pub top_n: Option<usize>,
    /// 자동 샘플링 임계값 (노드 수가 이 값 초과 시 자동 샘플링)
    pub auto_sample_threshold: usize,
    /// 메모리 예산 (MB, 자동 샘플 크기 계산용)
    pub memory_budget_mb: Option<usize>,
}

impl Default for StreamingBetweennessConfig {
    fn default() -> Self {
        Self {
            relationship_types: None,
            normalized: true,
            undirected: false,
            sample_size: None,
            min_score: 0.0,
            top_n: None,
            auto_sample_threshold: 100_000, // 100K 노드 초과 시 자동 샘플링
            memory_budget_mb: None,
        }
    }
}

impl StreamingBetweennessConfig {
    /// 정규화 설정
    pub fn with_normalized(mut self, normalized: bool) -> Self {
        self.normalized = normalized;
        self
    }

    /// 방향 무시 설정
    pub fn with_undirected(mut self, undirected: bool) -> Self {
        self.undirected = undirected;
        self
    }

    /// 샘플 크기 설정 (근사 계산)
    pub fn with_sample_size(mut self, size: usize) -> Self {
        self.sample_size = Some(size);
        self
    }

    /// 최소 점수 설정
    pub fn with_min_score(mut self, min: f64) -> Self {
        self.min_score = min;
        self
    }

    /// 상위 N개만 반환
    pub fn with_top_n(mut self, n: usize) -> Self {
        self.top_n = Some(n);
        self
    }

    /// 자동 샘플링 임계값 설정
    ///
    /// 노드 수가 이 값을 초과하면 자동으로 샘플링 모드가 활성화됩니다.
    pub fn with_auto_sample_threshold(mut self, threshold: usize) -> Self {
        self.auto_sample_threshold = threshold;
        self
    }

    /// 메모리 예산 설정 (MB)
    ///
    /// 메모리 예산을 기반으로 자동으로 sample_size를 계산합니다.
    /// 소스 노드당 약 80MB (1M 노드 기준) 메모리를 사용합니다.
    ///
    /// # 예시
    ///
    /// ```rust,ignore
    /// let config = StreamingBetweennessConfig::default()
    ///     .with_memory_budget(1024); // 1GB 예산 -> ~12개 소스 샘플링
    /// ```
    pub fn with_memory_budget(mut self, budget_mb: usize) -> Self {
        self.memory_budget_mb = Some(budget_mb);
        // 소스당 약 80MB (1M 노드 기준: HashMap<NodeId, i64> + sigma + predecessors)
        // 실제로는 노드 수에 따라 달라지지만, 안전한 추정치 사용
        const ESTIMATED_MB_PER_SOURCE: usize = 80;
        let computed_sample_size = budget_mb / ESTIMATED_MB_PER_SOURCE;
        if computed_sample_size > 0 && self.sample_size.is_none() {
            self.sample_size = Some(computed_sample_size.max(10)); // 최소 10개
        }
        self
    }

    /// 노드 수를 기반으로 자동 샘플 크기 계산
    ///
    /// 메모리 예산이 설정되어 있으면 그에 따라, 아니면 노드 수 기반으로 계산합니다.
    pub(crate) fn compute_effective_sample_size(&self, node_count: usize) -> Option<usize> {
        // 명시적 sample_size가 있으면 사용
        if self.sample_size.is_some() {
            return self.sample_size;
        }

        // 자동 샘플링 임계값 미만이면 전체 계산
        if node_count <= self.auto_sample_threshold {
            return None;
        }

        // 자동 샘플링: 노드 수의 제곱근 또는 메모리 기반
        if let Some(budget_mb) = self.memory_budget_mb {
            // 노드당 메모리 = (3 * HashMap 크기) / 노드 수
            // 대략 노드당 80바이트 (1M 노드에서 80MB)
            let bytes_per_node = 80;
            let bytes_per_source = node_count * bytes_per_node;
            let max_sources = (budget_mb * 1024 * 1024) / bytes_per_source;
            Some(max_sources.max(10))
        } else {
            // 기본: sqrt(n) 또는 최대 1000
            let sqrt_n = (node_count as f64).sqrt() as usize;
            Some(sqrt_n.clamp(100, 1000))
        }
    }
}

/// Betweenness 점수 반복자
pub struct BetweennessScoreIterator {
    scores: VecDeque<CentralityScore>,
    total_nodes: usize,
    sources_processed: usize,
}

impl BetweennessScoreIterator {
    /// 총 노드 수
    pub fn total_nodes(&self) -> usize {
        self.total_nodes
    }

    /// 처리된 소스 노드 수
    pub fn sources_processed(&self) -> usize {
        self.sources_processed
    }

    /// 남은 결과 수
    pub fn remaining(&self) -> usize {
        self.scores.len()
    }
}

impl Iterator for BetweennessScoreIterator {
    type Item = CentralityScore;

    fn next(&mut self) -> Option<Self::Item> {
        self.scores.pop_front()
    }
}

/// 스트리밍 Betweenness 중심성 계산기
pub struct StreamingBetweenness<'a> {
    engine: &'a GraphEngine,
    config: StreamingBetweennessConfig,
    streaming_config: StreamingConfig,
}

impl<'a> StreamingBetweenness<'a> {
    /// 새 스트리밍 Betweenness 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: StreamingBetweennessConfig::default(),
            streaming_config: StreamingConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(
        engine: &'a GraphEngine,
        config: StreamingBetweennessConfig,
        streaming_config: StreamingConfig,
    ) -> Self {
        Self {
            engine,
            config,
            streaming_config,
        }
    }

    /// Betweenness 계산 및 스트리밍 반복자 반환
    pub async fn compute(self) -> Result<BetweennessScoreIterator> {
        // 1. 그래프 구조 수집
        self.streaming_config
            .report_progress(0.0, AlgorithmPhase::CollectingGraph, 0);

        let (nodes, adjacency) = self.build_graph_structure().await?;

        if nodes.is_empty() {
            return Ok(BetweennessScoreIterator {
                scores: VecDeque::new(),
                total_nodes: 0,
                sources_processed: 0,
            });
        }

        let n = nodes.len();

        // 자동 샘플링 크기 계산 (메모리 보호)
        let effective_sample_size = self.config.compute_effective_sample_size(n);

        // 소스 노드 선택 (샘플링)
        let source_nodes: Vec<NodeId> = match effective_sample_size {
            Some(size) if size < n => {
                // 랜덤 샘플링 대신 균등 간격 선택
                let step = n / size;
                nodes
                    .iter()
                    .step_by(step.max(1))
                    .take(size)
                    .cloned()
                    .collect()
            }
            _ => nodes.clone(),
        };

        let sources_count = source_nodes.len();

        // 2. Betweenness 점수 초기화
        let mut betweenness: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();

        // 3. 각 소스에서 Brandes 알고리즘 실행
        for (idx, &source) in source_nodes.iter().enumerate() {
            // 진행 상황 보고
            let progress = idx as f64 / sources_count as f64;
            self.streaming_config
                .report_progress(progress, AlgorithmPhase::Computing, idx);

            // 조기 종료 체크
            if self.streaming_config.should_stop() {
                break;
            }

            // BFS로 최단 경로 수 및 거리 계산
            let (dist, sigma, predecessors) = self.bfs_from_source(source, &adjacency);

            // 역순으로 dependency 누적
            self.accumulate_dependencies(
                source,
                &nodes,
                &dist,
                &sigma,
                &predecessors,
                &mut betweenness,
            );
        }

        // 4. 정규화
        if self.config.normalized && n > 2 {
            let factor = if self.config.undirected {
                2.0 / ((n - 1) * (n - 2)) as f64
            } else {
                1.0 / ((n - 1) * (n - 2)) as f64
            };

            // 샘플링 보정 (자동 샘플링 포함)
            let sample_factor = if sources_count < n {
                n as f64 / sources_count as f64
            } else {
                1.0
            };

            for score in betweenness.values_mut() {
                *score *= factor * sample_factor;
            }
        }

        self.streaming_config
            .report_progress(1.0, AlgorithmPhase::Finalizing, n);

        // 5. 결과 필터링 및 정렬
        let mut result_scores: Vec<CentralityScore> = betweenness
            .into_iter()
            .filter(|(_, score)| *score >= self.config.min_score)
            .map(|(node_id, score)| CentralityScore::new(node_id, score))
            .collect();

        result_scores.sort();

        if let Some(top_n) = self.config.top_n {
            result_scores.truncate(top_n);
        }

        Ok(BetweennessScoreIterator {
            scores: result_scores.into_iter().collect(),
            total_nodes: n,
            sources_processed: sources_count,
        })
    }

    /// 그래프 구조 빌드
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

        // 엣지 수집
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

                adjacency.entry(from).or_default().push(to);

                if self.config.undirected {
                    adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok((nodes, adjacency))
    }

    /// BFS로 최단 경로 수 및 전임자 계산
    #[expect(clippy::type_complexity)]
    fn bfs_from_source(
        &self,
        source: NodeId,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
    ) -> (
        HashMap<NodeId, i64>,
        HashMap<NodeId, f64>,
        HashMap<NodeId, Vec<NodeId>>,
    ) {
        let mut dist: HashMap<NodeId, i64> = HashMap::new();
        let mut sigma: HashMap<NodeId, f64> = HashMap::new();
        let mut predecessors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        dist.insert(source, 0);
        sigma.insert(source, 1.0);

        let mut queue = VecDeque::new();
        queue.push_back(source);

        while let Some(v) = queue.pop_front() {
            let v_dist = dist.get(&v).copied().unwrap_or(0);

            if let Some(neighbors) = adjacency.get(&v) {
                for &w in neighbors {
                    // 첫 방문
                    if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(w) {
                        e.insert(v_dist + 1);
                        queue.push_back(w);
                    }

                    // 최단 경로 발견
                    if dist.get(&w) == Some(&(v_dist + 1)) {
                        let v_sigma = *sigma.get(&v).unwrap_or(&0.0);
                        *sigma.entry(w).or_insert(0.0) += v_sigma;
                        predecessors.entry(w).or_default().push(v);
                    }
                }
            }
        }

        (dist, sigma, predecessors)
    }

    /// Dependency 누적
    fn accumulate_dependencies(
        &self,
        source: NodeId,
        nodes: &[NodeId],
        dist: &HashMap<NodeId, i64>,
        sigma: &HashMap<NodeId, f64>,
        predecessors: &HashMap<NodeId, Vec<NodeId>>,
        betweenness: &mut HashMap<NodeId, f64>,
    ) {
        let mut delta: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 0.0)).collect();

        // 거리순으로 노드 정렬 (역순)
        let mut ordered: Vec<(NodeId, i64)> = dist.iter().map(|(&n, &d)| (n, d)).collect();
        ordered.sort_by_key(|entry| std::cmp::Reverse(entry.1));

        for (w, _) in ordered {
            if let Some(preds) = predecessors.get(&w) {
                for &v in preds {
                    let v_sigma = *sigma.get(&v).unwrap_or(&1.0);
                    let w_sigma = *sigma.get(&w).unwrap_or(&1.0);
                    let w_delta = *delta.get(&w).unwrap_or(&0.0);

                    let contribution = (v_sigma / w_sigma) * (1.0 + w_delta);
                    *delta.entry(v).or_insert(0.0) += contribution;
                }
            }

            if w != source {
                let w_delta = *delta.get(&w).unwrap_or(&0.0);
                *betweenness.entry(w).or_insert(0.0) += w_delta;
            }
        }
    }
}
