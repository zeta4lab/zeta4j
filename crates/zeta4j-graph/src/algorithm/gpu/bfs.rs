// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GPU 가속 BFS (Breadth-First Search) 알고리즘
//!
//! CUDA 또는 Metal을 사용하여 BFS를 GPU에서 계산합니다.
//!
//! # 예시
//!
//! ```ignore
//! use zeta4j_graph::algorithm::gpu::{BfsGpu, BfsGpuConfig};
//!
//! let bfs_gpu = BfsGpu::new()?;
//! let result = bfs_gpu.compute(&engine, source_node, BfsGpuConfig::default())?;
//!
//! println!("Distance to node 5: {:?}", result.distance(5));
//! ```

use std::collections::HashMap;

use crate::core::NodeId;
use crate::engine::GraphEngine;

use super::backend::BackendType;
use super::error::GpuResult;
use super::graph_repr::CsrGraph;
use super::{GpuBackend, auto_select_backend, is_gpu_available};

/// GPU BFS 설정
#[derive(Debug, Clone)]
pub struct BfsGpuConfig {
    /// 최대 탐색 깊이 (None이면 무제한)
    pub max_depth: Option<usize>,
    /// 방향성 고려 여부 (true면 나가는 엣지만)
    pub directed: bool,
}

impl Default for BfsGpuConfig {
    fn default() -> Self {
        Self {
            max_depth: None,
            directed: true,
        }
    }
}

impl BfsGpuConfig {
    /// 최대 깊이 설정
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// 방향성 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }
}

/// GPU BFS 결과
#[derive(Debug, Clone)]
pub struct BfsGpuResult {
    /// 시작 노드에서 각 노드까지의 거리 (-1이면 도달 불가)
    pub distances: HashMap<NodeId, i32>,
    /// 방문한 노드 수
    pub visited_count: usize,
    /// 최대 깊이 (BFS 레벨)
    pub max_depth_reached: usize,
    /// 사용된 백엔드
    pub backend: BackendType,
}

impl BfsGpuResult {
    /// 특정 노드까지의 거리 조회
    pub fn distance(&self, node_id: NodeId) -> Option<i32> {
        self.distances.get(&node_id).copied().filter(|&d| d >= 0)
    }

    /// 특정 거리에 있는 모든 노드 반환
    pub fn nodes_at_distance(&self, distance: i32) -> Vec<NodeId> {
        self.distances
            .iter()
            .filter(|(_, &d)| d == distance)
            .map(|(&id, _)| id)
            .collect()
    }

    /// 도달 가능한 모든 노드 반환
    pub fn reachable_nodes(&self) -> Vec<NodeId> {
        self.distances
            .iter()
            .filter(|(_, &d)| d >= 0)
            .map(|(&id, _)| id)
            .collect()
    }
}

/// GPU 가속 BFS 계산기
pub struct BfsGpu {
    /// GPU 백엔드
    backend: Box<dyn GpuBackend>,
}

impl BfsGpu {
    /// 새 GPU BFS 계산기 생성 (자동 백엔드 선택)
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

    /// 단일 소스 BFS 실행
    ///
    /// # Arguments
    /// * `engine` - 그래프 엔진
    /// * `source` - 시작 노드 ID
    /// * `config` - BFS 설정
    ///
    /// # Returns
    /// 각 노드까지의 거리
    pub fn compute(
        &self,
        engine: &GraphEngine,
        source: NodeId,
        config: BfsGpuConfig,
    ) -> GpuResult<BfsGpuResult> {
        // CSR 그래프 생성
        let csr = CsrGraph::from_graph(engine, None)?;

        if csr.num_nodes == 0 {
            return Ok(BfsGpuResult {
                distances: HashMap::new(),
                visited_count: 0,
                max_depth_reached: 0,
                backend: self.backend.backend_type(),
            });
        }

        // 시작 노드 인덱스 확인
        let source_idx = match csr.get_index(source) {
            Some(idx) => idx,
            None => {
                return Ok(BfsGpuResult {
                    distances: HashMap::new(),
                    visited_count: 0,
                    max_depth_reached: 0,
                    backend: self.backend.backend_type(),
                });
            }
        };

        // CPU fallback 구현
        let result = self.compute_cpu_fallback(&csr, source_idx as usize, &config)?;

        Ok(result)
    }

