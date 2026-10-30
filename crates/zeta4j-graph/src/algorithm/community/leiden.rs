//! Leiden Community Detection 알고리즘
//!
//! Louvain 알고리즘의 개선 버전으로, 연결성이 보장된 커뮤니티를 탐지합니다.
//! MS GraphRAG에서 사용되는 핵심 알고리즘입니다.
//!
//! ## 알고리즘 개요
//! 1. Phase 1 (Local Moving): 각 노드를 이웃 커뮤니티로 이동하며 모듈러리티 증가 확인
//! 2. Phase 2 (Refinement): 커뮤니티 내 연결성 보장, 서브커뮤니티 분할
//! 3. Phase 3 (Aggregation): 커뮤니티를 단일 노드로 축소하여 새 그래프 생성
//! 4. 수렴할 때까지 반복
//!
//! 시간 복잡도: O(n log n) 평균
//!
//! ## 참고 문헌
//! - Traag, V.A., Waltman, L. & van Eck, N.J.
//!   "From Louvain to Leiden: guaranteeing well-connected communities"
//!   Sci Rep 9, 5233 (2019)

use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 진행률 콜백 타입
/// (현재 단계, 전체 단계, 현재 반복, 최대 반복, 메시지)
pub type ProgressCallback = Box<dyn Fn(usize, usize, usize, usize, &str) + Send + Sync>;

/// Leiden 알고리즘 설정
#[derive(Debug, Clone)]
pub struct LeidenConfig {
    /// 최대 반복 횟수 (전체 사이클)
    pub max_iterations: usize,
    /// 모듈러리티 개선 임계값 (이보다 작으면 수렴으로 판단)
    pub tolerance: f64,
    /// Resolution 파라미터 (1.0이 기본, 높을수록 작은 커뮤니티)
    pub resolution: f64,
    /// Refinement 반복 횟수
    pub refinement_iterations: usize,
    /// 연결성 보장 활성화 (대규모 그래프에서는 false 권장)
    pub ensure_connectivity: bool,
    /// 랜덤 시드 (노드 순서 셔플용, None이면 고정 순서)
    pub random_seed: Option<u64>,
    /// 계층적 커뮤니티 저장 (중간 레벨 저장)
    pub store_hierarchy: bool,
    /// 노드 수 자동 튜닝 임계값 (이 이상이면 자동으로 최적화)
    pub auto_tune_threshold: usize,
}

impl Default for LeidenConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            tolerance: 1e-7,
            resolution: 1.0,
            refinement_iterations: 2,
            // 프로덕션 최적화: 대규모 그래프에서 연결성 검증은 비용이 높음
            ensure_connectivity: false,
            random_seed: None,
            // 프로덕션 최적화: 계층 저장 비활성화
            store_hierarchy: false,
            // 1000 노드 이상이면 자동 튜닝
            auto_tune_threshold: 1000,
        }
    }
}

impl LeidenConfig {
    /// 고품질 커뮤니티 탐지용 설정 (연결성 보장, 계층 저장)
    /// 주의: 대규모 그래프에서 매우 느릴 수 있음
    pub fn high_quality() -> Self {
        Self {
            max_iterations: 20,
            tolerance: 1e-8,
            resolution: 1.0,
            refinement_iterations: 3,
            ensure_connectivity: true,
            random_seed: None,
            store_hierarchy: true,
            auto_tune_threshold: 500,
        }
    }

    /// 빠른 실행용 설정 (대규모 그래프 권장)
    pub fn fast() -> Self {
        Self {
            max_iterations: 5,
            tolerance: 1e-5,
            resolution: 1.0,
            refinement_iterations: 1,
            ensure_connectivity: false,
            random_seed: None,
            store_hierarchy: false,
            auto_tune_threshold: 500,
        }
    }
}

// Builder 메서드들 (LeidenConfig::default() 또는 preset 후 체이닝)
impl LeidenConfig {
    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// 수렴 임계값 설정
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Resolution 파라미터 설정
    pub fn with_resolution(mut self, resolution: f64) -> Self {
        self.resolution = resolution;
        self
    }

