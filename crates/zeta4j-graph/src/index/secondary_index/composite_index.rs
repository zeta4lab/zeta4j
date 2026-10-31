// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Composite Property Index
//!
//! 복합 프로퍼티 인덱스 구현 (v1.79.0)

use indexmap::IndexMap;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};

use crate::core::{NodeId, Value};

use super::value::IndexValue;

/// 복합 프로퍼티 키 - 여러 프로퍼티 값의 조합
///
/// 프로퍼티 순서가 중요합니다. (name, age)와 (age, name)은 다른 인덱스입니다.
///
/// Note: `index::composite::CompositeKey`와 구분하기 위해 `MultiPropertyKey` 사용
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MultiPropertyKey(Vec<IndexValue>);

impl MultiPropertyKey {
    /// 새 복합 키 생성
    pub fn new(values: Vec<IndexValue>) -> Self {
        Self(values)
    }

    /// Value 슬라이스에서 복합 프로퍼티 키 생성
    pub fn from_values(values: &[&Value]) -> Self {
        Self(values.iter().map(|v| IndexValue::from(*v)).collect())
    }

    /// 키의 길이 (프로퍼티 수)
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 키가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 내부 값 참조
    pub fn values(&self) -> &[IndexValue] {
        &self.0
    }
}

/// 복합 프로퍼티 인덱스
///
/// 여러 프로퍼티를 조합한 인덱스입니다.
/// 예: (firstName, lastName) 조합으로 빠른 검색
///
/// Note: `index::composite::CompositeIndex`와 구분하기 위해 `MultiPropertyIndex` 사용
///
/// # Example
/// ```ignore
/// let index = MultiPropertyIndex::new(vec!["firstName", "lastName"]);
/// index.insert(1, &[&Value::String("John".into()), &Value::String("Doe".into())]);
/// let nodes = index.find(&[&Value::String("John".into()), &Value::String("Doe".into())]);
/// ```
#[derive(Debug)]
pub struct MultiPropertyIndex {
    /// 복합 키 -> 노드 ID 집합 매핑
    value_to_nodes: RwLock<HashMap<MultiPropertyKey, HashSet<NodeId>>>,
    /// 노드 ID -> 현재 인덱싱된 복합 키
    node_to_value: RwLock<HashMap<NodeId, MultiPropertyKey>>,
    /// 프로퍼티 이름 목록 (순서 중요)
    properties: Vec<String>,
}

impl MultiPropertyIndex {
    /// 새 복합 인덱스 생성
    ///
    /// # Arguments
    /// * `properties` - 인덱스할 프로퍼티 이름 목록 (순서 중요)
    pub fn new<S: Into<String>>(properties: Vec<S>) -> Self {
        Self {
            value_to_nodes: RwLock::new(HashMap::new()),
            node_to_value: RwLock::new(HashMap::new()),
            properties: properties.into_iter().map(|s| s.into()).collect(),
        }
    }

    /// 프로퍼티 목록 조회
    pub fn properties(&self) -> &[String] {
        &self.properties
    }

    /// 프로퍼티 수
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }

    /// 노드를 인덱스에 추가
    ///
    /// # Arguments
    /// * `node_id` - 노드 ID
    /// * `values` - 프로퍼티 값 슬라이스 (properties 순서와 동일해야 함)
    pub fn insert(&self, node_id: NodeId, values: &[&Value]) {
        if values.len() != self.properties.len() {
            return; // 프로퍼티 수가 맞지 않으면 무시
        }

        let composite_key = MultiPropertyKey::from_values(values);

        // 기존 값이 있으면 제거
        self.remove(node_id);

        // 새 값 추가
        {
            let mut value_to_nodes = self.value_to_nodes.write();
            value_to_nodes
                .entry(composite_key.clone())
                .or_default()
                .insert(node_id);
        }

        {
            let mut node_to_value = self.node_to_value.write();
            node_to_value.insert(node_id, composite_key);
        }
    }

    /// HashMap에서 프로퍼티 값을 추출하여 인덱스에 추가
    ///
    /// 모든 프로퍼티가 존재해야 인덱싱됩니다.
    pub fn insert_from_properties(&self, node_id: NodeId, properties: &IndexMap<String, Value>) {
        // 모든 프로퍼티가 존재하는지 확인
        let values: Vec<&Value> = self
            .properties
            .iter()
            .filter_map(|prop| properties.get(prop))
            .collect();

        if values.len() == self.properties.len() {
            self.insert(node_id, &values);
        }
    }

    /// 노드를 인덱스에서 제거
    pub fn remove(&self, node_id: NodeId) {
        let mut node_to_value = self.node_to_value.write();

        if let Some(old_key) = node_to_value.remove(&node_id) {
            let mut value_to_nodes = self.value_to_nodes.write();
            if let Some(nodes) = value_to_nodes.get_mut(&old_key) {
                nodes.remove(&node_id);
                if nodes.is_empty() {
                    value_to_nodes.remove(&old_key);
                }
            }
        }
    }

    /// 복합 키로 노드 검색
    pub fn find(&self, values: &[&Value]) -> Vec<NodeId> {
        if values.len() != self.properties.len() {
            return vec![];
        }

        let composite_key = MultiPropertyKey::from_values(values);
        let value_to_nodes = self.value_to_nodes.read();

        value_to_nodes
            .get(&composite_key)
            .map(|nodes| nodes.iter().copied().collect())
            .unwrap_or_default()
    }

    /// 부분 키로 검색 (prefix match)
    ///
    /// 복합 인덱스의 앞부분 프로퍼티만으로 검색합니다.
    /// 예: (firstName, lastName) 인덱스에서 firstName만으로 검색
    pub fn find_prefix(&self, values: &[&Value]) -> Vec<NodeId> {
        if values.is_empty() || values.len() > self.properties.len() {
            return vec![];
        }

        let prefix_values: Vec<IndexValue> = values.iter().map(|v| IndexValue::from(*v)).collect();
        let prefix_len = prefix_values.len();

        let value_to_nodes = self.value_to_nodes.read();
        let mut result = HashSet::new();

        for (key, nodes) in value_to_nodes.iter() {
            // prefix가 일치하는지 확인
            if key.values().len() >= prefix_len && key.values()[..prefix_len] == prefix_values[..] {
                result.extend(nodes.iter().copied());
            }
        }

        result.into_iter().collect()
    }

    /// 인덱스에 저장된 노드 수
    pub fn len(&self) -> usize {
        self.node_to_value.read().len()
    }

    /// 인덱스가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.node_to_value.read().is_empty()
    }

    /// 고유 키 수
    pub fn unique_keys(&self) -> usize {
        self.value_to_nodes.read().len()
    }

    /// 인덱스 초기화
    pub fn clear(&self) {
        self.value_to_nodes.write().clear();
        self.node_to_value.write().clear();
    }
}
