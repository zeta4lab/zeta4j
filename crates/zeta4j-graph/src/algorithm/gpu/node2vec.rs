//! GPU 가속 Node2Vec 랜덤 워크
//!
//! Node2Vec 논문(Grover & Leskovec, 2016)의 편향된 랜덤 워크의 GPU 구현입니다.
//!
//! # 알고리즘
//!
//! p와 q 파라미터로 BFS/DFS 탐색 스타일을 조절합니다:
//! - **p (return parameter)**: 이전 노드로 돌아갈 확률 조절
//! - **q (in-out parameter)**: BFS vs DFS 스타일 조절
//!
//! # 활용
//!
//! - 그래프 임베딩 (Node2Vec, DeepWalk)
//! - 유사 노드 탐색
//! - 추천 시스템

use crate::algorithm::gpu::{CsrGraph, GpuBackend, GpuError, GpuResult};
use crate::core::NodeId;
use rand::SeedableRng;
use rand::prelude::*;
use rand::rngs::StdRng;
use std::collections::HashMap;

/// Node2Vec GPU 설정
#[derive(Debug, Clone)]
pub struct Node2VecGpuConfig {
    /// 워크 길이
    pub walk_length: usize,
    /// 노드당 워크 수
    pub walks_per_node: usize,
    /// Return parameter (p)
    pub p: f64,
    /// In-out parameter (q)
    pub q: f64,
    /// 랜덤 시드 (재현 가능한 결과용)
    pub seed: Option<u64>,
}

impl Default for Node2VecGpuConfig {
    fn default() -> Self {
        Self {
            walk_length: 80,
            walks_per_node: 10,
            p: 1.0,
            q: 1.0,
            seed: None,
        }
    }
}

impl Node2VecGpuConfig {
    /// 워크 길이 설정
    pub fn with_walk_length(mut self, length: usize) -> Self {
        self.walk_length = length.max(1);
        self
    }

    /// 노드당 워크 수 설정
    pub fn with_walks_per_node(mut self, count: usize) -> Self {
        self.walks_per_node = count.max(1);
        self
    }

    /// Return parameter (p) 설정
    pub fn with_p(mut self, p: f64) -> Self {
        self.p = p.max(0.001);
        self
    }

    /// In-out parameter (q) 설정
    pub fn with_q(mut self, q: f64) -> Self {
        self.q = q.max(0.001);
        self
    }

    /// 시드 설정
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

/// Node2Vec GPU 결과
#[derive(Debug, Clone)]
pub struct Node2VecGpuResult {
    /// 워크 경로들
    pub walks: Vec<Vec<NodeId>>,
    /// 총 워크 수
    pub walk_count: usize,
    /// 평균 워크 길이
    pub average_length: f64,
    /// p 파라미터
    pub p: f64,
    /// q 파라미터
    pub q: f64,
    /// 사용된 백엔드 이름
    pub backend: String,
}

impl Node2VecGpuResult {
    /// 노드 등장 빈도
    pub fn node_frequency(&self) -> HashMap<NodeId, usize> {
        let mut freq = HashMap::new();
        for walk in &self.walks {
            for &node in walk {
                *freq.entry(node).or_insert(0) += 1;
            }
        }
        freq
    }

    /// 가장 자주 등장하는 상위 N개 노드
    pub fn top_frequent_nodes(&self, n: usize) -> Vec<(NodeId, usize)> {
        let mut freq: Vec<_> = self.node_frequency().into_iter().collect();
        freq.sort_by(|a, b| b.1.cmp(&a.1));
        freq.truncate(n);
        freq
    }

    /// 엣지 동시출현(co-occurrence) 계산
    pub fn edge_cooccurrence(&self) -> HashMap<(NodeId, NodeId), usize> {
        let mut cooc = HashMap::new();
        for walk in &self.walks {
            for window in walk.windows(2) {
                let (a, b) = (window[0], window[1]);
                let key = if a < b { (a, b) } else { (b, a) };
                *cooc.entry(key).or_insert(0) += 1;
            }
        }
        cooc
    }