    /// Refinement 반복 횟수 설정
    pub fn with_refinement_iterations(mut self, iterations: usize) -> Self {
        self.refinement_iterations = iterations;
        self
    }

    /// 연결성 보장 활성화/비활성화
    /// 주의: 대규모 그래프(>1000 노드)에서 true로 설정하면 매우 느림
    pub fn with_ensure_connectivity(mut self, ensure: bool) -> Self {
        self.ensure_connectivity = ensure;
        self
    }

    /// 계층 저장 설정
    pub fn with_store_hierarchy(mut self, store: bool) -> Self {
        self.store_hierarchy = store;
        self
    }

    /// 자동 튜닝 임계값 설정
    pub fn with_auto_tune_threshold(mut self, threshold: usize) -> Self {
        self.auto_tune_threshold = threshold;
        self
    }

    /// 노드 수에 따라 자동으로 설정 조정
    pub fn auto_tune_for_node_count(mut self, node_count: usize) -> Self {
        if node_count > self.auto_tune_threshold {
            // 대규모 그래프: 성능 최적화
            self.ensure_connectivity = false;
            self.store_hierarchy = false;
            self.max_iterations = self.max_iterations.min(5);
            self.refinement_iterations = self.refinement_iterations.min(1);
        }
        self
    }
}

/// Leiden 알고리즘 결과
#[derive(Debug, Clone)]
pub struct LeidenResult {
    /// 노드별 최종 커뮤니티 ID
    pub communities: HashMap<NodeId, usize>,
    /// 커뮤니티 수
    pub community_count: usize,
    /// 최종 모듈러리티 값
    pub modularity: f64,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 커뮤니티별 노드 수
    pub community_sizes: HashMap<usize, usize>,
    /// 계층적 커뮤니티 구조 (레벨별 커뮤니티 할당)
    pub hierarchy: Vec<HashMap<NodeId, usize>>,
    /// 연결성 보장 여부
    pub is_connected: bool,
}

impl LeidenResult {
    /// 특정 노드의 커뮤니티 ID 조회
    pub fn community_id(&self, node_id: NodeId) -> Option<usize> {
        self.communities.get(&node_id).copied()
    }

    /// 가장 큰 커뮤니티의 ID와 크기 반환
    pub fn largest_community(&self) -> Option<(usize, usize)> {
        self.community_sizes
            .iter()
            .max_by_key(|&(_, &size)| size)
            .map(|(&id, &size)| (id, size))
    }

    /// 특정 커뮤니티에 속한 노드들 반환
    pub fn nodes_in_community(&self, community_id: usize) -> Vec<NodeId> {
        self.communities
            .iter()
            .filter(|&(_, &cid)| cid == community_id)
            .map(|(&node_id, _)| node_id)
            .collect()
    }

    /// 두 노드가 같은 커뮤니티에 속하는지 확인
    pub fn same_community(&self, node1: NodeId, node2: NodeId) -> bool {
        match (self.community_id(node1), self.community_id(node2)) {
            (Some(c1), Some(c2)) => c1 == c2,
            _ => false,
        }
    }

    /// 특정 레벨의 커뮤니티 할당 조회
    pub fn communities_at_level(&self, level: usize) -> Option<&HashMap<NodeId, usize>> {
        self.hierarchy.get(level)
    }

    /// 커뮤니티 계층 깊이
    pub fn hierarchy_depth(&self) -> usize {
        self.hierarchy.len()
    }
}

/// 내부 그래프 표현 (Leiden 알고리즘용)
struct LeidenGraph {
    /// 노드 목록
    nodes: Vec<NodeId>,
    /// 노드 ID -> 인덱스 매핑
    node_to_idx: HashMap<NodeId, usize>,
    /// 인접 리스트: node_idx -> [(neighbor_idx, weight)]
    adjacency: Vec<Vec<(usize, f64)>>,
    /// 노드별 가중치 합 (자기 루프 포함)
    node_weights: Vec<f64>,
    /// 전체 엣지 가중치 합 (2m)
    total_weight: f64,
}

