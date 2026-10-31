// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 유량 네트워크 자료구조
//!
//! Maximum Flow 알고리즘을 위한 유량 네트워크 구현입니다.
//! 잔여 그래프(residual graph)를 효율적으로 관리합니다.

use crate::core::NodeId;
use std::collections::HashMap;

/// 유량 네트워크 간선
#[derive(Debug, Clone)]
pub struct FlowEdge {
    /// 목적지 노드
    pub to: NodeId,
    /// 용량
    pub capacity: f64,
    /// 현재 유량
    pub flow: f64,
    /// 역방향 간선 인덱스
    pub reverse_index: usize,
}

impl FlowEdge {
    /// 잔여 용량
    pub fn residual(&self) -> f64 {
        self.capacity - self.flow
    }

    /// 유량 추가
    pub fn add_flow(&mut self, delta: f64) {
        self.flow += delta;
    }
}

/// 유량 네트워크
#[derive(Debug, Clone)]
pub struct FlowNetwork {
    /// 인접 리스트 (노드 -> 간선 목록)
    adj: HashMap<NodeId, Vec<FlowEdge>>,
    /// 노드 목록
    nodes: Vec<NodeId>,
}

impl FlowNetwork {
    /// 새로운 유량 네트워크 생성
    pub fn new() -> Self {
        Self {
            adj: HashMap::new(),
            nodes: Vec::new(),
        }
    }

    /// 노드 추가
    pub fn add_node(&mut self, node: NodeId) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.adj.entry(node) {
            e.insert(Vec::new());
            self.nodes.push(node);
        }
    }

    /// 간선 추가 (양방향 - 역방향은 용량 0)
    pub fn add_edge(&mut self, from: NodeId, to: NodeId, capacity: f64) {
        self.add_node(from);
        self.add_node(to);

        let from_edges = self.adj.get(&from).map(|e| e.len()).unwrap_or(0);
        let to_edges = self.adj.get(&to).map(|e| e.len()).unwrap_or(0);

        // 정방향 간선
        self.adj.entry(from).or_default().push(FlowEdge {
            to,
            capacity,
            flow: 0.0,
            reverse_index: to_edges,
        });

        // 역방향 간선 (용량 0)
        self.adj.entry(to).or_default().push(FlowEdge {
            to: from,
            capacity: 0.0,
            flow: 0.0,
            reverse_index: from_edges,
        });
    }

    /// 양방향 간선 추가 (둘 다 용량 있음)
    pub fn add_undirected_edge(&mut self, from: NodeId, to: NodeId, capacity: f64) {
        self.add_node(from);
        self.add_node(to);

        let from_edges = self.adj.get(&from).map(|e| e.len()).unwrap_or(0);
        let to_edges = self.adj.get(&to).map(|e| e.len()).unwrap_or(0);

        // 양방향 모두 용량 있음
        self.adj.entry(from).or_default().push(FlowEdge {
            to,
            capacity,
            flow: 0.0,
            reverse_index: to_edges,
        });

        self.adj.entry(to).or_default().push(FlowEdge {
            to: from,
            capacity,
            flow: 0.0,
            reverse_index: from_edges,
        });
    }

    /// 노드의 인접 간선 수
    pub fn degree(&self, node: NodeId) -> usize {
        self.adj.get(&node).map(|e| e.len()).unwrap_or(0)
    }

    /// 노드의 간선들
    pub fn edges(&self, node: NodeId) -> Option<&Vec<FlowEdge>> {
        self.adj.get(&node)
    }

    /// 노드의 간선들 (mutable)
    pub fn edges_mut(&mut self, node: NodeId) -> Option<&mut Vec<FlowEdge>> {
        self.adj.get_mut(&node)
    }

    /// 특정 간선의 잔여 용량
    pub fn residual(&self, from: NodeId, edge_idx: usize) -> f64 {
        self.adj
            .get(&from)
            .and_then(|edges| edges.get(edge_idx))
            .map(|e| e.residual())
            .unwrap_or(0.0)
    }

    /// 유량 추가 (역방향 간선도 갱신)
    pub fn push_flow(&mut self, from: NodeId, edge_idx: usize, delta: f64) {
        if let Some(edges) = self.adj.get_mut(&from)
            && let Some(edge) = edges.get_mut(edge_idx)
        {
            let to = edge.to;
            let rev_idx = edge.reverse_index;
            edge.add_flow(delta);

            // 역방향 간선 갱신
            if let Some(rev_edges) = self.adj.get_mut(&to)
                && let Some(rev_edge) = rev_edges.get_mut(rev_idx)
            {
                rev_edge.add_flow(-delta);
            }
        }
    }

    /// 모든 노드
    pub fn nodes(&self) -> &[NodeId] {
        &self.nodes
    }

    /// 노드 수
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 간선 수 (역방향 포함)
    pub fn edge_count(&self) -> usize {
        self.adj.values().map(|e| e.len()).sum()
    }

    /// 유량 초기화
    pub fn reset_flow(&mut self) {
        for edges in self.adj.values_mut() {
            for edge in edges.iter_mut() {
                edge.flow = 0.0;
            }
        }
    }

    /// 간선별 유량 반환 (용량 > 0인 간선만)
    pub fn get_flow_edges(&self) -> Vec<(NodeId, NodeId, f64, f64)> {
        let mut result = Vec::new();
        for (&from, edges) in &self.adj {
            for edge in edges {
                if edge.capacity > 0.0 && edge.flow > 0.0 {
                    result.push((from, edge.to, edge.flow, edge.capacity));
                }
            }
        }
        result
    }

    /// 노드 존재 여부
    pub fn contains_node(&self, node: NodeId) -> bool {
        self.adj.contains_key(&node)
    }
}

impl Default for FlowNetwork {
    fn default() -> Self {
        Self::new()
    }
}
