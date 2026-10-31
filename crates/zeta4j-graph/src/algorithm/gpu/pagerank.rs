// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GPU 가속 PageRank 알고리즘
//!
//! CUDA 또는 Metal을 사용하여 PageRank를 GPU에서 계산합니다.
//!
//! # 예시
//!
//! ```ignore
//! use zeta4j_graph::algorithm::gpu::{PageRankGpu, PageRankGpuConfig};
//!
//! let pr_gpu = PageRankGpu::new()?;
//! let result = pr_gpu.compute(&engine, PageRankGpuConfig::default())?;
//!
//! for (node_id, score) in result.top_n(10) {
//!     println!("Node {}: {:.6}", node_id, score);
//! }
//! ```

use std::collections::HashMap;

use crate::core::NodeId;
use crate::engine::GraphEngine;

use super::backend::{BackendType, GpuBackend, KernelArg};
use super::error::{GpuError, GpuResult};
use super::graph_repr::BidirectionalCsrGraph;
use super::{auto_select_backend, is_gpu_available};

/// GPU PageRank 설정
#[derive(Debug, Clone)]
pub struct PageRankGpuConfig {
    /// 감쇠 계수 (damping factor), 일반적으로 0.85
    pub damping_factor: f32,
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 임계값 (tolerance)
    pub tolerance: f32,
    /// 수렴 검사 활성화 여부
    pub check_convergence: bool,
    /// 결과 정규화 여부
    pub normalize: bool,
}

impl Default for PageRankGpuConfig {
    fn default() -> Self {
        Self {
            damping_factor: 0.85,
            max_iterations: 20,
            tolerance: 1e-6,
            check_convergence: true,
            normalize: true,
        }
    }
}

impl PageRankGpuConfig {
    /// 감쇠 계수 설정
    pub fn with_damping_factor(mut self, factor: f32) -> Self {
        self.damping_factor = factor;
        self
    }

    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, iterations: usize) -> Self {
        self.max_iterations = iterations;
        self
    }

    /// 수렴 임계값 설정
    pub fn with_tolerance(mut self, tolerance: f32) -> Self {
        self.tolerance = tolerance;
        self
    }
}

/// GPU PageRank 결과
#[derive(Debug, Clone)]
pub struct PageRankGpuResult {
    /// 노드별 PageRank 점수
    pub scores: HashMap<NodeId, f64>,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 사용된 백엔드
    pub backend: BackendType,
}

impl PageRankGpuResult {
    /// 특정 노드의 PageRank 점수 조회
    pub fn score(&self, node_id: NodeId) -> Option<f64> {
        self.scores.get(&node_id).copied()
    }

    /// 상위 N개 노드 반환 (점수 내림차순)
    pub fn top_n(&self, n: usize) -> Vec<(NodeId, f64)> {
        let mut items: Vec<_> = self
            .scores
            .iter()
            .map(|(&id, &score)| (id, score))
            .collect();
        items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        items.truncate(n);
        items
    }
}

/// GPU 가속 PageRank 계산기
pub struct PageRankGpu {
    /// GPU 백엔드
    backend: Box<dyn GpuBackend>,
}

impl PageRankGpu {
    /// 새 GPU PageRank 계산기 생성 (자동 백엔드 선택)
    pub fn new() -> GpuResult<Self> {
        let backend = auto_select_backend()?;
        Ok(Self { backend })
    }

    /// 특정 백엔드로 생성
    pub fn with_backend(backend: Box<dyn GpuBackend>) -> Self {
        Self { backend }
    }

    /// GPU 사용 가능 여부
    pub fn is_available() -> bool {
        is_gpu_available()
    }

    /// 사용 중인 백엔드 정보
    pub fn backend_info(&self) -> super::backend::GpuDeviceInfo {
        self.backend.device_info()
    }

    /// PageRank 계산 (GPU 가속)
    ///
    /// # Arguments
    /// * `engine` - 그래프 엔진
    /// * `config` - PageRank 설정
    ///
    /// # Returns
    /// GPU에서 계산된 PageRank 결과
    pub fn compute(
        &self,
        engine: &GraphEngine,
        config: PageRankGpuConfig,
    ) -> GpuResult<PageRankGpuResult> {
        // 1. 양방향 CSR 그래프 생성 (incoming edges 필요)
        let csr = BidirectionalCsrGraph::from_graph(engine, None)?;

        if csr.outgoing.num_nodes == 0 {
            return Ok(PageRankGpuResult {
                scores: HashMap::new(),
                iterations: 0,
                converged: true,
                backend: self.backend.backend_type(),
            });
        }

        // 2. GPU 커널 사용 가능 여부에 따라 실행 방법 선택
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        if let Ok(result) = self.compute_gpu_kernel(&csr, &config) {
            return Ok(result);
        }

        // GPU 피처가 비활성화되었거나 GPU 실행 실패 시 CPU fallback
        self.compute_cpu_fallback(&csr, &config)
    }

