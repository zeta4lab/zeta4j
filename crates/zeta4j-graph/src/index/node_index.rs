// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 노드 전용 인덱싱 시스템

use crate::core::{MemoryReporter, NodeId, Value};
use crate::index::HashIndex;
use crate::index::range_index::{
    BTreeRangeIndex, fallback_float_range_search, fallback_integer_range_search,
};
use parking_lot::RwLock;
use std::collections::HashMap;

/// 노드 인덱싱 시스템
#[derive(Debug)]
pub struct NodeIndex {
    /// 레이블별 노드 인덱스 (label -> NodeId set)
    label_index: HashIndex<String, NodeId>,

    /// 속성 키별 노드 인덱스 (property_key -> NodeId set)
    property_key_index: HashIndex<String, NodeId>,

    /// 속성 값별 노드 인덱스 (property_key -> (value -> NodeId set))
    property_value_index: RwLock<HashMap<String, HashIndex<Value, NodeId>>>,

    /// 복합 인덱스 (label + property_key -> NodeId set)
    composite_index: RwLock<HashMap<String, HashIndex<Value, NodeId>>>,

    /// 범위 검색용 B+Tree 인덱스 (property_key -> BTreeRangeIndex)
    range_index: RwLock<HashMap<String, BTreeRangeIndex<NodeId>>>,
}

impl NodeIndex {
    /// 새로운 노드 인덱스 생성
    pub fn new() -> Self {
        Self {
            label_index: HashIndex::new(),
            property_key_index: HashIndex::new(),
            property_value_index: RwLock::new(HashMap::new()),
            composite_index: RwLock::new(HashMap::new()),
            range_index: RwLock::new(HashMap::new()),
        }
    }

