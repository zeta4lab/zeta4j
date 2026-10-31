// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Secondary Index Manager
//!
//! 여러 프로퍼티 인덱스를 관리 (단일 및 복합 인덱스 지원)

use indexmap::IndexMap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::Instant;

use crate::core::{NodeId, Value};

use super::composite_index::MultiPropertyIndex;
use super::property_index::PropertyIndex;
use super::value::IndexValue;
use crate::index::index_state::{
    IndexMetadata, IndexState, IndexStatesSummary, IndexVerificationResult,
};

/// Secondary Index Manager
/// 여러 프로퍼티 인덱스를 관리 (단일 및 복합 인덱스 지원)
#[derive(Debug)]
pub struct SecondaryIndexManager {
    /// 단일 프로퍼티 인덱스: 이름 -> 엔트리
    indexes: RwLock<HashMap<String, SecondaryIndexEntry>>,
    /// (라벨, 프로퍼티) -> 인덱스 이름 매핑 (빠른 조회용)
    label_prop_to_name: RwLock<HashMap<(String, String), String>>,
    /// 복합 프로퍼티 인덱스: 이름 -> 엔트리 (v1.79.0)
    composite_indexes: RwLock<HashMap<String, MultiPropertyIndexEntry>>,
    /// (라벨, 프로퍼티 목록) -> 인덱스 이름 매핑 (빠른 조회용)
    label_props_to_name: RwLock<HashMap<(String, Vec<String>), String>>,
}

/// 보조 인덱스 항목 (단일 프로퍼티)
#[derive(Debug)]
pub struct SecondaryIndexEntry {
    /// 인덱스 이름
    pub name: String,
    /// 대상 노드 라벨
    pub label: String,
    /// 대상 프로퍼티
    pub property: String,
    /// 실제 인덱스 데이터
    pub index: PropertyIndex,
    /// 인덱스 메타데이터 (상태 추적)
    pub metadata: RwLock<IndexMetadata>,
}

/// 복합 프로퍼티 인덱스 항목 (v1.79.0)
#[derive(Debug)]
pub struct MultiPropertyIndexEntry {
    /// 인덱스 이름
    pub name: String,
    /// 대상 노드 라벨
    pub label: String,
    /// 대상 프로퍼티 목록 (순서 중요)
    pub properties: Vec<String>,
    /// 실제 인덱스 데이터
    pub index: MultiPropertyIndex,
    /// 인덱스 메타데이터 (상태 추적)
    pub metadata: RwLock<IndexMetadata>,
}

impl SecondaryIndexManager {
    /// 새 인덱스 매니저 생성
    pub fn new() -> Self {
        Self {
            indexes: RwLock::new(HashMap::new()),
            label_prop_to_name: RwLock::new(HashMap::new()),
            composite_indexes: RwLock::new(HashMap::new()),
            label_props_to_name: RwLock::new(HashMap::new()),
        }
    }

    /// 인덱스 생성
    ///
    /// # Arguments
    /// * `name` - 인덱스 이름
    /// * `label` - 대상 노드 라벨
    /// * `property` - 대상 프로퍼티
    ///
    /// # Returns
    /// 성공 시 true, 이미 존재하면 false
    pub fn create_index(&self, name: &str, label: &str, property: &str) -> bool {
        let mut indexes = self.indexes.write();

        if indexes.contains_key(name) {
            return false;
        }

        let entry = SecondaryIndexEntry {
            name: name.to_string(),
            label: label.to_string(),
            property: property.to_string(),
            index: PropertyIndex::new(),
            metadata: RwLock::new(IndexMetadata::new()),
        };

        indexes.insert(name.to_string(), entry);

        let mut label_prop_to_name = self.label_prop_to_name.write();
        label_prop_to_name.insert((label.to_string(), property.to_string()), name.to_string());

        true
    }

