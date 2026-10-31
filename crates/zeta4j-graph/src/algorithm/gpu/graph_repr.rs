// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GPU용 그래프 표현
//!
//! GPU에서 효율적으로 처리할 수 있는 Compressed Sparse Row (CSR) 포맷을 제공합니다.

use std::collections::HashMap;

use crate::core::NodeId;
use crate::engine::GraphEngine;

use super::backend::{GpuBackend, GpuBuffer};
use super::error::GpuResult;

/// 슬라이스를 바이트 슬라이스로 변환 (GPU 업로드용)
#[inline]
fn slice_to_bytes<T: bytemuck::Pod>(slice: &[T]) -> &[u8] {
    bytemuck::cast_slice(slice)
}

/// Compressed Sparse Row (CSR) 그래프 표현
///
/// GPU에서 효율적으로 처리할 수 있는 그래프 포맷입니다.
///
/// # 구조
///
/// - `row_offsets[i]`: 노드 i의 이웃 목록 시작 인덱스
/// - `row_offsets[i+1] - row_offsets[i]`: 노드 i의 차수 (이웃 수)
/// - `col_indices[row_offsets[i]..row_offsets[i+1]]`: 노드 i의 이웃 목록
///
/// # 예시
///
/// ```text
/// 그래프: 0 -> 1, 0 -> 2, 1 -> 2
///
/// row_offsets = [0, 2, 3, 3]
/// col_indices = [1, 2, 2]
///
/// 노드 0의 이웃: col_indices[0..2] = [1, 2]
/// 노드 1의 이웃: col_indices[2..3] = [2]
/// 노드 2의 이웃: col_indices[3..3] = []
/// ```
#[derive(Debug, Clone)]
pub struct CsrGraph {
    /// 노드 수
    pub num_nodes: usize,
    /// 엣지 수
    pub num_edges: usize,
    /// 각 노드의 이웃 목록 시작 오프셋 (길이: num_nodes + 1)
    pub row_offsets: Vec<u32>,
    /// 엣지의 목적지 노드 인덱스 (길이: num_edges)
    pub col_indices: Vec<u32>,
    /// 엣지 가중치 (Optional, 길이: num_edges)
    pub edge_weights: Option<Vec<f32>>,
    /// NodeId → 내부 인덱스 매핑
    pub node_to_index: HashMap<NodeId, u32>,
    /// 내부 인덱스 → NodeId 매핑
    pub index_to_node: Vec<NodeId>,
}