impl LeidenGraph {
    /// 새 그래프 생성
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_to_idx: HashMap::new(),
            adjacency: Vec::new(),
            node_weights: Vec::new(),
            total_weight: 0.0,
        }
    }

    /// 노드 수
    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 노드의 이웃과 가중치 조회
    fn neighbors(&self, node_idx: usize) -> &[(usize, f64)] {
        &self.adjacency[node_idx]
    }

    /// 두 노드 간의 연결 가중치 조회
    #[allow(dead_code)]
    fn edge_weight(&self, from_idx: usize, to_idx: usize) -> f64 {
        for &(neighbor, weight) in &self.adjacency[from_idx] {
            if neighbor == to_idx {
                return weight;
            }
        }
        0.0
    }

    /// 특정 커뮤니티 내 연결 컴포넌트 분할
    fn find_connected_components(
        &self,
        community_members: &[usize],
        node_to_community: &[usize],
        community_id: usize,
    ) -> Vec<Vec<usize>> {
        if community_members.is_empty() {
            return Vec::new();
        }

        let member_set: HashSet<usize> = community_members.iter().copied().collect();
        let mut visited: HashSet<usize> = HashSet::new();
        let mut components: Vec<Vec<usize>> = Vec::new();

        for &start in community_members {
            if visited.contains(&start) {
                continue;
            }

            let mut component = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back(start);
            visited.insert(start);

            while let Some(node) = queue.pop_front() {
                component.push(node);

                for &(neighbor, _) in self.neighbors(node) {
                    if !visited.contains(&neighbor)
                        && member_set.contains(&neighbor)
                        && node_to_community[neighbor] == community_id
                    {
                        visited.insert(neighbor);
                        queue.push_back(neighbor);
                    }
                }
            }

            components.push(component);
        }

        components
    }
}

/// Leiden Community Detection 계산기
pub struct Leiden<'a> {
    engine: &'a GraphEngine,
    config: LeidenConfig,
    /// 취소 토큰 (None이면 취소 불가)
    cancel_token: Option<Arc<AtomicBool>>,
    /// 진행률 콜백 (None이면 진행률 보고 안함)
    progress_callback: Option<Arc<ProgressCallback>>,
}