    /// 인덱스 삭제
    pub fn drop_index(&self, name: &str) -> bool {
        let mut indexes = self.indexes.write();

        if let Some(entry) = indexes.remove(name) {
            let mut label_prop_to_name = self.label_prop_to_name.write();
            label_prop_to_name.remove(&(entry.label, entry.property));
            true
        } else {
            false
        }
    }

    /// 인덱스 존재 여부 확인 (이름으로)
    pub fn has_index(&self, name: &str) -> bool {
        self.indexes.read().contains_key(name)
    }

    /// 인덱스 존재 여부 확인 (라벨+프로퍼티로)
    pub fn has_index_for(&self, label: &str, property: &str) -> bool {
        self.label_prop_to_name
            .read()
            .contains_key(&(label.to_string(), property.to_string()))
    }

    /// 노드를 관련 인덱스에 추가
    /// 노드의 라벨과 프로퍼티를 확인하여 해당하는 인덱스에 추가
    pub fn index_node(
        &self,
        node_id: NodeId,
        labels: &[String],
        properties: &IndexMap<String, Value>,
    ) {
        let indexes = self.indexes.read();

        for entry in indexes.values() {
            // 라벨이 일치하는지 확인
            if labels.contains(&entry.label) {
                // 프로퍼티가 있는지 확인
                if let Some(value) = properties.get(&entry.property) {
                    entry.index.insert(node_id, value);
                }
            }
        }
    }

    /// 노드를 관련 인덱스에서 제거
    pub fn unindex_node(&self, node_id: NodeId, labels: &[String]) {
        let indexes = self.indexes.read();

        for entry in indexes.values() {
            if labels.contains(&entry.label) {
                entry.index.remove(node_id);
            }
        }
    }

    /// 노드 프로퍼티 업데이트 시 인덱스 갱신
    pub fn update_node_property(
        &self,
        node_id: NodeId,
        labels: &[String],
        property: &str,
        new_value: Option<&Value>,
    ) {
        let indexes = self.indexes.read();

        for entry in indexes.values() {
            if labels.contains(&entry.label) && entry.property == property {
                if let Some(value) = new_value {
                    entry.index.insert(node_id, value);
                } else {
                    entry.index.remove(node_id);
                }
            }
        }
    }

    /// 인덱스를 사용하여 노드 검색
    ///
    /// # Arguments
    /// * `label` - 노드 라벨
    /// * `property` - 프로퍼티 이름
    /// * `value` - 검색할 값
    ///
    /// # Returns
    /// Some(노드 ID 목록) - 인덱스가 있고 검색 성공
    /// None - 해당 라벨+프로퍼티에 대한 인덱스가 없음
    pub fn find_nodes(&self, label: &str, property: &str, value: &Value) -> Option<Vec<NodeId>> {
        let label_prop_to_name = self.label_prop_to_name.read();

        if let Some(index_name) = label_prop_to_name.get(&(label.to_string(), property.to_string()))
        {
            let indexes = self.indexes.read();
            if let Some(entry) = indexes.get(index_name) {
                return Some(entry.index.find(value));
            }
        }

        None
    }

    /// 모든 인덱스 목록 조회
    pub fn list_indexes(&self) -> Vec<(String, String, String, usize)> {
        let indexes = self.indexes.read();

        indexes
            .values()
            .map(|entry| {
                (
                    entry.name.clone(),
                    entry.label.clone(),
                    entry.property.clone(),
                    entry.index.len(),
                )
            })
            .collect()
    }

    pub fn memory_size(&self) -> usize {
        let single = self
            .list_indexes()
            .into_iter()
            .map(|(name, label, property, entries)| {
                name.len()
                    .saturating_add(label.len())
                    .saturating_add(property.len())
                    .saturating_add(
                        entries.saturating_mul(
                            std::mem::size_of::<NodeId>()
                                .saturating_add(std::mem::size_of::<IndexValue>())
                                .saturating_mul(2),
                        ),
                    )
            })
            .fold(0usize, usize::saturating_add);
        let composite = self
            .list_composite_indexes()
            .into_iter()
            .map(|(name, label, properties, entries)| {
                name.len()
                    .saturating_add(label.len())
                    .saturating_add(
                        properties
                            .iter()
                            .map(String::len)
                            .fold(0usize, usize::saturating_add),
                    )
                    .saturating_add(
                        entries.saturating_mul(
                            std::mem::size_of::<NodeId>()
                                .saturating_add(
                                    properties
                                        .len()
                                        .saturating_mul(std::mem::size_of::<IndexValue>()),
                                )
                                .saturating_mul(2),
                        ),
                    )
            })
            .fold(0usize, usize::saturating_add);
        single.saturating_add(composite)
    }