    /// 초기 용량을 지정한 노드 인덱스 생성
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            label_index: HashIndex::with_capacity(capacity),
            property_key_index: HashIndex::with_capacity(capacity),
            property_value_index: RwLock::new(HashMap::with_capacity(capacity)),
            composite_index: RwLock::new(HashMap::with_capacity(capacity)),
            range_index: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }

    /// 노드의 레이블 인덱싱
    pub fn add_node_label(&self, node_id: NodeId, label: &str) {
        self.label_index.insert(label.to_string(), node_id);
    }

    /// 노드의 레이블 인덱스에서 제거
    pub fn remove_node_label(&self, node_id: NodeId, label: &str) {
        self.label_index.remove(&label.to_string(), &node_id);
    }

    /// 노드의 속성 인덱싱
    pub fn add_node_property(&self, node_id: NodeId, key: &str, value: &Value) {
        // 속성 키 인덱스에 추가
        self.property_key_index.insert(key.to_string(), node_id);

        // 속성 값 인덱스에 추가
        let mut property_value_index = self.property_value_index.write();
        property_value_index
            .entry(key.to_string())
            .or_default()
            .insert(value.clone(), node_id);

        // 숫자 타입은 범위 인덱스에도 추가 (O(log n) 범위 검색 지원)
        match value {
            Value::Integer(v) => {
                let mut range_indices = self.range_index.write();
                range_indices
                    .entry(key.to_string())
                    .or_default()
                    .insert_integer(*v, node_id);
            }
            Value::Float(v) => {
                let mut range_indices = self.range_index.write();
                range_indices
                    .entry(key.to_string())
                    .or_default()
                    .insert_float(*v, node_id);
            }
            _ => {} // 숫자가 아닌 값은 범위 인덱스에 추가하지 않음
        }
    }

    /// 노드의 속성 인덱스에서 제거
    pub fn remove_node_property(&self, node_id: NodeId, key: &str, value: &Value) {
        // 속성 키 인덱스에서 제거
        self.property_key_index.remove(&key.to_string(), &node_id);

        // 속성 값 인덱스에서 제거
        let mut property_value_index = self.property_value_index.write();
        if let Some(value_index) = property_value_index.get_mut(key) {
            value_index.remove(value, &node_id);
            // 빈 인덱스는 제거
            if value_index.is_empty() {
                property_value_index.remove(key);
            }
        }

        // 범위 인덱스에서도 제거
        match value {
            Value::Integer(v) => {
                let mut range_indices = self.range_index.write();
                if let Some(range_idx) = range_indices.get_mut(key) {
                    range_idx.remove_integer(*v, &node_id);
                    if range_idx.is_empty() {
                        range_indices.remove(key);
                    }
                }
            }
            Value::Float(v) => {
                let mut range_indices = self.range_index.write();
                if let Some(range_idx) = range_indices.get_mut(key) {
                    range_idx.remove_float(*v, &node_id);
                    if range_idx.is_empty() {
                        range_indices.remove(key);
                    }
                }
            }
            _ => {}
        }
    }

    /// 복합 인덱스 생성 (레이블 + 속성)
    pub fn create_composite_index(&self, label: &str, property_key: &str) {
        let composite_key = format!("{}:{}", label, property_key);
        let mut composite_index = self.composite_index.write();
        composite_index.insert(composite_key, HashIndex::new());
    }

    /// 복합 인덱스에 노드 추가
    pub fn add_to_composite_index(
        &self,
        node_id: NodeId,
        label: &str,
        property_key: &str,
        property_value: &Value,
    ) {
        let composite_key = format!("{}:{}", label, property_key);
        let mut composite_index = self.composite_index.write();

        if let Some(index) = composite_index.get_mut(&composite_key) {
            index.insert(property_value.clone(), node_id);
        }
    }

    /// 복합 인덱스에서 노드 제거
    pub fn remove_from_composite_index(
        &self,
        node_id: NodeId,
        label: &str,
        property_key: &str,
        property_value: &Value,
    ) {
        let composite_key = format!("{}:{}", label, property_key);
        let mut composite_index = self.composite_index.write();

        if let Some(index) = composite_index.get_mut(&composite_key) {
            index.remove(property_value, &node_id);
        }
    }

    /// 레이블로 노드들 검색
    pub fn find_nodes_by_label(&self, label: &str) -> Vec<NodeId> {
        self.label_index.get(&label.to_string())
    }

    /// 속성 키로 노드들 검색
    pub fn find_nodes_by_property_key(&self, key: &str) -> Vec<NodeId> {
        self.property_key_index.get(&key.to_string())
    }

    /// 속성 키-값으로 노드들 검색
    pub fn find_nodes_by_property(&self, key: &str, value: &Value) -> Vec<NodeId> {
        let property_value_index = self.property_value_index.read();
        property_value_index
            .get(key)
            .map_or(Vec::new(), |index| index.get(value))
    }

    /// 속성 값 범위로 노드들 검색 (정수 값만)
    /// 시간 복잡도: O(log n + k) (k = 결과 수)
    pub fn find_nodes_by_integer_range(&self, key: &str, min: i64, max: i64) -> Vec<NodeId> {
        // BTreeRangeIndex 사용 (O(log n + k))
        let range_indices = self.range_index.read();
        if let Some(range_idx) = range_indices.get(key) {
            return range_idx.range_integer(min, max);
        }
        drop(range_indices);

        // 범위 인덱스가 없으면 기존 방식으로 폴백 (O(n))
        let property_value_index = self.property_value_index.read();
        property_value_index
            .get(key)
            .map(|index| fallback_integer_range_search(index, min, max))
            .unwrap_or_default()
    }

    /// 속성 값 범위로 노드들 검색 (실수 값만)
    /// 시간 복잡도: O(log n + k) (k = 결과 수)
    pub fn find_nodes_by_float_range(&self, key: &str, min: f64, max: f64) -> Vec<NodeId> {
        // BTreeRangeIndex 사용 (O(log n + k))
        let range_indices = self.range_index.read();
        if let Some(range_idx) = range_indices.get(key) {
            return range_idx.range_float(min, max);
        }
        drop(range_indices);

        // 범위 인덱스가 없으면 기존 방식으로 폴백 (O(n))
        let property_value_index = self.property_value_index.read();
        property_value_index
            .get(key)
            .map(|index| fallback_float_range_search(index, min, max))
            .unwrap_or_default()
    }

    /// 문자열 속성 값으로 접두사 검색
    pub fn find_nodes_by_string_prefix(&self, key: &str, prefix: &str) -> Vec<NodeId> {
        let property_value_index = self.property_value_index.read();
        if let Some(index) = property_value_index.get(key) {
            let mut results = Vec::new();
            // keys()를 사용하여 Value 타입의 키들을 순회
            let all_keys = index.keys();
            for value in all_keys {
                if let Some(str_val) = value.as_string()
                    && str_val.starts_with(prefix)
                {
                    results.extend(index.get(&value));
                }
            }
            results
        } else {
            Vec::new()
        }
    }

    /// 복합 인덱스로 노드들 검색
    pub fn find_nodes_by_composite(
        &self,
        label: &str,
        property_key: &str,
        property_value: &Value,
    ) -> Vec<NodeId> {
        let composite_key = format!("{}:{}", label, property_key);
        let composite_index = self.composite_index.read();

        composite_index
            .get(&composite_key)
            .map_or(Vec::new(), |index| index.get(property_value))
    }

    /// 다중 레이블 교집합 검색
    pub fn find_nodes_by_labels_intersection(&self, labels: &[&str]) -> Vec<NodeId> {
        if labels.is_empty() {
            return Vec::new();
        }

        // 첫 번째 레이블의 노드들을 기준으로 시작
        let mut result_nodes: Vec<NodeId> = self.find_nodes_by_label(labels[0]);

        // 나머지 레이블들과 교집합 계산
        for &label in &labels[1..] {
            let label_nodes = self.find_nodes_by_label(label);
            result_nodes.retain(|node_id| label_nodes.contains(node_id));
        }

        result_nodes
    }

    /// 다중 레이블 합집합 검색
    pub fn find_nodes_by_labels_union(&self, labels: &[&str]) -> Vec<NodeId> {
        let mut result_nodes = Vec::new();
        let mut seen_nodes = std::collections::HashSet::new();

        for &label in labels {
            for node_id in self.find_nodes_by_label(label) {
                if seen_nodes.insert(node_id) {
                    result_nodes.push(node_id);
                }
            }
        }

        result_nodes
    }

    /// 노드의 모든 인덱스 항목 제거
    pub fn remove_node(&self, node_id: NodeId) {
        // 레이블 인덱스에서 제거
        let labels_to_remove: Vec<String> = self.label_index.get_reverse(&node_id);
        for label in labels_to_remove {
            self.label_index.remove(&label, &node_id);
        }

        // 속성 키 인덱스에서 제거
        let keys_to_remove: Vec<String> = self.property_key_index.get_reverse(&node_id);
        for key in keys_to_remove {
            self.property_key_index.remove(&key, &node_id);

            // 속성 값 인덱스에서도 제거
            let mut property_value_index = self.property_value_index.write();
            if let Some(value_index) = property_value_index.get_mut(&key) {
                let values_to_remove: Vec<Value> = value_index.get_reverse(&node_id);
                for value in values_to_remove {
                    value_index.remove(&value, &node_id);
                }
                if value_index.is_empty() {
                    property_value_index.remove(&key);
                }
            }
        }

        // 복합 인덱스에서 제거
        let mut composite_index = self.composite_index.write();
        for index in composite_index.values_mut() {
            let values_to_remove: Vec<Value> = index.get_reverse(&node_id);
            for value in values_to_remove {
                index.remove(&value, &node_id);
            }
        }
    }

    /// 인덱스 통계
    pub fn stats(&self) -> NodeIndexStats {
        let property_value_index = self.property_value_index.read();
        let composite_index = self.composite_index.read();

        NodeIndexStats {
            label_count: self.label_index.key_count(),
            property_key_count: self.property_key_index.key_count(),
            property_value_indexes: property_value_index.len(),
            composite_indexes: composite_index.len(),
            total_label_mappings: self.label_index.total_mappings(),
            total_property_mappings: self.property_key_index.total_mappings(),
            memory_usage: self.memory_usage(),
        }
    }

    /// 메모리 사용량 추정
    pub fn memory_usage(&self) -> usize {
        let mut size = std::mem::size_of::<Self>();
        size += self.label_index.memory_usage();
        size += self.property_key_index.memory_usage();

        let property_value_index = self.property_value_index.read();
        for index in property_value_index.values() {
            size += index.memory_usage();
        }

        let composite_index = self.composite_index.read();
        for index in composite_index.values() {
            size += index.memory_usage();
        }

        size
    }

    /// 모든 인덱스 초기화
    pub fn clear(&self) {
        self.label_index.clear();
        self.property_key_index.clear();

        let mut property_value_index = self.property_value_index.write();
        property_value_index.clear();

        let mut composite_index = self.composite_index.write();
        composite_index.clear();
    }

    /// 인덱스 압축
    pub fn compact(&self) {
        self.label_index.compact();
        self.property_key_index.compact();

        let mut property_value_index = self.property_value_index.write();
        for index in property_value_index.values_mut() {
            index.compact();
        }

        let mut composite_index = self.composite_index.write();
        for index in composite_index.values_mut() {
            index.compact();
        }
    }
}

impl Default for NodeIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// 노드 인덱스 통계
#[derive(Debug, Clone)]
pub struct NodeIndexStats {
    pub label_count: usize,
    pub property_key_count: usize,
    pub property_value_indexes: usize,
    pub composite_indexes: usize,
    pub total_label_mappings: usize,
    pub total_property_mappings: usize,
    pub memory_usage: usize,
}

impl MemoryReporter for NodeIndexStats {
    fn memory_usage(&self) -> usize {
        self.memory_usage
    }
}
