//! GPU 가속 커뮤니티 탐지 알고리즘
//!
//! Label Propagation 및 Triangle Counting의 GPU 구현입니다.
//!
//! # 지원 알고리즘
//!
//! - **Label Propagation**: 빠른 커뮤니티 탐지
//! - **Triangle Counting**: 삼각형 개수 계산
//! - **Clustering Coefficient**: 클러스터링 계수 계산

use crate::algorithm::gpu::{CsrGraph, GpuBackend, GpuError, GpuResult};
use crate::core::NodeId;
use std::collections::HashMap;

/// Label Propagation GPU 설정
#[derive(Debug, Clone)]
pub struct LabelPropagationGpuConfig {
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 임계값 (변경 비율, 0.0이면 완전 수렴까지)
    pub convergence_threshold: f64,
    /// 가중치 사용 여부
    pub use_weights: bool,
}

impl Default for LabelPropagationGpuConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            convergence_threshold: 0.0,
            use_weights: false,
        }
    }
}

impl LabelPropagationGpuConfig {
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

    /// 가중치 사용 설정
    pub fn with_weights(mut self, use_weights: bool) -> Self {
        self.use_weights = use_weights;
        self
    }
}

/// Label Propagation GPU 결과
#[derive(Debug, Clone)]
pub struct LabelPropagationGpuResult {
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
    /// 사용된 백엔드 이름
    pub backend: String,
}

impl LabelPropagationGpuResult {
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
}

/// GPU 가속 Label Propagation
pub struct LabelPropagationGpu {
    config: LabelPropagationGpuConfig,
}