    /// 특정 인덱스 조회
    pub fn get_index(&self, name: &str) -> Option<(String, String)> {
        self.indexes
            .read()
            .get(name)
            .map(|entry| (entry.label.clone(), entry.property.clone()))
    }

    // ========================================================================
    // 복합 프로퍼티 인덱스 메서드 (v1.79.0)
    // ========================================================================

    /// 복합 인덱스 생성
    ///
    /// # Arguments
    /// * `name` - 인덱스 이름
    /// * `label` - 대상 노드 라벨
    /// * `properties` - 대상 프로퍼티 목록 (순서 중요, 최소 2개)
    ///
    /// # Returns
    /// 성공 시 true, 이미 존재하거나 프로퍼티가 2개 미만이면 false
    pub fn create_composite_index(&self, name: &str, label: &str, properties: &[&str]) -> bool {
        // 최소 2개 프로퍼티 필요
        if properties.len() < 2 {
            return false;
        }

        let mut composite_indexes = self.composite_indexes.write();

        // 이름 중복 체크 (단일 인덱스와도 비교)
        if composite_indexes.contains_key(name) || self.indexes.read().contains_key(name) {
            return false;
        }

        let props: Vec<String> = properties.iter().map(|s| s.to_string()).collect();
        let entry = MultiPropertyIndexEntry {
            name: name.to_string(),
            label: label.to_string(),
            properties: props.clone(),
            index: MultiPropertyIndex::new(props.clone()),
            metadata: RwLock::new(IndexMetadata::new()),
        };

        composite_indexes.insert(name.to_string(), entry);

        let mut label_props_to_name = self.label_props_to_name.write();
        label_props_to_name.insert((label.to_string(), props), name.to_string());

        true
    }

    /// 복합 인덱스 삭제
    pub fn drop_composite_index(&self, name: &str) -> bool {
        let mut composite_indexes = self.composite_indexes.write();

        if let Some(entry) = composite_indexes.remove(name) {
            let mut label_props_to_name = self.label_props_to_name.write();
            label_props_to_name.remove(&(entry.label, entry.properties));
            true
        } else {
            false
        }
    }

    /// 복합 인덱스 존재 여부 확인 (이름으로)
    pub fn has_composite_index(&self, name: &str) -> bool {
        self.composite_indexes.read().contains_key(name)
    }

    /// 복합 인덱스 존재 여부 확인 (라벨+프로퍼티 목록으로)
    pub fn has_composite_index_for(&self, label: &str, properties: &[&str]) -> bool {
        let props: Vec<String> = properties.iter().map(|s| s.to_string()).collect();
        self.label_props_to_name
            .read()
            .contains_key(&(label.to_string(), props))
    }

    /// 노드를 복합 인덱스에 추가
    pub fn index_node_composite(
        &self,
        node_id: NodeId,
        labels: &[String],
        properties: &IndexMap<String, Value>,
    ) {
        let composite_indexes = self.composite_indexes.read();

        for entry in composite_indexes.values() {
            // 라벨이 일치하는지 확인
            if labels.contains(&entry.label) {
                entry.index.insert_from_properties(node_id, properties);
            }
        }
    }

    /// 노드를 복합 인덱스에서 제거
    pub fn unindex_node_composite(&self, node_id: NodeId, labels: &[String]) {
        let composite_indexes = self.composite_indexes.read();

        for entry in composite_indexes.values() {
            if labels.contains(&entry.label) {
                entry.index.remove(node_id);
            }
        }
    }

