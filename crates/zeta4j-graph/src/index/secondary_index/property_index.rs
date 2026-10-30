//! Property Index
//!
//! 단일 프로퍼티 인덱스 구현

use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};

use crate::core::{NodeId, Value};

use super::value::IndexValue;

/// 단일 프로퍼티 인덱스
/// 하나의 (Label, Property) 조합에 대한 인덱스
#[derive(Debug)]
pub struct PropertyIndex {
    /// 값 -> 노드 ID 집합 매핑
    value_to_nodes: RwLock<HashMap<IndexValue, HashSet<NodeId>>>,
    /// 노드 ID -> 현재 인덱싱된 값 (업데이트 시 이전 값 제거용)
    pub(crate) node_to_value: RwLock<HashMap<NodeId, IndexValue>>,
}

impl PropertyIndex {
    /// 새 프로퍼티 인덱스 생성
    pub fn new() -> Self {
        Self {
            value_to_nodes: RwLock::new(HashMap::new()),
            node_to_value: RwLock::new(HashMap::new()),
        }
    }

    /// 노드를 인덱스에 추가
    pub fn insert(&self, node_id: NodeId, value: &Value) {
        let index_value = IndexValue::from(value);

        // 기존 값이 있으면 제거
        self.remove(node_id);

        // 새 값 추가
        {
            let mut value_to_nodes = self.value_to_nodes.write();
            value_to_nodes
                .entry(index_value.clone())
                .or_default()
                .insert(node_id);
        }

        {
            let mut node_to_value = self.node_to_value.write();
            node_to_value.insert(node_id, index_value);
        }
    }

    /// 노드를 인덱스에서 제거
    pub fn remove(&self, node_id: NodeId) {
        let mut node_to_value = self.node_to_value.write();

        if let Some(old_value) = node_to_value.remove(&node_id) {
            let mut value_to_nodes = self.value_to_nodes.write();
            if let Some(nodes) = value_to_nodes.get_mut(&old_value) {
                nodes.remove(&node_id);
                if nodes.is_empty() {
                    value_to_nodes.remove(&old_value);
                }
            }
        }
    }

    /// 값으로 노드 검색
    pub fn find(&self, value: &Value) -> Vec<NodeId> {
        let index_value = IndexValue::from(value);
        let value_to_nodes = self.value_to_nodes.read();

        value_to_nodes
            .get(&index_value)
            .map(|nodes| nodes.iter().copied().collect())
            .unwrap_or_default()
    }

    /// 인덱스에 저장된 노드 수
    pub fn len(&self) -> usize {
        self.node_to_value.read().len()
    }

    /// 인덱스가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.node_to_value.read().is_empty()
    }

    /// 인덱스 초기화
    pub fn clear(&self) {
        self.value_to_nodes.write().clear();
        self.node_to_value.write().clear();
    }
}

impl Default for PropertyIndex {
    fn default() -> Self {
        Self::new()
    }
}