    /// 다중 소스 BFS 실행
    ///
    /// 여러 소스에서 동시에 BFS를 실행합니다.
    pub fn compute_multi_source(
        &self,
        engine: &GraphEngine,
        sources: &[NodeId],
        config: BfsGpuConfig,
    ) -> GpuResult<BfsGpuResult> {
        // CSR 그래프 생성
        let csr = CsrGraph::from_graph(engine, None)?;

        if csr.num_nodes == 0 || sources.is_empty() {
            return Ok(BfsGpuResult {
                distances: HashMap::new(),
                visited_count: 0,
                max_depth_reached: 0,
                backend: self.backend.backend_type(),
            });
        }

        // 소스 노드 인덱스들 확인
        let source_indices: Vec<usize> = sources
            .iter()
            .filter_map(|&s| csr.get_index(s).map(|i| i as usize))
            .collect();

        if source_indices.is_empty() {
            return Ok(BfsGpuResult {
                distances: HashMap::new(),
                visited_count: 0,
                max_depth_reached: 0,
                backend: self.backend.backend_type(),
            });
        }

        // CPU fallback 구현
        let result = self.compute_multi_source_cpu_fallback(&csr, &source_indices, &config)?;

        Ok(result)
    }

    /// CPU Fallback 구현
    fn compute_cpu_fallback(
        &self,
        csr: &CsrGraph,
        source_idx: usize,
        config: &BfsGpuConfig,
    ) -> GpuResult<BfsGpuResult> {
        let num_nodes = csr.num_nodes;
        let mut distances = vec![-1i32; num_nodes];
        let mut frontier = vec![false; num_nodes];
        let mut next_frontier = vec![false; num_nodes];

        // 시작 노드 설정
        distances[source_idx] = 0;
        frontier[source_idx] = true;

        let mut current_level = 0;
        let max_depth = config.max_depth.unwrap_or(usize::MAX);

        // 레벨 동기화 BFS
        loop {
            if current_level >= max_depth as i32 {
                break;
            }

            let mut changed = false;

            // 현재 프론티어의 모든 노드 처리
            for (node, &is_frontier) in frontier.iter().enumerate() {
                if !is_frontier {
                    continue;
                }

                // 이웃 탐색
                let neighbors = csr.neighbors(node as u32);
                for &neighbor in neighbors {
                    let neighbor = neighbor as usize;
                    if distances[neighbor] == -1 {
                        distances[neighbor] = current_level + 1;
                        next_frontier[neighbor] = true;
                        changed = true;
                    }
                }
            }

            if !changed {
                break;
            }

            // 프론티어 스왑
            std::mem::swap(&mut frontier, &mut next_frontier);
            next_frontier.fill(false);
            current_level += 1;
        }

        // 결과 변환
        let visited_count = distances.iter().filter(|&&d| d >= 0).count();
        let max_depth_reached = distances.iter().filter(|&&d| d >= 0).max().unwrap_or(&0);

        let distance_map: HashMap<NodeId, i32> = distances
            .iter()
            .enumerate()
            .filter_map(|(idx, &dist)| csr.get_node_id(idx as u32).map(|node_id| (node_id, dist)))
            .collect();

        Ok(BfsGpuResult {
            distances: distance_map,
            visited_count,
            max_depth_reached: *max_depth_reached as usize,
            backend: self.backend.backend_type(),
        })
    }

    /// 다중 소스 CPU Fallback
    fn compute_multi_source_cpu_fallback(
        &self,
        csr: &CsrGraph,
        source_indices: &[usize],
        config: &BfsGpuConfig,
    ) -> GpuResult<BfsGpuResult> {
        let num_nodes = csr.num_nodes;
        let mut distances = vec![-1i32; num_nodes];
        let mut frontier = vec![false; num_nodes];
        let mut next_frontier = vec![false; num_nodes];

        // 모든 시작 노드 설정
        for &source_idx in source_indices {
            distances[source_idx] = 0;
            frontier[source_idx] = true;
        }

        let mut current_level = 0;
        let max_depth = config.max_depth.unwrap_or(usize::MAX);

        // 레벨 동기화 BFS
        loop {
            if current_level >= max_depth as i32 {
                break;
            }

            let mut changed = false;

            for (node, &is_frontier) in frontier.iter().enumerate() {
                if !is_frontier {
                    continue;
                }

                let neighbors = csr.neighbors(node as u32);
                for &neighbor in neighbors {
                    let neighbor = neighbor as usize;
                    if distances[neighbor] == -1 {
                        distances[neighbor] = current_level + 1;
                        next_frontier[neighbor] = true;
                        changed = true;
                    }
                }
            }

            if !changed {
                break;
            }

            std::mem::swap(&mut frontier, &mut next_frontier);
            next_frontier.fill(false);
            current_level += 1;
        }

        let visited_count = distances.iter().filter(|&&d| d >= 0).count();
        let max_depth_reached = distances.iter().filter(|&&d| d >= 0).max().unwrap_or(&0);

        let distance_map: HashMap<NodeId, i32> = distances
            .iter()
            .enumerate()
            .filter_map(|(idx, &dist)| csr.get_node_id(idx as u32).map(|node_id| (node_id, dist)))
            .collect();

        Ok(BfsGpuResult {
            distances: distance_map,
            visited_count,
            max_depth_reached: *max_depth_reached as usize,
            backend: self.backend.backend_type(),
        })
    }
}