    /// 노드를 모든 인덱스(단일+복합)에 추가
    pub fn index_node_all(
        &self,
        node_id: NodeId,
        labels: &[String],
        properties: &IndexMap<String, Value>,
    ) {
        self.index_node(node_id, labels, properties);
        self.index_node_composite(node_id, labels, properties);
    }

    /// 노드를 모든 인덱스(단일+복합)에서 제거
    pub fn unindex_node_all(&self, node_id: NodeId, labels: &[String]) {
        self.unindex_node(node_id, labels);
        self.unindex_node_composite(node_id, labels);
    }

    /// 복합 인덱스를 사용하여 노드 검색
    ///
    /// # Arguments
    /// * `label` - 노드 라벨
    /// * `properties` - 프로퍼티 이름 목록
    /// * `values` - 검색할 값 목록 (properties와 동일한 순서)
    ///
    /// # Returns
    /// Some(노드 ID 목록) - 인덱스가 있고 검색 성공
    /// None - 해당 라벨+프로퍼티 조합에 대한 인덱스가 없음
    pub fn find_nodes_composite(
        &self,
        label: &str,
        properties: &[&str],
        values: &[&Value],
    ) -> Option<Vec<NodeId>> {
        if properties.len() != values.len() {
            return None;
        }

        let props: Vec<String> = properties.iter().map(|s| s.to_string()).collect();
        let label_props_to_name = self.label_props_to_name.read();

        if let Some(index_name) = label_props_to_name.get(&(label.to_string(), props)) {
            let composite_indexes = self.composite_indexes.read();
            if let Some(entry) = composite_indexes.get(index_name) {
                return Some(entry.index.find(values));
            }
        }

        None
    }

    /// 복합 인덱스에서 prefix 검색 (앞부분 프로퍼티만으로 검색)
    ///
    /// 예: (firstName, lastName) 인덱스에서 firstName만으로 검색
    pub fn find_nodes_composite_prefix(
        &self,
        label: &str,
        properties: &[&str],
        values: &[&Value],
    ) -> Option<Vec<NodeId>> {
        let props: Vec<String> = properties.iter().map(|s| s.to_string()).collect();
        let label_props_to_name = self.label_props_to_name.read();

        if let Some(index_name) = label_props_to_name.get(&(label.to_string(), props)) {
            let composite_indexes = self.composite_indexes.read();
            if let Some(entry) = composite_indexes.get(index_name) {
                return Some(entry.index.find_prefix(values));
            }
        }

        None
    }

    /// 모든 복합 인덱스 목록 조회
    pub fn list_composite_indexes(&self) -> Vec<(String, String, Vec<String>, usize)> {
        let composite_indexes = self.composite_indexes.read();

        composite_indexes
            .values()
            .map(|entry| {
                (
                    entry.name.clone(),
                    entry.label.clone(),
                    entry.properties.clone(),
                    entry.index.len(),
                )
            })
            .collect()
    }

    /// 특정 복합 인덱스 조회
    pub fn get_composite_index(&self, name: &str) -> Option<(String, Vec<String>)> {
        self.composite_indexes
            .read()
            .get(name)
            .map(|entry| (entry.label.clone(), entry.properties.clone()))
    }

    /// 모든 인덱스 초기화 (단일+복합)
    pub fn clear(&self) {
        self.indexes.write().clear();
        self.label_prop_to_name.write().clear();
        self.composite_indexes.write().clear();
        self.label_props_to_name.write().clear();
    }

    /// 단일 인덱스만 초기화
    pub fn clear_simple(&self) {
        self.indexes.write().clear();
        self.label_prop_to_name.write().clear();
    }

    /// 복합 인덱스만 초기화
    pub fn clear_composite(&self) {
        self.composite_indexes.write().clear();
        self.label_props_to_name.write().clear();
    }

