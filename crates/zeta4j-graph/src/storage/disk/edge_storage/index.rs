//! 보조 인덱스 관리
//!
//! B+Tree는 u64 키만 지원하므로, 보조 인덱스는 메모리에서 관리하고
//! 시작 시 전체 Edge를 스캔하여 rebuild 합니다.

use crate::core::{EdgeId, NodeId};
use indexmap::IndexSet;
use parking_lot::RwLock;
use std::collections::HashMap;

use super::types::TypeEdgeKey;

/// 메모리 기반 보조 인덱스
#[derive(Debug)]
pub struct SecondaryIndexes {
    /// 노드별 나가는 엣지 인덱스 (from_node -> EdgeId set)
    pub outgoing: RwLock<HashMap<NodeId, IndexSet<EdgeId>>>,

    /// 노드별 들어오는 엣지 인덱스 (to_node -> EdgeId set)
    pub incoming: RwLock<HashMap<NodeId, IndexSet<EdgeId>>>,

    /// 관계 타입별 엣지 인덱스 (type_hash -> EdgeId set)
    /// type_hash는 relation_type의 해시값
    pub type_index: RwLock<HashMap<u32, IndexSet<EdgeId>>>,

    /// type_hash -> relation_type 역매핑 (디버깅/조회용)
    pub type_names: RwLock<HashMap<u32, String>>,
}

impl SecondaryIndexes {
    /// 새 인덱스 생성
    pub fn new() -> Self {
        Self {
            outgoing: RwLock::new(HashMap::new()),
            incoming: RwLock::new(HashMap::new()),
            type_index: RwLock::new(HashMap::new()),
            type_names: RwLock::new(HashMap::new()),
        }
    }

    /// 엣지 추가 시 인덱스 업데이트
    pub fn insert(&self, edge_id: EdgeId, from_node: NodeId, to_node: NodeId, relation_type: &str) {
        let type_hash = TypeEdgeKey::hash_type(relation_type);

        // outgoing index
        self.outgoing
            .write()
            .entry(from_node)
            .or_default()
            .insert(edge_id);

        // incoming index
        self.incoming
            .write()
            .entry(to_node)
            .or_default()
            .insert(edge_id);

        // type index
        self.type_index
            .write()
            .entry(type_hash)
            .or_default()
            .insert(edge_id);

        // type name mapping
        self.type_names
            .write()
            .entry(type_hash)
            .or_insert_with(|| relation_type.to_string());
    }

    /// 엣지 삭제 시 인덱스 업데이트
    pub fn remove(&self, edge_id: EdgeId, from_node: NodeId, to_node: NodeId, relation_type: &str) {
        let type_hash = TypeEdgeKey::hash_type(relation_type);

        // outgoing index
        {
            let mut outgoing = self.outgoing.write();
            if let Some(set) = outgoing.get_mut(&from_node) {
                set.shift_remove(&edge_id);
                if set.is_empty() {
                    outgoing.remove(&from_node);
                }
            }
        }

        // incoming index
        {
            let mut incoming = self.incoming.write();
            if let Some(set) = incoming.get_mut(&to_node) {
                set.shift_remove(&edge_id);
                if set.is_empty() {
                    incoming.remove(&to_node);
                }
            }
        }

        // type index
        {
            let mut type_idx = self.type_index.write();
            if let Some(set) = type_idx.get_mut(&type_hash) {
                set.shift_remove(&edge_id);
                if set.is_empty() {
                    type_idx.remove(&type_hash);
                    // type_names도 정리
                    self.type_names.write().remove(&type_hash);
                }
            }
        }
    }

    /// 특정 노드에서 나가는 엣지들
    pub fn get_outgoing(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.outgoing
            .read()
            .get(&node_id)
            .map(|set| set.iter().rev().copied().collect())
            .unwrap_or_default()
    }

    /// 특정 노드로 들어오는 엣지들
    pub fn get_incoming(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.incoming
            .read()
            .get(&node_id)
            .map(|set| set.iter().rev().copied().collect())
            .unwrap_or_default()
    }

    /// 특정 관계 타입의 엣지들
    pub fn get_by_type(&self, relation_type: &str) -> Vec<EdgeId> {
        let type_hash = TypeEdgeKey::hash_type(relation_type);
        self.type_index
            .read()
            .get(&type_hash)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// 특정 노드와 연결된 모든 엣지들 (양방향)
    pub fn get_connected(&self, node_id: NodeId) -> Vec<EdgeId> {
        let mut result = std::collections::HashSet::new();

        if let Some(set) = self.outgoing.read().get(&node_id) {
            result.extend(set.iter().copied());
        }

        if let Some(set) = self.incoming.read().get(&node_id) {
            result.extend(set.iter().copied());
        }

        result.into_iter().collect()
    }

    /// 노드에 연결된 엣지가 있는지
    pub fn has_connections(&self, node_id: NodeId) -> bool {
        self.outgoing.read().contains_key(&node_id) || self.incoming.read().contains_key(&node_id)
    }

    /// 인덱스 초기화
    pub fn clear(&self) {
        self.outgoing.write().clear();
        self.incoming.write().clear();
        self.type_index.write().clear();
        self.type_names.write().clear();
    }

    /// 통계
    pub fn stats(&self) -> SecondaryIndexStats {
        SecondaryIndexStats {
            outgoing_nodes: self.outgoing.read().len(),
            incoming_nodes: self.incoming.read().len(),
            type_count: self.type_index.read().len(),
        }
    }
}

impl Default for SecondaryIndexes {
    fn default() -> Self {
        Self::new()
    }
}

/// 보조 인덱스 통계
#[derive(Debug, Clone)]
pub struct SecondaryIndexStats {
    pub outgoing_nodes: usize,
    pub incoming_nodes: usize,
    pub type_count: usize,
}