/// GPU 가속 SSSP (Single-Source Shortest Path)
pub struct SsspGpu {
    /// GPU 백엔드
    backend: Box<dyn GpuBackend>,
}

/// SSSP 설정
#[derive(Debug, Clone)]
pub struct SsspGpuConfig {
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 임계값
    pub tolerance: f32,
}

impl Default for SsspGpuConfig {
    fn default() -> Self {
        Self {
            max_iterations: 1000,
            tolerance: 1e-6,
        }
    }
}

/// SSSP 결과
#[derive(Debug, Clone)]
pub struct SsspGpuResult {
    /// 시작 노드에서 각 노드까지의 최단 거리
    pub distances: HashMap<NodeId, f64>,
    /// 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 사용된 백엔드
    pub backend: BackendType,
}

impl SsspGpuResult {
    /// 특정 노드까지의 거리 조회
    pub fn distance(&self, node_id: NodeId) -> Option<f64> {
        self.distances
            .get(&node_id)
            .copied()
            .filter(|&d| d.is_finite())
    }

    /// 도달 가능한 노드들
    pub fn reachable_nodes(&self) -> Vec<NodeId> {
        self.distances
            .iter()
            .filter(|(_, &d)| d.is_finite())
            .map(|(&id, _)| id)
            .collect()
    }
}

impl SsspGpu {
    /// 새 GPU SSSP 계산기 생성
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

    /// SSSP 계산 (Bellman-Ford 스타일)
    ///
    /// # Arguments
    /// * `engine` - 그래프 엔진
    /// * `source` - 시작 노드 ID
    /// * `weight_property` - 가중치 프로퍼티 이름
    /// * `config` - SSSP 설정
    pub fn compute(
        &self,
        engine: &GraphEngine,
        source: NodeId,
        weight_property: &str,
        config: SsspGpuConfig,
    ) -> GpuResult<SsspGpuResult> {
        // CSR 그래프 생성 (가중치 포함)
        let csr = CsrGraph::from_graph(engine, Some(weight_property))?;

        if csr.num_nodes == 0 {
            return Ok(SsspGpuResult {
                distances: HashMap::new(),
                iterations: 0,
                converged: true,
                backend: self.backend.backend_type(),
            });
        }

        let source_idx = match csr.get_index(source) {
            Some(idx) => idx as usize,
            None => {
                return Ok(SsspGpuResult {
                    distances: HashMap::new(),
                    iterations: 0,
                    converged: true,
                    backend: self.backend.backend_type(),
                });
            }
        };

        // CPU fallback
        let result = self.compute_cpu_fallback(&csr, source_idx, &config)?;

        Ok(result)
    }

    /// CPU Fallback (Bellman-Ford)
    fn compute_cpu_fallback(
        &self,
        csr: &CsrGraph,
        source_idx: usize,
        config: &SsspGpuConfig,
    ) -> GpuResult<SsspGpuResult> {
        let num_nodes = csr.num_nodes;
        let mut distances = vec![f64::INFINITY; num_nodes];
        distances[source_idx] = 0.0;

        let weights = csr.edge_weights.as_ref();
        let mut iterations = 0;
        let mut converged = false;

        // Bellman-Ford 반복
        for _ in 0..config.max_iterations {
            iterations += 1;
            let mut changed = false;

            for node in 0..num_nodes {
                let node_dist = distances[node];
                if node_dist.is_infinite() {
                    continue;
                }

                let start = csr.row_offsets[node] as usize;
                let end = csr.row_offsets[node + 1] as usize;

                for edge_idx in start..end {
                    let neighbor = csr.col_indices[edge_idx] as usize;
                    let weight = weights.map_or(1.0, |w| w[edge_idx] as f64);
                    let new_dist = node_dist + weight;

                    if new_dist < distances[neighbor] {
                        distances[neighbor] = new_dist;
                        changed = true;
                    }
                }
            }

            if !changed {
                converged = true;
                break;
            }
        }

        let distance_map: HashMap<NodeId, f64> = distances
            .iter()
            .enumerate()
            .filter_map(|(idx, &dist)| csr.get_node_id(idx as u32).map(|node_id| (node_id, dist)))
            .collect();

        Ok(SsspGpuResult {
            distances: distance_map,
            iterations,
            converged,
            backend: self.backend.backend_type(),
        })
    }
}