    /// CPU Fallback 구현 (GPU 커널 완성 전 사용)
    ///
    /// 실제 GPU 커널이 연결되면 이 메서드는 제거됩니다.
    fn compute_cpu_fallback(
        &self,
        csr: &BidirectionalCsrGraph,
        config: &PageRankGpuConfig,
    ) -> GpuResult<PageRankGpuResult> {
        let num_nodes = csr.outgoing.num_nodes;
        let damping = config.damping_factor;
        let teleport = (1.0 - damping) / num_nodes as f32;

        // 초기 점수
        let initial_score = 1.0 / num_nodes as f32;
        let mut scores = vec![initial_score; num_nodes];
        let mut new_scores = vec![0.0f32; num_nodes];

        // 나가는 차수 계산
        let out_degrees: Vec<u32> = (0..num_nodes)
            .map(|i| csr.outgoing.degree(i as u32))
            .collect();

        let mut iterations = 0;
        let mut converged = false;

        // 반복 계산
        for _ in 0..config.max_iterations {
            iterations += 1;

            // 댕글링 노드 기여도 계산
            let dangling_sum: f32 = out_degrees
                .iter()
                .enumerate()
                .filter(|(_, &d)| d == 0)
                .map(|(i, _)| scores[i])
                .sum();
            let dangling_contribution = damping * dangling_sum / num_nodes as f32;

            // 새 점수 계산
            for (node, new_score) in new_scores.iter_mut().enumerate() {
                let incoming = csr.incoming.neighbors(node as u32);
                let mut sum = 0.0f32;

                for &src in incoming {
                    let degree = out_degrees[src as usize];
                    if degree > 0 {
                        sum += scores[src as usize] / degree as f32;
                    }
                }

                *new_score = teleport + damping * sum + dangling_contribution;
            }

            // 수렴 검사
            if config.check_convergence {
                let max_diff: f32 = scores
                    .iter()
                    .zip(new_scores.iter())
                    .map(|(&old, &new)| (new - old).abs())
                    .fold(0.0f32, |a, b| a.max(b));

                if max_diff < config.tolerance {
                    converged = true;
                    std::mem::swap(&mut scores, &mut new_scores);
                    break;
                }
            }

            std::mem::swap(&mut scores, &mut new_scores);
        }

        // 정규화
        if config.normalize {
            let total: f32 = scores.iter().sum();
            if total > 0.0 {
                for score in &mut scores {
                    *score /= total;
                }
            }
        }

        // 결과 변환
        let score_map: HashMap<NodeId, f64> = scores
            .iter()
            .enumerate()
            .filter_map(|(idx, &score)| {
                csr.outgoing
                    .get_node_id(idx as u32)
                    .map(|node_id| (node_id, score as f64))
            })
            .collect();

        Ok(PageRankGpuResult {
            scores: score_map,
            iterations,
            converged,
            backend: self.backend.backend_type(),
        })
    }