impl<'a> Leiden<'a> {
    /// 새로운 Leiden 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: LeidenConfig::default(),
            cancel_token: None,
            progress_callback: None,
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: LeidenConfig) -> Self {
        Self {
            engine,
            config,
            cancel_token: None,
            progress_callback: None,
        }
    }

    /// 취소 토큰 설정
    pub fn with_cancel_token(mut self, token: Arc<AtomicBool>) -> Self {
        self.cancel_token = Some(token);
        self
    }

    /// 진행률 콜백 설정
    pub fn with_progress_callback(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// 취소 여부 확인
    fn is_cancelled(&self) -> bool {
        self.cancel_token
            .as_ref()
            .map(|t| t.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// 진행률 보고
    fn report_progress(
        &self,
        step: usize,
        total_steps: usize,
        iteration: usize,
        max_iter: usize,
        msg: &str,
    ) {
        if let Some(callback) = &self.progress_callback {
            callback(step, total_steps, iteration, max_iter, msg);
        }
    }

    /// 전체 그래프에 대해 Leiden 커뮤니티 탐지 실행
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<LeidenResult> {
        // 취소 확인
        if self.is_cancelled() {
            return Ok(LeidenResult {
                communities: HashMap::new(),
                community_count: 0,
                modularity: 0.0,
                iterations: 0,
                community_sizes: HashMap::new(),
                hierarchy: Vec::new(),
                is_connected: true,
            });
        }

        self.report_progress(
            1,
            4,
            0,
            self.config.max_iterations,
            "Building graph structure",
        );

        // 1. 그래프 구조 빌드
        let graph = self.build_graph(rel_types).await?;

        if graph.node_count() == 0 {
            return Ok(LeidenResult {
                communities: HashMap::new(),
                community_count: 0,
                modularity: 0.0,
                iterations: 0,
                community_sizes: HashMap::new(),
                hierarchy: Vec::new(),
                is_connected: true,
            });
        }

        // 노드 수에 따른 자동 튜닝
        let config = self
            .config
            .clone()
            .auto_tune_for_node_count(graph.node_count());

        self.report_progress(
            2,
            4,
            0,
            config.max_iterations,
            &format!(
                "Initializing {} nodes, connectivity={}",
                graph.node_count(),
                config.ensure_connectivity
            ),
        );

        // 2. 초기 커뮤니티 할당 (각 노드가 자신만의 커뮤니티)
        let mut node_to_community: Vec<usize> = (0..graph.node_count()).collect();
        let mut community_weights: Vec<f64> = graph.node_weights.clone();
        let mut community_internal: Vec<f64> = vec![0.0; graph.node_count()];

        // 자기 루프 가중치를 내부 가중치에 추가
        for (node_idx, neighbors) in graph.adjacency.iter().enumerate() {
            for &(neighbor_idx, weight) in neighbors {
                if neighbor_idx == node_idx {
                    community_internal[node_idx] += weight;
                }
            }
        }

        let mut iterations = 0;
        let mut improved = true;
        let mut hierarchy: Vec<HashMap<NodeId, usize>> = Vec::new();

        // 3. 메인 루프: Phase 1 -> Phase 2 -> Phase 3
        while improved && iterations < config.max_iterations {
            // 취소 확인
            if self.is_cancelled() {
                self.report_progress(3, 4, iterations, config.max_iterations, "Cancelled");
                break;
            }

            improved = false;
            iterations += 1;

            self.report_progress(
                3,
                4,
                iterations,
                config.max_iterations,
                &format!(
                    "Iteration {}/{}: Phase 1 (Local Moving)",
                    iterations, config.max_iterations
                ),
            );

            // Phase 1: Local Moving (Louvain과 유사)
            let phase1_improved = self.phase1_local_moving_with_config(
                &graph,
                &mut node_to_community,
                &mut community_weights,
                &mut community_internal,
                &config,
            );

            if phase1_improved {
                improved = true;
            }

            // Phase 2: Refinement (Leiden 고유 - 연결성 보장)
            if config.ensure_connectivity {
                self.report_progress(
                    3,
                    4,
                    iterations,
                    config.max_iterations,
                    &format!(
                        "Iteration {}/{}: Phase 2 (Refinement)",
                        iterations, config.max_iterations
                    ),
                );

                let phase2_changed = self.phase2_refinement_with_config(
                    &graph,
                    &mut node_to_community,
                    &mut community_weights,
                    &mut community_internal,
                    &config,
                );

                if phase2_changed {
                    improved = true;
                }
            }

            // 계층 저장
            if config.store_hierarchy {
                let level_communities = self.collect_level_communities(&graph, &node_to_community);
                hierarchy.push(level_communities);
            }
        }

        self.report_progress(
            4,
            4,
            iterations,
            config.max_iterations,
            "Finalizing results",
        );

        // 4. 연결성 검증
        let is_connected = if config.ensure_connectivity && !self.is_cancelled() {
            self.verify_connectivity(&graph, &node_to_community)
        } else {
            false
        };

        // 5. 결과 정리
        let mut communities: HashMap<NodeId, usize> = HashMap::new();
        let mut community_remap: HashMap<usize, usize> = HashMap::new();
        let mut next_community_id = 0;

        for (node_idx, &comm) in node_to_community.iter().enumerate() {
            let remapped = *community_remap.entry(comm).or_insert_with(|| {
                let id = next_community_id;
                next_community_id += 1;
                id
            });
            communities.insert(graph.nodes[node_idx], remapped);
        }

        // 커뮤니티별 크기 계산
        let mut community_sizes: HashMap<usize, usize> = HashMap::new();
        for &comm in communities.values() {
            *community_sizes.entry(comm).or_insert(0) += 1;
        }

        // 최종 모듈러리티 계산
        let modularity = self.calculate_modularity(
            &graph,
            &node_to_community,
            &community_weights,
            &community_internal,
        );

        Ok(LeidenResult {
            communities,
            community_count: next_community_id,
            modularity,
            iterations,
            community_sizes,
            hierarchy,
            is_connected,
        })
    }

    /// Phase 1: 로컬 이동 최적화 (Louvain과 유사)
    #[allow(dead_code)]
    fn phase1_local_moving(
        &self,
        graph: &LeidenGraph,
        node_to_community: &mut [usize],
        community_weights: &mut [f64],
        community_internal: &mut [f64],
    ) -> bool {
        self.phase1_local_moving_with_config(
            graph,
            node_to_community,
            community_weights,
            community_internal,
            &self.config,
        )
    }

    /// Phase 1: 로컬 이동 최적화 (설정 지정 버전)
    fn phase1_local_moving_with_config(
        &self,
        graph: &LeidenGraph,
        node_to_community: &mut [usize],
        community_weights: &mut [f64],
        community_internal: &mut [f64],
        config: &LeidenConfig,
    ) -> bool {
        let m2 = graph.total_weight;
        if m2 == 0.0 {
            return false;
        }

        let mut improved = false;
        let mut changed = true;
        let mut local_iterations = 0;
        let max_local_iterations = graph.node_count().min(100); // 로컬 이동 최대 반복 제한

        while changed && local_iterations < max_local_iterations {
            // 취소 확인
            if self.is_cancelled() {
                break;
            }

            changed = false;
            local_iterations += 1;

            for node_idx in 0..graph.node_count() {
                let current_comm = node_to_community[node_idx];
                let node_weight = graph.node_weights[node_idx];

                // 현재 커뮤니티와의 연결 가중치
                let ki_in_current =
                    self.calculate_ki_in(graph, node_idx, current_comm, node_to_community);

                // 현재 커뮤니티에서 노드 제거
                community_weights[current_comm] -= node_weight;
                community_internal[current_comm] -= ki_in_current;

                // 이웃 커뮤니티들 수집
                let mut neighbor_communities: HashSet<usize> = HashSet::new();
                for &(neighbor_idx, _) in graph.neighbors(node_idx) {
                    if neighbor_idx != node_idx {
                        neighbor_communities.insert(node_to_community[neighbor_idx]);
                    }
                }
                neighbor_communities.insert(current_comm);

                // 최적의 커뮤니티 찾기
                let mut best_comm = current_comm;
                let mut best_delta = 0.0;

                for &target_comm in &neighbor_communities {
                    let ki_in_target =
                        self.calculate_ki_in(graph, node_idx, target_comm, node_to_community);

                    let sigma_tot = community_weights[target_comm];

                    // 모듈러리티 증가량 계산
                    let delta = ki_in_target / m2
                        - config.resolution * (sigma_tot * node_weight) / (m2 * m2) * 2.0;

                    if delta > best_delta + config.tolerance {
                        best_delta = delta;
                        best_comm = target_comm;
                    }
                }

                // 새 커뮤니티로 이동
                if best_comm != current_comm {
                    node_to_community[node_idx] = best_comm;
                    changed = true;
                    improved = true;
                }

                // 새 커뮤니티에 노드 추가
                let ki_in_new = self.calculate_ki_in(
                    graph,
                    node_idx,
                    node_to_community[node_idx],
                    node_to_community,
                );
                community_weights[node_to_community[node_idx]] += node_weight;
                community_internal[node_to_community[node_idx]] += ki_in_new;
            }
        }

        improved
    }

    /// Phase 2: Refinement - 커뮤니티 내 연결성 보장
    ///
    /// Leiden의 핵심 개선사항: 연결되지 않은 서브그래프를 분할
    #[allow(dead_code)]
    fn phase2_refinement(
        &self,
        graph: &LeidenGraph,
        node_to_community: &mut [usize],
        community_weights: &mut Vec<f64>,
        community_internal: &mut Vec<f64>,
    ) -> bool {
        self.phase2_refinement_with_config(
            graph,
            node_to_community,
            community_weights,
            community_internal,
            &self.config,
        )
    }

    /// Phase 2: Refinement - 커뮤니티 내 연결성 보장 (설정 지정 버전)
    fn phase2_refinement_with_config(
        &self,
        graph: &LeidenGraph,
        node_to_community: &mut [usize],
        community_weights: &mut Vec<f64>,
        community_internal: &mut Vec<f64>,
        config: &LeidenConfig,
    ) -> bool {
        // 취소 확인
        if self.is_cancelled() {
            return false;
        }

        let mut changed = false;

        // 현재 커뮤니티 목록 수집
        let mut communities: HashMap<usize, Vec<usize>> = HashMap::new();
        for (node_idx, &comm) in node_to_community.iter().enumerate() {
            communities.entry(comm).or_default().push(node_idx);
        }

        let max_comm_id = node_to_community.iter().copied().max().unwrap_or(0);
        let mut next_comm_id = max_comm_id + 1;

        // 각 커뮤니티에 대해 연결 컴포넌트 확인
        for (&comm_id, members) in &communities {
            // 취소 확인
            if self.is_cancelled() {
                break;
            }

            if members.len() <= 1 {
                continue;
            }

            let components = graph.find_connected_components(members, node_to_community, comm_id);

            // 여러 컴포넌트로 분할된 경우
            if components.len() > 1 {
                changed = true;

                // 가장 큰 컴포넌트는 원래 커뮤니티 유지
                let mut sorted_components = components;
                sorted_components.sort_by_key(|a| std::cmp::Reverse(a.len()));

                // 나머지 컴포넌트들은 새로운 커뮤니티로
                for component in sorted_components.into_iter().skip(1) {
                    let new_comm_id = next_comm_id;
                    next_comm_id += 1;

                    // 필요시 벡터 확장
                    while community_weights.len() <= new_comm_id {
                        community_weights.push(0.0);
                        community_internal.push(0.0);
                    }

                    for &node_idx in &component {
                        // 이전 커뮤니티에서 제거
                        let old_ki_in = self.calculate_ki_in(
                            graph,
                            node_idx,
                            node_to_community[node_idx],
                            node_to_community,
                        );
                        community_weights[node_to_community[node_idx]] -=
                            graph.node_weights[node_idx];
                        community_internal[node_to_community[node_idx]] -= old_ki_in;

                        // 새 커뮤니티로 이동
                        node_to_community[node_idx] = new_comm_id;
                    }

                    // 새 커뮤니티 가중치 계산
                    for &node_idx in &component {
                        let ki_in =
                            self.calculate_ki_in(graph, node_idx, new_comm_id, node_to_community);
                        community_weights[new_comm_id] += graph.node_weights[node_idx];
                        community_internal[new_comm_id] += ki_in;
                    }
                }
            }
        }

        // Refinement 내 로컬 이동 (선택적)
        for _ in 0..config.refinement_iterations {
            if self.is_cancelled() {
                break;
            }

            let local_changed = self.phase1_local_moving_with_config(
                graph,
                node_to_community,
                community_weights,
                community_internal,
                config,
            );
            if !local_changed {
                break;
            }
            changed = true;
        }

        changed
    }

    /// 노드와 특정 커뮤니티 사이의 연결 가중치 합 계산
    fn calculate_ki_in(
        &self,
        graph: &LeidenGraph,
        node_idx: usize,
        community: usize,
        node_to_community: &[usize],
    ) -> f64 {
        let mut ki_in = 0.0;

        for &(neighbor_idx, weight) in graph.neighbors(node_idx) {
            if node_to_community[neighbor_idx] == community {
                ki_in += weight;
            }
        }

        ki_in
    }

    /// 모듈러리티 계산
    fn calculate_modularity(
        &self,
        graph: &LeidenGraph,
        node_to_community: &[usize],
        community_weights: &[f64],
        community_internal: &[f64],
    ) -> f64 {
        let m2 = graph.total_weight;
        if m2 == 0.0 {
            return 0.0;
        }

        let mut q = 0.0;

        let max_comm = node_to_community.iter().copied().max().unwrap_or(0);
        for comm in 0..=max_comm {
            if comm < community_weights.len() && community_weights[comm] > 0.0 {
                let sigma_tot = community_weights[comm];
                let sigma_in = community_internal[comm];

                q += sigma_in / m2 - self.config.resolution * (sigma_tot / m2).powi(2);
            }
        }

        q
    }

    /// 연결성 검증
    fn verify_connectivity(&self, graph: &LeidenGraph, node_to_community: &[usize]) -> bool {
        let mut communities: HashMap<usize, Vec<usize>> = HashMap::new();
        for (node_idx, &comm) in node_to_community.iter().enumerate() {
            communities.entry(comm).or_default().push(node_idx);
        }

        for (&comm_id, members) in &communities {
            if members.len() <= 1 {
                continue;
            }

            let components = graph.find_connected_components(members, node_to_community, comm_id);

            if components.len() > 1 {
                return false;
            }
        }

        true
    }

    /// 현재 레벨의 커뮤니티 할당 수집
    fn collect_level_communities(
        &self,
        graph: &LeidenGraph,
        node_to_community: &[usize],
    ) -> HashMap<NodeId, usize> {
        let mut result = HashMap::new();
        for (node_idx, &comm) in node_to_community.iter().enumerate() {
            result.insert(graph.nodes[node_idx], comm);
        }
        result
    }

    /// GraphEngine에서 Leiden용 그래프 구조 빌드
    ///
    /// 최적화: all_node_ids()와 all_edge_ids() 사용하여 대량 조회 수행
    async fn build_graph(&self, rel_types: Option<&[String]>) -> Result<LeidenGraph> {
        let mut graph = LeidenGraph::new();

        // 노드 수집 - 대량 조회 사용 (O(1) 대신 벡터 반환)
        let node_ids = self.engine.all_node_ids().await;

        // 노드 인덱스 매핑 구축 (HashMap 예약)
        graph.nodes.reserve(node_ids.len());
        graph.adjacency.reserve(node_ids.len());
        graph.node_weights.reserve(node_ids.len());
        graph.node_to_idx.reserve(node_ids.len());

        for node_id in node_ids {
            let idx = graph.nodes.len();
            graph.node_to_idx.insert(node_id, idx);
            graph.nodes.push(node_id);
            graph.adjacency.push(Vec::new());
            graph.node_weights.push(0.0);
        }

        if graph.nodes.is_empty() {
            return Ok(graph);
        }

        // 엣지 수집 - 대량 조회 사용 (무방향 그래프로 처리)
        let edge_ids = self.engine.all_edge_ids().await;

        for edge_id in edge_ids {
            // 취소 확인 (매 1000개 엣지마다)
            if edge_id % 1000 == 0 && self.is_cancelled() {
                break;
            }

            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(types) = rel_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // 두 노드 모두 유효한지 확인
                if let (Some(&from_idx), Some(&to_idx)) =
                    (graph.node_to_idx.get(&from), graph.node_to_idx.get(&to))
                {
                    // Read edge weight property if present, otherwise default to 1.0
                    let weight = edge
                        .get_property("weight")
                        .and_then(|v| match v {
                            crate::core::Value::Float(f) => Some(*f),
                            crate::core::Value::Integer(i) => Some(*i as f64),
                            _ => None,
                        })
                        .unwrap_or(1.0);

                    // 무방향 그래프: 양방향 추가
                    graph.adjacency[from_idx].push((to_idx, weight));
                    graph.node_weights[from_idx] += weight;
                    graph.total_weight += weight;

                    if from_idx != to_idx {
                        graph.adjacency[to_idx].push((from_idx, weight));
                        graph.node_weights[to_idx] += weight;
                        graph.total_weight += weight;
                    }
                }
            }
        }

        Ok(graph)
    }
}