    // ========================================================================
    // 인덱스 상태 관리 메서드 (v1.2.0)
    // ========================================================================

    /// 인덱스 상태 조회
    pub fn get_index_state(&self, name: &str) -> Option<IndexState> {
        // 단일 인덱스에서 찾기
        if let Some(entry) = self.indexes.read().get(name) {
            return Some(entry.metadata.read().state);
        }
        // 복합 인덱스에서 찾기
        if let Some(entry) = self.composite_indexes.read().get(name) {
            return Some(entry.metadata.read().state);
        }
        None
    }

    /// 인덱스 메타데이터 조회
    pub fn get_index_metadata(&self, name: &str) -> Option<IndexMetadata> {
        // 단일 인덱스에서 찾기
        if let Some(entry) = self.indexes.read().get(name) {
            return Some(entry.metadata.read().clone());
        }
        // 복합 인덱스에서 찾기
        if let Some(entry) = self.composite_indexes.read().get(name) {
            return Some(entry.metadata.read().clone());
        }
        None
    }

    /// 인덱스 상태를 Stale로 표시
    pub fn mark_index_stale(&self, name: &str, stale_count: usize) -> bool {
        // 단일 인덱스에서 찾기
        if let Some(entry) = self.indexes.read().get(name) {
            entry.metadata.write().mark_stale(stale_count);
            return true;
        }
        // 복합 인덱스에서 찾기
        if let Some(entry) = self.composite_indexes.read().get(name) {
            entry.metadata.write().mark_stale(stale_count);
            return true;
        }
        false
    }

    /// 인덱스 상태를 Clean으로 표시
    pub fn mark_index_clean(&self, name: &str) -> bool {
        // 단일 인덱스에서 찾기
        if let Some(entry) = self.indexes.read().get(name) {
            entry.metadata.write().mark_clean();
            return true;
        }
        // 복합 인덱스에서 찾기
        if let Some(entry) = self.composite_indexes.read().get(name) {
            entry.metadata.write().mark_clean();
            return true;
        }
        false
    }

    /// 인덱스 상태를 Rebuilding으로 표시
    pub fn mark_index_rebuilding(&self, name: &str) -> bool {
        // 단일 인덱스에서 찾기
        if let Some(entry) = self.indexes.read().get(name) {
            entry.metadata.write().mark_rebuilding();
            return true;
        }
        // 복합 인덱스에서 찾기
        if let Some(entry) = self.composite_indexes.read().get(name) {
            entry.metadata.write().mark_rebuilding();
            return true;
        }
        false
    }

    /// 모든 인덱스 상태 요약 조회
    pub fn get_all_states_summary(&self) -> IndexStatesSummary {
        let mut summary = IndexStatesSummary::new();

        // 단일 인덱스
        for entry in self.indexes.read().values() {
            let state = entry.metadata.read().state;
            summary.add(&entry.name, state);
        }

        // 복합 인덱스
        for entry in self.composite_indexes.read().values() {
            let state = entry.metadata.read().state;
            summary.add(&entry.name, state);
        }

        summary
    }

    /// 잠재적으로 stale인 인덱스 목록 조회 (시간 기반)
    pub fn get_potentially_stale_indexes(&self) -> Vec<String> {
        let mut stale_indexes = Vec::new();

        // 단일 인덱스
        for entry in self.indexes.read().values() {
            if entry.metadata.read().is_potentially_stale() {
                stale_indexes.push(entry.name.clone());
            }
        }

        // 복합 인덱스
        for entry in self.composite_indexes.read().values() {
            if entry.metadata.read().is_potentially_stale() {
                stale_indexes.push(entry.name.clone());
            }
        }

        stale_indexes
    }