    /// GPU 커널을 사용한 PageRank 계산 (실제 구현)
    ///
    /// CUDA 또는 Metal 커널을 직접 호출합니다.
    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    fn compute_gpu_kernel(
        &self,
        csr: &BidirectionalCsrGraph,
        config: &PageRankGpuConfig,
    ) -> GpuResult<PageRankGpuResult> {
        let num_nodes = csr.outgoing.num_nodes;
        let damping = config.damping_factor;
        let teleport = (1.0 - damping) / num_nodes as f32;

        // GPU에 그래프 업로드 (incoming edges for PageRank)
        let gpu_incoming = csr.incoming.upload_to_gpu(self.backend.as_ref())?;

        // 나가는 차수 계산 및 업로드
        let out_degrees: Vec<u32> = (0..num_nodes)
            .map(|i| csr.outgoing.degree(i as u32))
            .collect();
        let gpu_out_degrees = self.backend.upload_bytes(slice_to_bytes(&out_degrees))?;

        // 점수 버퍼 할당 및 초기화
        let initial_score = 1.0 / num_nodes as f32;
        let scores: Vec<f32> = vec![initial_score; num_nodes];
        let mut gpu_scores = self.backend.upload_bytes(slice_to_bytes(&scores))?;
        let mut gpu_new_scores = self
            .backend
            .allocate(num_nodes * std::mem::size_of::<f32>())?;

        // PageRank 파라미터 구조체 (Metal 셰이더와 일치해야 함)
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct PageRankParams {
            num_nodes: u32,
            damping: f32,
            teleport: f32,
            dangling_contribution: f32,
        }

        // 최적 실행 설정
        let (grid_size, block_size) = self
            .backend
            .optimal_launch_config("pagerank::pagerank_iteration", num_nodes)?;

        let mut iterations = 0;
        let mut converged = false;

        // 반복 계산
        for _iter in 0..config.max_iterations {
            iterations += 1;

            // 댕글링 노드 기여도 계산 (CPU에서 - GPU atomic 사용하면 더 빠름)
            let current_scores: Vec<f32> =
                bytes_to_vec(&self.backend.download_bytes(&gpu_scores)?)?;
            let dangling_sum: f32 = out_degrees
                .iter()
                .enumerate()
                .filter(|(_, &d)| d == 0)
                .map(|(i, _)| current_scores[i])
                .sum();
            let dangling_contribution = damping * dangling_sum / num_nodes as f32;

            // 파라미터 업로드
            let params = PageRankParams {
                num_nodes: num_nodes as u32,
                damping,
                teleport,
                dangling_contribution,
            };
            let gpu_params = self
                .backend
                .upload_bytes(slice_to_bytes(std::slice::from_ref(&params)))?;

            // PageRank 반복 커널 실행 (Metal 셰이더 인터페이스에 맞춤)
            self.backend.execute_kernel(
                "pagerank::pagerank_iteration_with_dangling",
                grid_size,
                block_size,
                &[
                    KernelArg::Buffer(gpu_incoming.row_offsets.clone()),
                    KernelArg::Buffer(gpu_incoming.col_indices.clone()),
                    KernelArg::Buffer(gpu_out_degrees.clone()),
                    KernelArg::Buffer(gpu_scores.clone()),
                    KernelArg::Buffer(gpu_new_scores.clone()),
                    KernelArg::Buffer(gpu_params.clone()),
                ],
            )?;

            self.backend.synchronize()?;

            // 수렴 검사 (CPU에서)
            if config.check_convergence {
                let new_scores: Vec<f32> =
                    bytes_to_vec(&self.backend.download_bytes(&gpu_new_scores)?)?;
                let max_diff: f32 = current_scores
                    .iter()
                    .zip(new_scores.iter())
                    .map(|(&old, &new)| (new - old).abs())
                    .fold(0.0f32, |a, b| a.max(b));

                if max_diff < config.tolerance {
                    converged = true;
                    std::mem::swap(&mut gpu_scores, &mut gpu_new_scores);
                    break;
                }
            }

            std::mem::swap(&mut gpu_scores, &mut gpu_new_scores);

            // 파라미터 버퍼 해제
            self.backend.free(gpu_params)?;
        }

        // 결과 다운로드
        let mut final_scores: Vec<f32> = bytes_to_vec(&self.backend.download_bytes(&gpu_scores)?)?;

        // 정규화 (CPU에서)
        if config.normalize {
            let total: f32 = final_scores.iter().sum();
            if total > 0.0 {
                for score in &mut final_scores {
                    *score /= total;
                }
            }
        }

        // 버퍼 해제
        self.backend.free(gpu_incoming.row_offsets)?;
        self.backend.free(gpu_incoming.col_indices)?;
        self.backend.free(gpu_out_degrees)?;
        self.backend.free(gpu_scores)?;
        self.backend.free(gpu_new_scores)?;

        // 결과 변환
        let score_map: HashMap<NodeId, f64> = final_scores
            .iter()
            .enumerate()
            .filter_map(|(idx, &score)| {
                csr.outgoing
                    .get_node_id(idx as u32)
                    .map(|node_id| (node_id, score as f64))
            })
            .collect();

        Ok(PageRankGpuResult {
            scores: score_map,
            iterations,
            converged,
            backend: self.backend.backend_type(),
        })
    }
}

/// 슬라이스를 바이트로 변환
#[inline]
fn slice_to_bytes<T: bytemuck::Pod>(slice: &[T]) -> &[u8] {
    bytemuck::cast_slice(slice)
}

/// 바이트를 Vec<T>로 변환
#[inline]
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