impl CsrGraph {
    /// GraphEngine에서 CSR 그래프 생성
    ///
    /// # Arguments
    /// * `graph` - 소스 그래프
    /// * `weight_property` - 가중치로 사용할 프로퍼티 이름 (None이면 가중치 없음)
    pub fn from_graph(graph: &GraphEngine, weight_property: Option<&str>) -> GpuResult<Self> {
        // 노드 수집
        let node_count = graph.node_count()?;
        let mut nodes: Vec<NodeId> = Vec::with_capacity(node_count);

        for node_id in 1..=node_count as u64 {
            if graph.get_node(node_id).ok().flatten().is_some() {
                nodes.push(node_id);
            }
        }

        let num_nodes = nodes.len();

        if num_nodes == 0 {
            return Ok(Self {
                num_nodes: 0,
                num_edges: 0,
                row_offsets: vec![0],
                col_indices: Vec::new(),
                edge_weights: None,
                node_to_index: HashMap::new(),
                index_to_node: Vec::new(),
            });
        }

        // 노드 ID → 인덱스 매핑 생성
        let mut node_to_index = HashMap::with_capacity(num_nodes);
        let mut index_to_node = Vec::with_capacity(num_nodes);

        for (idx, &node_id) in nodes.iter().enumerate() {
            node_to_index.insert(node_id, idx as u32);
            index_to_node.push(node_id);
        }

        // 엣지 수집 (노드별 나가는 엣지)
        let edge_count = graph.edge_count();
        let mut outgoing_edges: HashMap<NodeId, Vec<(NodeId, Option<f32>)>> = HashMap::new();

        for node_id in &nodes {
            outgoing_edges.insert(*node_id, Vec::new());
        }

        let has_weights = weight_property.is_some();

        for edge_id in 1..=edge_count as u64 {
            if let Ok(Some(edge)) = graph.get_edge(edge_id) {
                let from = edge.from_node();
                let to = edge.to_node();

                if node_to_index.contains_key(&from) && node_to_index.contains_key(&to) {
                    let weight = if has_weights {
                        weight_property
                            .and_then(|prop| edge.properties().get(prop))
                            .and_then(|v| match v {
                                crate::core::Value::Float(f) => Some(*f as f32),
                                crate::core::Value::Integer(i) => Some(*i as f32),
                                _ => None,
                            })
                    } else {
                        None
                    };
                    outgoing_edges.entry(from).or_default().push((to, weight));
                }
            }
        }

        // CSR 구조 생성
        let mut row_offsets = Vec::with_capacity(num_nodes + 1);
        let mut col_indices = Vec::new();
        let mut edge_weights_vec: Vec<f32> = Vec::new();

        let mut current_offset: u32 = 0;

        for node_id in &nodes {
            row_offsets.push(current_offset);

            if let Some(edges) = outgoing_edges.get(node_id) {
                for (target_id, weight) in edges {
                    if let Some(&target_idx) = node_to_index.get(target_id) {
                        col_indices.push(target_idx);

                        if has_weights {
                            edge_weights_vec.push(weight.unwrap_or(1.0));
                        }

                        current_offset += 1;
                    }
                }
            }
        }

        row_offsets.push(current_offset);

        let num_edges = col_indices.len();

        Ok(Self {
            num_nodes,
            num_edges,
            row_offsets,
            col_indices,
            edge_weights: if has_weights {
                Some(edge_weights_vec)
            } else {
                None
            },
            node_to_index,
            index_to_node,
        })
    }

    /// 빈 CSR 그래프 생성
    pub fn empty() -> Self {
        Self {
            num_nodes: 0,
            num_edges: 0,
            row_offsets: vec![0],
            col_indices: Vec::new(),
            edge_weights: None,
            node_to_index: HashMap::new(),
            index_to_node: Vec::new(),
        }
    }

    /// 노드 i의 차수 (이웃 수)
    pub fn degree(&self, node_idx: u32) -> u32 {
        if node_idx as usize >= self.num_nodes {
            return 0;
        }
        self.row_offsets[node_idx as usize + 1] - self.row_offsets[node_idx as usize]
    }

    /// 노드 i의 이웃 목록
    pub fn neighbors(&self, node_idx: u32) -> &[u32] {
        if node_idx as usize >= self.num_nodes {
            return &[];
        }
        let start = self.row_offsets[node_idx as usize] as usize;
        let end = self.row_offsets[node_idx as usize + 1] as usize;
        &self.col_indices[start..end]
    }

    /// GPU에 업로드
    pub fn upload_to_gpu(&self, backend: &dyn GpuBackend) -> GpuResult<GpuCsrGraph> {
        // dyn GpuBackend에서 제네릭 upload를 호출할 수 없으므로 bytes 버전 사용
        let row_offsets = backend.upload_bytes(slice_to_bytes(&self.row_offsets))?;
        let col_indices = backend.upload_bytes(slice_to_bytes(&self.col_indices))?;
        let edge_weights = if let Some(ref weights) = self.edge_weights {
            Some(backend.upload_bytes(slice_to_bytes(weights))?)
        } else {
            None
        };

        Ok(GpuCsrGraph {
            row_offsets,
            col_indices,
            edge_weights,
            num_nodes: self.num_nodes,
            num_edges: self.num_edges,
        })
    }

    /// NodeId를 내부 인덱스로 변환
    pub fn get_index(&self, node_id: NodeId) -> Option<u32> {
        self.node_to_index.get(&node_id).copied()
    }