    /// 단일 인덱스 검증 (실제 데이터와 비교)
    ///
    /// # Arguments
    /// * `name` - 인덱스 이름
    /// * `node_lookup` - 노드 ID로 (labels, properties) 조회하는 함수
    ///
    /// # Returns
    /// 검증 결과 또는 인덱스가 없으면 None
    pub fn verify_index<F>(&self, name: &str, node_lookup: F) -> Option<IndexVerificationResult>
    where
        F: Fn(NodeId) -> Option<(Vec<String>, IndexMap<String, Value>)>,
    {
        let start = Instant::now();

        let indexes = self.indexes.read();
        let entry = indexes.get(name)?;

        let mut total_entries = 0;
        let mut stale_entries = 0;

        // 인덱스의 모든 노드를 검증
        let node_to_value = entry.index.node_to_value.read();
        for (&node_id, indexed_value) in node_to_value.iter() {
            total_entries += 1;

            // 실제 노드 데이터 조회
            match node_lookup(node_id) {
                Some((labels, properties)) => {
                    // 라벨 확인
                    if !labels.contains(&entry.label) {
                        stale_entries += 1;
                        continue;
                    }
                    // 프로퍼티 값 확인
                    match properties.get(&entry.property) {
                        Some(actual_value) => {
                            let actual_index_value = IndexValue::from(actual_value);
                            if &actual_index_value != indexed_value {
                                stale_entries += 1;
                            }
                        }
                        None => {
                            // 프로퍼티가 없으면 stale
                            stale_entries += 1;
                        }
                    }
                }
                None => {
                    // 노드가 없으면 stale
                    stale_entries += 1;
                }
            }
        }

        drop(node_to_value);
        drop(indexes);

        let duration = start.elapsed();

        // 상태 업데이트
        if stale_entries > 0 {
            self.mark_index_stale(name, stale_entries);
        } else {
            self.mark_index_clean(name);
        }

        Some(IndexVerificationResult::with_mismatches(
            total_entries,
            stale_entries,
            0, // missing_entries는 별도 스캔 필요
            duration,
        ))
    }

    /// 모든 인덱스 검증
    pub fn verify_all_indexes<F>(&self, node_lookup: F) -> Vec<(String, IndexVerificationResult)>
    where
        F: Fn(NodeId) -> Option<(Vec<String>, IndexMap<String, Value>)> + Clone,
    {
        let mut results = Vec::new();

        // 단일 인덱스 이름 목록
        let index_names: Vec<String> = self.indexes.read().keys().cloned().collect();

        for name in index_names {
            if let Some(result) = self.verify_index(&name, node_lookup.clone()) {
                results.push((name, result));
            }
        }

        results
    }

    /// 인덱스 업데이트 기록 (내부용)
    #[expect(dead_code)]
    fn record_index_update(&self, name: &str) {
        // 단일 인덱스
        if let Some(entry) = self.indexes.read().get(name) {
            let count = entry.index.len();
            entry.metadata.write().record_update(count);
            return;
        }
        // 복합 인덱스
        if let Some(entry) = self.composite_indexes.read().get(name) {
            let count = entry.index.len();
            entry.metadata.write().record_update(count);
        }
    }

    /// 단일 인덱스 상태 정보 포함 목록 조회
    pub fn list_indexes_with_state(&self) -> Vec<(String, String, String, usize, IndexState)> {
        let indexes = self.indexes.read();

        indexes
            .values()
            .map(|entry| {
                (
                    entry.name.clone(),
                    entry.label.clone(),
                    entry.property.clone(),
                    entry.index.len(),
                    entry.metadata.read().state,
                )
            })
            .collect()
    }

    /// 복합 인덱스 상태 정보 포함 목록 조회
    pub fn list_composite_indexes_with_state(
        &self,
    ) -> Vec<(String, String, Vec<String>, usize, IndexState)> {
        let composite_indexes = self.composite_indexes.read();

        composite_indexes
            .values()
            .map(|entry| {
                (
                    entry.name.clone(),
                    entry.label.clone(),
                    entry.properties.clone(),
                    entry.index.len(),
                    entry.metadata.read().state,
                )
            })
            .collect()
    }
}

impl Default for SecondaryIndexManager {
    fn default() -> Self {
        Self::new()
    }
}