    /// 특정 노드가 포함된 워크들
    pub fn walks_containing(&self, node_id: NodeId) -> Vec<&Vec<NodeId>> {
        self.walks
            .iter()
            .filter(|walk| walk.contains(&node_id))
            .collect()
    }
}

/// GPU 가속 Node2Vec
pub struct Node2VecGpu {
    config: Node2VecGpuConfig,
}

impl Node2VecGpu {
    /// 새로운 GPU Node2Vec 생성
    pub fn new() -> Self {
        Self {
            config: Node2VecGpuConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(config: Node2VecGpuConfig) -> Self {
        Self { config }
    }

    /// 단일 시작 노드에서 워크 생성
    pub fn walk_from_node(
        &self,
        backend: &dyn GpuBackend,
        graph: &CsrGraph,
        start_node: NodeId,
    ) -> GpuResult<Node2VecGpuResult> {
        // 단일 노드 워크는 CPU로 처리 (GPU 오버헤드가 더 큼)
        let _ = backend;
        self.walk_from_node_cpu(graph, start_node)
    }

    /// 모든 노드에서 워크 생성
    pub fn generate_walks(
        &self,
        backend: &dyn GpuBackend,
        graph: &CsrGraph,
    ) -> GpuResult<Node2VecGpuResult> {
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        if let Ok(result) = self.generate_walks_gpu(backend, graph) {
            return Ok(result);
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        let _ = backend;

        self.generate_walks_cpu(graph)
    }

    /// GPU 커널로 워크 생성
    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    fn generate_walks_gpu(
        &self,
        backend: &dyn GpuBackend,
        graph: &CsrGraph,
    ) -> GpuResult<Node2VecGpuResult> {
        use crate::algorithm::gpu::backend::KernelArg;

        let num_nodes = graph.num_nodes;
        if num_nodes == 0 {
            return Ok(Node2VecGpuResult {
                walks: Vec::new(),
                walk_count: 0,
                average_length: 0.0,
                p: self.config.p,
                q: self.config.q,
                backend: backend.name().to_string(),
            });
        }

        let num_walks = num_nodes * self.config.walks_per_node;
        let walk_length = self.config.walk_length;
        let total_walk_size = num_walks * walk_length;

        // 그래프 데이터 업로드
        let gpu_row_offsets = backend.upload_bytes(slice_to_bytes(&graph.row_offsets))?;
        let gpu_col_indices = backend.upload_bytes(slice_to_bytes(&graph.col_indices))?;

        // 워크 버퍼 할당
        let gpu_walks = backend.allocate(total_walk_size * std::mem::size_of::<u32>())?;
        let gpu_walk_lengths = backend.allocate(num_walks * std::mem::size_of::<u32>())?;

        // 시작 노드 배열 생성 (노드 0,0,0...,1,1,1..., 각 walks_per_node번)
        let start_nodes: Vec<u32> = (0..num_nodes)
            .flat_map(|node| std::iter::repeat_n(node as u32, self.config.walks_per_node))
            .collect();
        let gpu_start_nodes = backend.upload_bytes(slice_to_bytes(&start_nodes))?;

        // 랜덤 시드
        let seed = self.config.seed.unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
        }) as u32;

        // 최적 실행 설정
        let (grid_size, block_size) = backend.optimal_launch_config("node2vec", num_walks)?;

        // p=q=1이면 DeepWalk 커널 사용 (최적화)
        let is_deepwalk =
            (self.config.p - 1.0).abs() < 0.001 && (self.config.q - 1.0).abs() < 0.001;

        if is_deepwalk {
            // DeepWalk 커널 실행
            backend.execute_kernel(
                "node2vec::deepwalk_random_walk",
                grid_size,
                block_size,
                &[
                    KernelArg::Buffer(gpu_row_offsets.clone()),
                    KernelArg::Buffer(gpu_col_indices.clone()),
                    KernelArg::Buffer(gpu_walks.clone()),
                    KernelArg::Buffer(gpu_walk_lengths.clone()),
                    KernelArg::Buffer(gpu_start_nodes.clone()),
                    KernelArg::UInt32(num_walks as u32),
                    KernelArg::UInt32(walk_length as u32),
                    KernelArg::UInt32(seed),
                ],
            )?;
        } else {
            // Node2Vec 커널 실행
            backend.execute_kernel(
                "node2vec::node2vec_random_walk",
                grid_size,
                block_size,
                &[
                    KernelArg::Buffer(gpu_row_offsets.clone()),
                    KernelArg::Buffer(gpu_col_indices.clone()),
                    KernelArg::Buffer(gpu_walks.clone()),
                    KernelArg::Buffer(gpu_walk_lengths.clone()),
                    KernelArg::Buffer(gpu_start_nodes.clone()),
                    KernelArg::UInt32(num_walks as u32),
                    KernelArg::UInt32(walk_length as u32),
                    KernelArg::Float(self.config.p as f32),
                    KernelArg::Float(self.config.q as f32),
                    KernelArg::UInt32(seed),
                ],
            )?;
        }

        backend.synchronize()?;

        // 결과 다운로드
        let walks_bytes = backend.download_bytes(&gpu_walks)?;
        let walks_data: Vec<u32> = bytes_to_vec(&walks_bytes)?;

        let lengths_bytes = backend.download_bytes(&gpu_walk_lengths)?;
        let lengths_data: Vec<u32> = bytes_to_vec(&lengths_bytes)?;

        // 버퍼 해제
        backend.free(gpu_row_offsets)?;
        backend.free(gpu_col_indices)?;
        backend.free(gpu_walks)?;
        backend.free(gpu_walk_lengths)?;
        backend.free(gpu_start_nodes)?;

        // 결과 변환 (인덱스 -> NodeId)
        let mut walks = Vec::with_capacity(num_walks);
        let mut total_length = 0usize;

        for (walk_idx, &len) in lengths_data.iter().enumerate() {
            let length = len as usize;
            total_length += length;

            let walk_start = walk_idx * walk_length;
            let walk: Vec<NodeId> = (0..length)
                .filter_map(|i| {
                    let idx = walks_data[walk_start + i] as usize;
                    if idx < graph.index_to_node.len() {
                        Some(graph.index_to_node[idx])
                    } else {
                        None
                    }
                })
                .collect();

            if !walk.is_empty() {
                walks.push(walk);
            }
        }

        let average_length = if walks.is_empty() {
            0.0
        } else {
            total_length as f64 / walks.len() as f64
        };

        Ok(Node2VecGpuResult {
            walk_count: walks.len(),
            walks,
            average_length,
            p: self.config.p,
            q: self.config.q,
            backend: backend.name().to_string(),
        })
    }

    /// 단일 노드에서 워크 생성 (CPU Fallback)
    fn walk_from_node_cpu(
        &self,
        graph: &CsrGraph,
        start_node: NodeId,
    ) -> GpuResult<Node2VecGpuResult> {
        let mut rng = match self.config.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        };

        // 시작 노드의 인덱스 찾기
        let start_idx = graph
            .node_to_index
            .get(&start_node)
            .map(|&idx| idx as usize)
            .unwrap_or(0);

        let mut walks = Vec::with_capacity(self.config.walks_per_node);

        for _ in 0..self.config.walks_per_node {
            let walk = self.single_walk(graph, start_idx, &mut rng);
            walks.push(walk);
        }

        let total_length: usize = walks.iter().map(|w| w.len()).sum();
        let average_length = if walks.is_empty() {
            0.0
        } else {
            total_length as f64 / walks.len() as f64
        };

        Ok(Node2VecGpuResult {
            walks,
            walk_count: self.config.walks_per_node,
            average_length,
            p: self.config.p,
            q: self.config.q,
            backend: "CPU Fallback".to_string(),
        })
    }