    /// 내부 인덱스를 NodeId로 변환
    pub fn get_node_id(&self, index: u32) -> Option<NodeId> {
        self.index_to_node.get(index as usize).copied()
    }

    /// 메모리 사용량 (bytes)
    pub fn memory_usage(&self) -> usize {
        let row_offsets_size = self.row_offsets.len() * std::mem::size_of::<u32>();
        let col_indices_size = self.col_indices.len() * std::mem::size_of::<u32>();
        let weights_size = self
            .edge_weights
            .as_ref()
            .map(|w| w.len() * std::mem::size_of::<f32>())
            .unwrap_or(0);

        row_offsets_size + col_indices_size + weights_size
    }
}

/// GPU에 업로드된 CSR 그래프
#[derive(Debug)]
pub struct GpuCsrGraph {
    /// 노드별 오프셋 버퍼
    pub row_offsets: GpuBuffer,
    /// 이웃 인덱스 버퍼
    pub col_indices: GpuBuffer,
    /// 엣지 가중치 버퍼 (Optional)
    pub edge_weights: Option<GpuBuffer>,
    /// 노드 수
    pub num_nodes: usize,
    /// 엣지 수
    pub num_edges: usize,
}

impl GpuCsrGraph {
    /// GPU 메모리 사용량 (bytes)
    pub fn gpu_memory_usage(&self) -> usize {
        let mut total = self.row_offsets.size + self.col_indices.size;
        if let Some(ref weights) = self.edge_weights {
            total += weights.size;
        }
        total
    }
}

/// 양방향 CSR 그래프 (in-edges와 out-edges 모두 포함)
#[derive(Debug, Clone)]
pub struct BidirectionalCsrGraph {
    /// 나가는 엣지 (정방향)
    pub outgoing: CsrGraph,
    /// 들어오는 엣지 (역방향)
    pub incoming: CsrGraph,
}

impl BidirectionalCsrGraph {
    /// GraphEngine에서 양방향 CSR 그래프 생성
    pub fn from_graph(graph: &GraphEngine, weight_property: Option<&str>) -> GpuResult<Self> {
        let outgoing = CsrGraph::from_graph(graph, weight_property)?;

        // 역방향 그래프 생성
        let mut incoming_edges: Vec<Vec<(u32, Option<f32>)>> = vec![Vec::new(); outgoing.num_nodes];

        for src_idx in 0..outgoing.num_nodes {
            let start = outgoing.row_offsets[src_idx] as usize;
            let end = outgoing.row_offsets[src_idx + 1] as usize;

            for edge_idx in start..end {
                let dst_idx = outgoing.col_indices[edge_idx];
                let weight = outgoing.edge_weights.as_ref().map(|w| w[edge_idx]);
                incoming_edges[dst_idx as usize].push((src_idx as u32, weight));
            }
        }

        // incoming CSR 구성
        let mut row_offsets = Vec::with_capacity(outgoing.num_nodes + 1);
        let mut col_indices = Vec::new();
        let mut edge_weights_vec: Vec<f32> = Vec::new();
        let has_weights = outgoing.edge_weights.is_some();

        let mut current_offset: u32 = 0;

        for edges in &incoming_edges {
            row_offsets.push(current_offset);
            for (src_idx, weight) in edges {
                col_indices.push(*src_idx);
                if has_weights {
                    edge_weights_vec.push(weight.unwrap_or(1.0));
                }
                current_offset += 1;
            }
        }
        row_offsets.push(current_offset);

        let incoming = CsrGraph {
            num_nodes: outgoing.num_nodes,
            num_edges: outgoing.num_edges,
            row_offsets,
            col_indices,
            edge_weights: if has_weights {
                Some(edge_weights_vec)
            } else {
                None
            },
            node_to_index: outgoing.node_to_index.clone(),
            index_to_node: outgoing.index_to_node.clone(),
        };

        Ok(Self { outgoing, incoming })
    }

    /// 메모리 사용량 (bytes)
    pub fn memory_usage(&self) -> usize {
        self.outgoing.memory_usage() + self.incoming.memory_usage()
    }
}