impl LabelPropagationGpu {
    /// 새로운 GPU Label Propagation 생성
    pub fn new() -> Self {
        Self {
            config: LabelPropagationGpuConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(config: LabelPropagationGpuConfig) -> Self {
        Self { config }
    }

    /// 커뮤니티 탐지 실행
    pub fn detect(
        &self,
        backend: &dyn GpuBackend,
        graph: &CsrGraph,
    ) -> GpuResult<LabelPropagationGpuResult> {
        let num_nodes = graph.num_nodes;

        if num_nodes == 0 {
            return Ok(LabelPropagationGpuResult {
                labels: HashMap::new(),
                communities: HashMap::new(),
                community_count: 0,
                iterations: 0,
                converged: true,
                backend: "CPU Fallback".to_string(),
            });
        }

        // GPU 커널 사용 가능 여부에 따라 실행 방법 선택
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        match self.detect_gpu(backend, graph) {
            Ok(result) => return Ok(result),
            Err(e) => {
                eprintln!("[LPA GPU] Kernel failed, falling back to CPU: {:?}", e);
            }
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        let _ = backend;

        self.detect_cpu(graph)
    }

    /// GPU 커널을 사용한 Label Propagation
    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    fn detect_gpu(
        &self,
        backend: &dyn GpuBackend,
        graph: &CsrGraph,
    ) -> GpuResult<LabelPropagationGpuResult> {
        use super::backend::KernelArg;

        let num_nodes = graph.num_nodes;

        // GPU에 그래프 업로드
        let gpu_row_offsets = backend.upload_bytes(slice_to_bytes(&graph.row_offsets))?;
        let gpu_col_indices = backend.upload_bytes(slice_to_bytes(&graph.col_indices))?;

        // 레이블 버퍼 할당 및 초기화
        let labels: Vec<u32> = (0..num_nodes as u32).collect();
        let mut gpu_labels = backend.upload_bytes(slice_to_bytes(&labels))?;
        let mut gpu_new_labels = backend.allocate(num_nodes * std::mem::size_of::<u32>())?;

        // 변경 플래그 버퍼 (atomic int)
        let changed_init: Vec<i32> = vec![0];
        let gpu_changed = backend.upload_bytes(slice_to_bytes(&changed_init))?;

        let (grid_size, block_size) =
            backend.optimal_launch_config("community::lpa_update_labels", num_nodes)?;

        let mut iterations = 0;
        let mut converged = false;

        while iterations < self.config.max_iterations && !converged {
            iterations += 1;

            // 변경 플래그 초기화
            backend.execute_kernel(
                "community::clear_changed_flag",
                [1, 1, 1],
                [1, 1, 1],
                &[KernelArg::Buffer(gpu_changed.clone())],
            )?;

            // 레이블 업데이트 커널 실행
            backend.execute_kernel(
                "community::lpa_update_labels",
                grid_size,
                block_size,
                &[
                    KernelArg::Buffer(gpu_row_offsets.clone()),
                    KernelArg::Buffer(gpu_col_indices.clone()),
                    KernelArg::Buffer(gpu_labels.clone()),
                    KernelArg::Buffer(gpu_new_labels.clone()),
                    KernelArg::UInt32(num_nodes as u32),
                    KernelArg::Buffer(gpu_changed.clone()),
                ],
            )?;

            backend.synchronize()?;

            // 변경 여부 확인
            let changed_bytes = backend.download_bytes(&gpu_changed)?;
            let changed_result: Vec<i32> = bytes_to_vec(&changed_bytes)?;
            let Some(changed) = changed_result.first() else {
                return Err(GpuError::InvalidArgument(
                    "GPU returned an empty convergence buffer".to_string(),
                ));
            };
            if *changed == 0 {
                converged = true;
            }

            // 레이블 복사
            backend.execute_kernel(
                "community::lpa_copy_labels",
                grid_size,
                block_size,
                &[
                    KernelArg::Buffer(gpu_labels.clone()),
                    KernelArg::Buffer(gpu_new_labels.clone()),
                    KernelArg::UInt32(num_nodes as u32),
                ],
            )?;

            std::mem::swap(&mut gpu_labels, &mut gpu_new_labels);
        }

        // 결과 다운로드
        let labels_bytes = backend.download_bytes(&gpu_labels)?;
        let final_labels: Vec<u32> = bytes_to_vec(&labels_bytes)?;

        // 버퍼 해제
        backend.free(gpu_row_offsets)?;
        backend.free(gpu_col_indices)?;
        backend.free(gpu_labels)?;
        backend.free(gpu_new_labels)?;
        backend.free(gpu_changed)?;

        // 결과 변환
        let mut node_labels = HashMap::new();
        let mut communities: HashMap<usize, Vec<NodeId>> = HashMap::new();
        let mut label_remap: HashMap<u32, usize> = HashMap::new();
        let mut next_label = 0;

        for (node, &label) in final_labels.iter().enumerate() {
            let normalized = *label_remap.entry(label).or_insert_with(|| {
                let l = next_label;
                next_label += 1;
                l
            });

            let node_id = graph.index_to_node[node];
            node_labels.insert(node_id, normalized);
            communities.entry(normalized).or_default().push(node_id);
        }

        for nodes in communities.values_mut() {
            nodes.sort();
        }

        Ok(LabelPropagationGpuResult {
            labels: node_labels,
            community_count: communities.len(),
            communities,
            iterations,
            converged,
            backend: "Metal GPU".to_string(),
        })
    }

    /// CPU Fallback 구현
    fn detect_cpu(&self, graph: &CsrGraph) -> GpuResult<LabelPropagationGpuResult> {
        let num_nodes = graph.num_nodes;

        // 초기 레이블 할당 (각 노드에 고유 레이블)
        let mut labels: Vec<usize> = (0..num_nodes).collect();
        let mut new_labels = labels.clone();

        let mut iterations = 0;
        let mut converged = false;

        while iterations < self.config.max_iterations && !converged {
            iterations += 1;
            let mut changed = false;

            for node in 0..num_nodes {
                let start = graph.row_offsets[node];
                let end = graph.row_offsets[node + 1];

                if start >= end {
                    new_labels[node] = labels[node];
                    continue;
                }

                // 이웃들의 레이블 카운트
                let mut label_counts: HashMap<usize, usize> = HashMap::new();
                for i in start as usize..end as usize {
                    let neighbor = graph.col_indices[i] as usize;
                    if neighbor < num_nodes {
                        *label_counts.entry(labels[neighbor]).or_insert(0) += 1;
                    }
                }

                // 가장 빈번한 레이블 선택 (동률 시 작은 레이블)
                let best_label = label_counts
                    .into_iter()
                    .max_by(|(l1, c1), (l2, c2)| c1.cmp(c2).then_with(|| l2.cmp(l1)))
                    .map(|(label, _)| label)
                    .unwrap_or(labels[node]);

                new_labels[node] = best_label;

                if labels[node] != best_label {
                    changed = true;
                }
            }

            // 레이블 복사
            labels.copy_from_slice(&new_labels);

            if !changed {
                converged = true;
            }
        }

        // 결과 변환
        let mut node_labels = HashMap::new();
        let mut communities: HashMap<usize, Vec<NodeId>> = HashMap::new();

        // 레이블 정규화 (0부터 연속)
        let mut label_remap: HashMap<usize, usize> = HashMap::new();
        let mut next_label = 0;

        for (node, &label) in labels.iter().enumerate() {
            let normalized = *label_remap.entry(label).or_insert_with(|| {
                let l = next_label;
                next_label += 1;
                l
            });

            let node_id = graph.index_to_node[node];
            node_labels.insert(node_id, normalized);
            communities.entry(normalized).or_default().push(node_id);
        }

        // 커뮤니티 내 노드 정렬
        for nodes in communities.values_mut() {
            nodes.sort();
        }

        Ok(LabelPropagationGpuResult {
            labels: node_labels,
            community_count: communities.len(),
            communities,
            iterations,
            converged,
            backend: "CPU Fallback".to_string(),
        })
    }
}

impl Default for LabelPropagationGpu {
    fn default() -> Self {
        Self::new()
    }
}

/// Triangle Counting GPU 설정
#[derive(Debug, Clone)]
pub struct TriangleCountingGpuConfig {
    /// 노드별 삼각형 수 계산 여부
    pub compute_per_node: bool,
    /// 클러스터링 계수 계산 여부
    pub compute_clustering_coefficient: bool,
}

impl Default for TriangleCountingGpuConfig {
    fn default() -> Self {
        Self {
            compute_per_node: true,
            compute_clustering_coefficient: false,
        }
    }
}

impl TriangleCountingGpuConfig {
    /// 노드별 삼각형 수 계산 설정
    pub fn with_per_node(mut self, enabled: bool) -> Self {
        self.compute_per_node = enabled;
        self
    }

    /// 클러스터링 계수 계산 설정
    pub fn with_clustering_coefficient(mut self, enabled: bool) -> Self {
        self.compute_clustering_coefficient = enabled;
        self
    }
}

/// Triangle Counting GPU 결과
#[derive(Debug, Clone)]
pub struct TriangleCountingGpuResult {
    /// 전체 삼각형 수
    pub total_triangles: u64,
    /// 노드별 삼각형 수 (옵션)
    pub node_triangles: Option<HashMap<NodeId, u64>>,
    /// 노드별 클러스터링 계수 (옵션)
    pub clustering_coefficients: Option<HashMap<NodeId, f64>>,
    /// 평균 클러스터링 계수
    pub average_clustering_coefficient: f64,
    /// 사용된 백엔드 이름
    pub backend: String,
}

impl TriangleCountingGpuResult {
    /// 특정 노드의 삼각형 수 조회
    pub fn node_count(&self, node_id: NodeId) -> Option<u64> {
        self.node_triangles.as_ref()?.get(&node_id).copied()
    }

    /// 특정 노드의 클러스터링 계수 조회
    pub fn clustering_coefficient(&self, node_id: NodeId) -> Option<f64> {
        self.clustering_coefficients
            .as_ref()?
            .get(&node_id)
            .copied()
    }
}

/// GPU 가속 Triangle Counting
pub struct TriangleCountingGpu {
    config: TriangleCountingGpuConfig,
}

impl TriangleCountingGpu {
    /// 새로운 GPU Triangle Counting 생성
    pub fn new() -> Self {
        Self {
            config: TriangleCountingGpuConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(config: TriangleCountingGpuConfig) -> Self {
        Self { config }
    }

    /// 삼각형 계수 실행
    pub fn count(
        &self,
        backend: &dyn GpuBackend,
        graph: &CsrGraph,
    ) -> GpuResult<TriangleCountingGpuResult> {
        let num_nodes = graph.num_nodes;

        if num_nodes == 0 {
            return Ok(TriangleCountingGpuResult {
                total_triangles: 0,
                node_triangles: Some(HashMap::new()),
                clustering_coefficients: Some(HashMap::new()),
                average_clustering_coefficient: 0.0,
                backend: "CPU Fallback".to_string(),
            });
        }

        // GPU 커널 사용 가능 여부에 따라 실행 방법 선택
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        if let Ok(result) = self.count_gpu(backend, graph) {
            return Ok(result);
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        let _ = backend;

        self.count_cpu(graph)
    }

    /// GPU 커널을 사용한 Triangle Counting
    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    fn count_gpu(
        &self,
        backend: &dyn GpuBackend,
        graph: &CsrGraph,
    ) -> GpuResult<TriangleCountingGpuResult> {
        use super::backend::KernelArg;

        let num_nodes = graph.num_nodes;

        // GPU에 그래프 업로드
        let gpu_row_offsets = backend.upload_bytes(slice_to_bytes(&graph.row_offsets))?;
        let gpu_col_indices = backend.upload_bytes(slice_to_bytes(&graph.col_indices))?;

        // 노드별 삼각형 수 버퍼
        let gpu_triangle_counts = backend.allocate(num_nodes * std::mem::size_of::<u64>())?;

        // 전체 합 버퍼 (atomic)
        let total_init: Vec<u64> = vec![0];
        let gpu_total = backend.upload_bytes(slice_to_bytes(&total_init))?;

        let (grid_size, block_size) =
            backend.optimal_launch_config("community::triangle_count_per_node", num_nodes)?;

        // 노드별 삼각형 수 계산
        backend.execute_kernel(
            "community::triangle_count_per_node",
            grid_size,
            block_size,
            &[
                KernelArg::Buffer(gpu_row_offsets.clone()),
                KernelArg::Buffer(gpu_col_indices.clone()),
                KernelArg::Buffer(gpu_triangle_counts.clone()),
                KernelArg::UInt32(num_nodes as u32),
            ],
        )?;

        backend.synchronize()?;

        // 결과 다운로드
        let counts_bytes = backend.download_bytes(&gpu_triangle_counts)?;
        let node_counts: Vec<u64> = bytes_to_vec(&counts_bytes)?;

        // 전체 삼각형 수 계산 (CPU에서 - 간단함)
        let total_triangles: u64 = node_counts.iter().sum();

        // 클러스터링 계수 계산 (CPU에서)
        let mut clustering_coefficients = HashMap::new();
        let mut avg_cc = 0.0;
        let mut valid_nodes = 0;

        if self.config.compute_clustering_coefficient {
            for (node, &count) in node_counts.iter().enumerate() {
                let degree = (graph.row_offsets[node + 1] - graph.row_offsets[node]) as u64;
                if degree >= 2 {
                    let possible = degree * (degree - 1) / 2;
                    let cc = count as f64 / possible as f64;
                    let node_id = graph.index_to_node[node];
                    clustering_coefficients.insert(node_id, cc);
                    avg_cc += cc;
                    valid_nodes += 1;
                } else {
                    let node_id = graph.index_to_node[node];
                    clustering_coefficients.insert(node_id, 0.0);
                }
            }
            if valid_nodes > 0 {
                avg_cc /= valid_nodes as f64;
            }
        }

        // 노드별 삼각형 수 변환
        let node_triangles: Option<HashMap<NodeId, u64>> = if self.config.compute_per_node {
            Some(
                node_counts
                    .iter()
                    .enumerate()
                    .map(|(i, &c)| (graph.index_to_node[i], c))
                    .collect(),
            )
        } else {
            None
        };

        // 버퍼 해제
        backend.free(gpu_row_offsets)?;
        backend.free(gpu_col_indices)?;
        backend.free(gpu_triangle_counts)?;
        backend.free(gpu_total)?;

        Ok(TriangleCountingGpuResult {
            total_triangles,
            node_triangles,
            clustering_coefficients: if self.config.compute_clustering_coefficient {
                Some(clustering_coefficients)
            } else {
                None
            },
            average_clustering_coefficient: avg_cc,
            backend: "Metal GPU".to_string(),
        })
    }

    /// CPU Fallback 구현
    fn count_cpu(&self, graph: &CsrGraph) -> GpuResult<TriangleCountingGpuResult> {
        let num_nodes = graph.num_nodes;

        // 이웃 집합 미리 계산
        let mut neighbors: Vec<std::collections::HashSet<usize>> = Vec::with_capacity(num_nodes);
        for node in 0..num_nodes {
            let start = graph.row_offsets[node] as usize;
            let end = graph.row_offsets[node + 1] as usize;
            let mut neighbor_set = std::collections::HashSet::new();
            for i in start..end {
                let neighbor = graph.col_indices[i] as usize;
                if neighbor < num_nodes {
                    neighbor_set.insert(neighbor);
                }
            }
            neighbors.push(neighbor_set);
        }

        // 노드별 삼각형 수 계산
        let mut node_triangles: Vec<u64> = vec![0; num_nodes];
        let mut total_triangles: u64 = 0;

        for u in 0..num_nodes {
            let start_u = graph.row_offsets[u] as usize;
            let end_u = graph.row_offsets[u + 1] as usize;

            for i in start_u..end_u {
                let v = graph.col_indices[i] as usize;
                if v <= u || v >= num_nodes {
                    continue;
                }

                // u와 v의 공통 이웃 찾기
                let common_count = neighbors[u]
                    .intersection(&neighbors[v])
                    .filter(|&&w| w > v)
                    .count() as u64;

                node_triangles[u] += common_count;
                node_triangles[v] += common_count;

                // 공통 이웃들에게도 카운트 추가
                for &w in neighbors[u].intersection(&neighbors[v]) {
                    if w > v {
                        node_triangles[w] += 1;
                    }
                }

                total_triangles += common_count;
            }
        }

        // 결과 변환
        let mut result_node_triangles = HashMap::new();
        let mut result_clustering = HashMap::new();
        let mut total_cc = 0.0;
        let mut valid_nodes = 0;

        for (node, &triangles) in node_triangles.iter().enumerate() {
            let node_id = graph.index_to_node[node];

            if self.config.compute_per_node {
                result_node_triangles.insert(node_id, triangles);
            }

            if self.config.compute_clustering_coefficient {
                let degree = (graph.row_offsets[node + 1] - graph.row_offsets[node]) as usize;
                let cc = if degree >= 2 {
                    let possible = (degree * (degree - 1)) / 2;
                    triangles as f64 / possible as f64
                } else {
                    0.0
                };
                result_clustering.insert(node_id, cc);
                total_cc += cc;
                valid_nodes += 1;
            }
        }

        let avg_cc = if valid_nodes > 0 {
            total_cc / valid_nodes as f64
        } else {
            0.0
        };

        Ok(TriangleCountingGpuResult {
            total_triangles,
            node_triangles: if self.config.compute_per_node {
                Some(result_node_triangles)
            } else {
                None
            },
            clustering_coefficients: if self.config.compute_clustering_coefficient {
                Some(result_clustering)
            } else {
                None
            },
            average_clustering_coefficient: avg_cc,
            backend: "CPU Fallback".to_string(),
        })
    }
}

impl Default for TriangleCountingGpu {
    fn default() -> Self {
        Self::new()
    }
}

/// 슬라이스를 바이트로 변환
#[inline]
#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
fn slice_to_bytes<T: bytemuck::Pod>(slice: &[T]) -> &[u8] {
    bytemuck::cast_slice(slice)
}

/// 바이트를 Vec<T>로 변환
#[inline]
#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
fn bytes_to_vec<T: bytemuck::Pod>(bytes: &[u8]) -> GpuResult<Vec<T>> {
    let element_size = std::mem::size_of::<T>();
    if element_size == 0 || !bytes.len().is_multiple_of(element_size) {
        return Err(GpuError::InvalidArgument(format!(
            "{} GPU bytes cannot be decoded as {}-byte elements",
            bytes.len(),
            element_size
        )));
    }
    Ok(bytemuck::allocation::pod_collect_to_vec(bytes))
}