    /// 모든 노드에서 워크 생성 (CPU Fallback)
    fn generate_walks_cpu(&self, graph: &CsrGraph) -> GpuResult<Node2VecGpuResult> {
        let num_nodes = graph.num_nodes;

        if num_nodes == 0 {
            return Ok(Node2VecGpuResult {
                walks: Vec::new(),
                walk_count: 0,
                average_length: 0.0,
                p: self.config.p,
                q: self.config.q,
                backend: "CPU Fallback".to_string(),
            });
        }

        let mut rng = match self.config.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        };

        let mut all_walks = Vec::with_capacity(num_nodes * self.config.walks_per_node);

        for node_idx in 0..num_nodes {
            for _ in 0..self.config.walks_per_node {
                let walk = self.single_walk(graph, node_idx, &mut rng);
                all_walks.push(walk);
            }
        }

        // 워크 셔플 (학습 배치 다양성을 위해)
        all_walks.shuffle(&mut rng);

        let total_length: usize = all_walks.iter().map(|w| w.len()).sum();
        let average_length = if all_walks.is_empty() {
            0.0
        } else {
            total_length as f64 / all_walks.len() as f64
        };

        Ok(Node2VecGpuResult {
            walk_count: all_walks.len(),
            walks: all_walks,
            average_length,
            p: self.config.p,
            q: self.config.q,
            backend: "CPU Fallback".to_string(),
        })
    }

