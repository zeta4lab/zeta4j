// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! DiskEdgeStorage 타입 정의

use crate::core::{EdgeId, NodeId};
use std::hash::{Hash, Hasher};

/// 노드-엣지 복합 키 (outgoing/incoming 인덱스용)
///
/// B+Tree는 u64 키만 지원하므로, node_id와 edge_id를 하나의 u64로 인코딩
/// - 상위 32비트: node_id (최대 4B 노드)
/// - 하위 32비트: edge_id (최대 4B 엣지)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeEdgeKey {
    pub node_id: NodeId,
    pub edge_id: EdgeId,
}

impl NodeEdgeKey {
    pub fn new(node_id: NodeId, edge_id: EdgeId) -> Self {
        Self { node_id, edge_id }
    }

    /// u64로 인코딩 (B+Tree 키로 사용)
    pub fn encode(&self) -> u64 {
        // 상위 32비트: node_id, 하위 32비트: edge_id
        (self.node_id << 32) | (self.edge_id & 0xFFFFFFFF)
    }

    /// u64에서 디코딩
    pub fn decode(encoded: u64) -> Self {
        let node_id = (encoded >> 32) as NodeId;
        let edge_id = (encoded & 0xFFFFFFFF) as EdgeId;
        Self { node_id, edge_id }
    }

    /// 특정 node_id의 범위 시작 키
    pub fn range_start(node_id: NodeId) -> u64 {
        node_id << 32
    }

    /// 특정 node_id의 범위 끝 키 (exclusive)
    pub fn range_end(node_id: NodeId) -> u64 {
        (node_id << 32) | 0xFFFFFFFF
    }
}

/// 타입-엣지 복합 키 (type 인덱스용)
///
/// relation_type 문자열을 해시하여 u64로 변환
/// - 상위 32비트: type_hash
/// - 하위 32비트: edge_id
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeEdgeKey {
    pub type_hash: u32,
    pub edge_id: EdgeId,
}

impl TypeEdgeKey {
    pub fn new(relation_type: &str, edge_id: EdgeId) -> Self {
        Self {
            type_hash: Self::hash_type(relation_type),
            edge_id,
        }
    }

    /// relation_type을 u32 해시로 변환
    pub fn hash_type(relation_type: &str) -> u32 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        relation_type.hash(&mut hasher);
        hasher.finish() as u32
    }

    /// u64로 인코딩 (B+Tree 키로 사용)
    pub fn encode(&self) -> u64 {
        ((self.type_hash as u64) << 32) | (self.edge_id & 0xFFFFFFFF)
    }

    /// u64에서 디코딩
    pub fn decode(encoded: u64) -> Self {
        let type_hash = (encoded >> 32) as u32;
        let edge_id = (encoded & 0xFFFFFFFF) as EdgeId;
        Self { type_hash, edge_id }
    }

    /// 특정 type_hash의 범위 시작 키
    pub fn range_start(type_hash: u32) -> u64 {
        (type_hash as u64) << 32
    }

    /// 특정 type_hash의 범위 끝 키 (exclusive)
    pub fn range_end(type_hash: u32) -> u64 {
        ((type_hash as u64) << 32) | 0xFFFFFFFF
    }
}
