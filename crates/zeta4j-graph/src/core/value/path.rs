//! 경로 타입 정의
//!
//! Cypher의 가변 길이 패턴 결과를 표현하는 경로 타입을 정의합니다.

use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

/// 경로 타입 (노드와 관계의 시퀀스)
/// Cypher의 가변 길이 패턴 결과를 표현
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Path {
    /// 경로상의 노드 ID 목록
    pub nodes: Vec<u64>,
    /// 경로상의 엣지 ID 목록 (nodes.len() - 1 개)
    pub edges: Vec<u64>,
}

impl Path {
    /// 빈 경로 생성 (시작 노드만 포함)
    pub fn new(start_node: u64) -> Self {
        Self {
            nodes: vec![start_node],
            edges: vec![],
        }
    }

    /// 노드와 엣지로 경로 생성
    pub fn from_parts(nodes: Vec<u64>, edges: Vec<u64>) -> Self {
        Self { nodes, edges }
    }

    /// 경로에 엣지와 노드 추가
    pub fn extend(&mut self, edge_id: u64, node_id: u64) {
        self.edges.push(edge_id);
        self.nodes.push(node_id);
    }

    /// 경로 길이 (엣지 수)
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// 경로가 비어있는지 (엣지 없음)
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// 시작 노드
    pub fn start_node(&self) -> Option<u64> {
        self.nodes.first().copied()
    }

    /// 끝 노드
    pub fn end_node(&self) -> Option<u64> {
        self.nodes.last().copied()
    }

    /// 메모리 사용량 추정 (바이트)
    pub fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.nodes.len().saturating_mul(std::mem::size_of::<u64>()))
            .saturating_add(self.edges.len().saturating_mul(std::mem::size_of::<u64>()))
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<")?;
        for (i, node_id) in self.nodes.iter().enumerate() {
            if i > 0
                && let Some(edge_id) = self.edges.get(i - 1)
            {
                write!(f, "-[{}]-", edge_id)?;
            }
            write!(f, "({})", node_id)?;
        }
        write!(f, ">")
    }
}

impl Hash for Path {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.nodes.hash(state);
        self.edges.hash(state);
    }
}

impl PartialOrd for Path {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Path {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.nodes
            .cmp(&other.nodes)
            .then(self.edges.cmp(&other.edges))
    }
}

impl Eq for Path {}