    /// 단일 워크 수행
    fn single_walk(&self, graph: &CsrGraph, start_idx: usize, rng: &mut StdRng) -> Vec<NodeId> {
        let mut walk = Vec::with_capacity(self.config.walk_length);
        let mut current_idx = start_idx;
        walk.push(graph.index_to_node[current_idx]);

        let mut prev_idx: Option<usize> = None;

        for _ in 1..self.config.walk_length {
            let start = graph.row_offsets[current_idx];
            let end = graph.row_offsets[current_idx + 1];

            if start >= end {
                break; // 이웃이 없으면 종료
            }

            let neighbors: Vec<usize> = (start as usize..end as usize)
                .map(|i| graph.col_indices[i] as usize)
                .filter(|&n| n < graph.num_nodes)
                .collect();

            if neighbors.is_empty() {
                break;
            }

            // 전이 가중치 계산
            let weights = self.calculate_weights(graph, prev_idx, current_idx, &neighbors);

            // 가중치 기반 랜덤 선택
            let next_idx = self.weighted_random_choice(&neighbors, &weights, rng);

            walk.push(graph.index_to_node[next_idx]);
            prev_idx = Some(current_idx);
            current_idx = next_idx;
        }

        walk
    }

    /// Node2Vec 전이 가중치 계산
    fn calculate_weights(
        &self,
        graph: &CsrGraph,
        prev_idx: Option<usize>,
        _current_idx: usize,
        neighbors: &[usize],
    ) -> Vec<f64> {
        let prev = match prev_idx {
            Some(p) => p,
            None => return vec![1.0; neighbors.len()], // 첫 스텝은 균등
        };

        // 이전 노드의 이웃 집합
        let prev_start = graph.row_offsets[prev] as usize;
        let prev_end = graph.row_offsets[prev + 1] as usize;
        let prev_neighbors: std::collections::HashSet<usize> = (prev_start..prev_end)
            .map(|i| graph.col_indices[i] as usize)
            .collect();

        neighbors
            .iter()
            .map(|&neighbor| {
                if neighbor == prev {
                    // 이전 노드로 돌아가기: 1/p
                    1.0 / self.config.p
                } else if prev_neighbors.contains(&neighbor) {
                    // 이전 노드와도 연결된 노드: 1
                    1.0
                } else {
                    // 이전 노드와 연결 안 된 노드: 1/q
                    1.0 / self.config.q
                }
            })
            .collect()
    }

    /// 가중치 기반 랜덤 선택
    fn weighted_random_choice(&self, items: &[usize], weights: &[f64], rng: &mut StdRng) -> usize {
        let total: f64 = weights.iter().sum();
        if total <= 0.0 || items.is_empty() {
            return items.first().copied().unwrap_or(0);
        }

        let threshold: f64 = rng.random::<f64>() * total;
        let mut cumulative = 0.0;

        for (i, &weight) in weights.iter().enumerate() {
            cumulative += weight;
            if cumulative >= threshold {
                return items[i];
            }
        }

        *items.last().unwrap_or(&0)
    }
}

impl Default for Node2VecGpu {
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
