// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 함수 인덱스 매니저
//!
//! 여러 함수 인덱스를 통합 관리합니다.

use super::{FunctionExpression, FunctionIndex};
use crate::core::{NodeId, Value};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::collections::HashMap;

/// 함수 인덱스 정보
#[derive(Debug, Clone)]
pub struct FunctionIndexInfo {
    /// 인덱스 이름
    pub name: String,
    /// 대상 라벨
    pub label: String,
    /// 원본 프로퍼티
    pub property: String,
    /// 함수 표현식 (문자열 표현)
    pub expression: String,
    /// 인덱싱된 노드 수
    pub node_count: usize,
    /// 고유 값 개수
    pub distinct_count: usize,
}

/// 함수 인덱스 통계
#[derive(Debug, Clone, Default)]
pub struct FunctionIndexStats {
    /// 전체 인덱스 수
    pub index_count: usize,
    /// 전체 인덱싱된 노드 수
    pub total_nodes: usize,
    /// 인덱스별 통계
    pub per_index: Vec<FunctionIndexInfo>,
}

/// 함수 인덱스 매니저
///
/// 여러 함수 인덱스를 관리하고 노드 변경에 따른 인덱스 업데이트를 처리합니다.
#[derive(Debug)]
pub struct FunctionIndexManager {
    /// 인덱스 이름 -> FunctionIndex 매핑
    indexes: RwLock<HashMap<String, FunctionIndex>>,
    /// (라벨, 프로퍼티) -> 인덱스 이름 목록 매핑
    /// 한 프로퍼티에 여러 함수 인덱스가 있을 수 있음 (lower, upper 등)
    prop_to_indexes: RwLock<HashMap<(String, String), Vec<String>>>,
}

impl FunctionIndexManager {
    /// 새 함수 인덱스 매니저 생성
    pub fn new() -> Self {
        Self {
            indexes: RwLock::new(HashMap::new()),
            prop_to_indexes: RwLock::new(HashMap::new()),
        }
    }

    /// 함수 인덱스 생성
    ///
    /// # Arguments
    /// * `name` - 인덱스 이름
    /// * `label` - 대상 노드 라벨
    /// * `expression` - 함수 표현식
    ///
    /// # Returns
    /// 성공 시 true, 이미 존재하면 false
    pub fn create_index(&self, name: &str, label: &str, expression: FunctionExpression) -> bool {
        let mut indexes = self.indexes.write();

        if indexes.contains_key(name) {
            return false;
        }

        let property = expression.source_property().to_string();
        let index = FunctionIndex::new(name, label, expression);

        indexes.insert(name.to_string(), index);

        // 프로퍼티 -> 인덱스 매핑 추가
        let mut prop_to_indexes = self.prop_to_indexes.write();
        let key = (label.to_string(), property);
        prop_to_indexes
            .entry(key)
            .or_default()
            .push(name.to_string());

        true
    }

    /// 함수 인덱스 삭제
    ///
    /// # Arguments
    /// * `name` - 인덱스 이름
    ///
    /// # Returns
    /// 성공 시 true, 존재하지 않으면 false
    pub fn drop_index(&self, name: &str) -> bool {
        let mut indexes = self.indexes.write();

        if let Some(index) = indexes.remove(name) {
            // 프로퍼티 -> 인덱스 매핑에서도 제거
            let mut prop_to_indexes = self.prop_to_indexes.write();
            let key = (
                index.label().to_string(),
                index.source_property().to_string(),
            );
            if let Some(names) = prop_to_indexes.get_mut(&key) {
                names.retain(|n| n != name);
                if names.is_empty() {
                    prop_to_indexes.remove(&key);
                }
            }
            true
        } else {
            false
        }
    }

    /// 인덱스 존재 여부 확인 (이름으로)
    pub fn has_index(&self, name: &str) -> bool {
        self.indexes.read().contains_key(name)
    }

    /// 특정 함수 표현식에 대한 인덱스 찾기
    ///
    /// # Arguments
    /// * `label` - 노드 라벨
    /// * `expression` - 함수 표현식
    ///
    /// # Returns
    /// 일치하는 인덱스 이름 (있으면)
    pub fn find_index_for(&self, label: &str, expression: &FunctionExpression) -> Option<String> {
        let indexes = self.indexes.read();

        for (name, index) in indexes.iter() {
            if index.label() == label && index.expression() == expression {
                return Some(name.clone());
            }
        }

        None
    }

    /// 노드를 관련 인덱스에 추가
    ///
    /// 노드의 라벨과 프로퍼티를 확인하여 해당하는 모든 함수 인덱스에 추가합니다.
    ///
    /// # Arguments
    /// * `node_id` - 노드 ID
    /// * `labels` - 노드 라벨 목록
    /// * `properties` - 노드 프로퍼티
    pub fn index_node(
        &self,
        node_id: NodeId,
        labels: &[String],
        properties: &IndexMap<String, Value>,
    ) {
        let prop_to_indexes = self.prop_to_indexes.read();
        let indexes = self.indexes.read();

        for label in labels {
            for (prop_name, prop_value) in properties {
                let key = (label.clone(), prop_name.clone());
                if let Some(index_names) = prop_to_indexes.get(&key) {
                    for index_name in index_names {
                        if let Some(index) = indexes.get(index_name) {
                            index.insert(node_id, prop_value);
                        }
                    }
                }
            }
        }
    }

    /// 노드를 관련 인덱스에서 제거
    ///
    /// # Arguments
    /// * `node_id` - 노드 ID
    /// * `labels` - 노드 라벨 목록
    pub fn unindex_node(&self, node_id: NodeId, labels: &[String]) {
        let indexes = self.indexes.read();

        for index in indexes.values() {
            if labels.contains(&index.label().to_string()) {
                index.remove(node_id);
            }
        }
    }

    /// 노드 프로퍼티 업데이트 시 인덱스 갱신
    ///
    /// # Arguments
    /// * `node_id` - 노드 ID
    /// * `labels` - 노드 라벨 목록
    /// * `property` - 변경된 프로퍼티 이름
    /// * `new_value` - 새 값 (None이면 프로퍼티 삭제)
    pub fn update_node_property(
        &self,
        node_id: NodeId,
        labels: &[String],
        property: &str,
        new_value: Option<&Value>,
    ) {
        let prop_to_indexes = self.prop_to_indexes.read();
        let indexes = self.indexes.read();

        for label in labels {
            let key = (label.clone(), property.to_string());
            if let Some(index_names) = prop_to_indexes.get(&key) {
                for index_name in index_names {
                    if let Some(index) = indexes.get(index_name) {
                        if let Some(value) = new_value {
                            index.insert(node_id, value);
                        } else {
                            index.remove(node_id);
                        }
                    }
                }
            }
        }
    }

    /// 함수 인덱스를 사용하여 노드 검색
    ///
    /// # Arguments
    /// * `label` - 노드 라벨
    /// * `expression` - 함수 표현식
    /// * `value` - 검색할 계산된 값
    ///
    /// # Returns
    /// Some(노드 ID 목록) - 인덱스가 있고 검색 성공
    /// None - 해당하는 인덱스가 없음
    pub fn find_nodes(
        &self,
        label: &str,
        expression: &FunctionExpression,
        value: &Value,
    ) -> Option<Vec<NodeId>> {
        let indexes = self.indexes.read();

        for index in indexes.values() {
            if index.label() == label && index.expression() == expression {
                return Some(index.find(value));
            }
        }

        None
    }

    /// 인덱스 이름으로 노드 검색
    ///
    /// # Arguments
    /// * `index_name` - 인덱스 이름
    /// * `value` - 검색할 계산된 값
    ///
    /// # Returns
    /// Some(노드 ID 목록) - 인덱스가 있고 검색 성공
    /// None - 해당하는 인덱스가 없음
    pub fn find_nodes_by_index_name(&self, index_name: &str, value: &Value) -> Option<Vec<NodeId>> {
        let indexes = self.indexes.read();
        indexes.get(index_name).map(|index| index.find(value))
    }

    /// 모든 인덱스 목록 조회
    pub fn list_indexes(&self) -> Vec<FunctionIndexInfo> {
        let indexes = self.indexes.read();

        indexes
            .values()
            .map(|index| FunctionIndexInfo {
                name: index.name().to_string(),
                label: index.label().to_string(),
                property: index.source_property().to_string(),
                expression: index.expression().to_string(),
                node_count: index.len(),
                distinct_count: index.distinct_values(),
            })
            .collect()
    }

    /// 특정 인덱스 정보 조회
    pub fn get_index_info(&self, name: &str) -> Option<FunctionIndexInfo> {
        let indexes = self.indexes.read();
        indexes.get(name).map(|index| FunctionIndexInfo {
            name: index.name().to_string(),
            label: index.label().to_string(),
            property: index.source_property().to_string(),
            expression: index.expression().to_string(),
            node_count: index.len(),
            distinct_count: index.distinct_values(),
        })
    }

    /// 통계 조회
    pub fn get_stats(&self) -> FunctionIndexStats {
        let list = self.list_indexes();
        FunctionIndexStats {
            index_count: list.len(),
            total_nodes: list.iter().map(|i| i.node_count).sum(),
            per_index: list,
        }
    }

    pub fn memory_size(&self) -> usize {
        self.list_indexes()
            .into_iter()
            .map(|index| {
                std::mem::size_of::<FunctionIndexInfo>()
                    .saturating_add(index.name.len())
                    .saturating_add(index.label.len())
                    .saturating_add(index.property.len())
                    .saturating_add(index.expression.len())
                    .saturating_add(
                        index.node_count.saturating_mul(
                            std::mem::size_of::<NodeId>()
                                .saturating_add(std::mem::size_of::<Value>())
                                .saturating_mul(2),
                        ),
                    )
            })
            .fold(0usize, usize::saturating_add)
    }

    /// 모든 인덱스 초기화
    pub fn clear(&self) {
        self.indexes.write().clear();
        self.prop_to_indexes.write().clear();
    }

    /// 특정 라벨에 대한 모든 함수 인덱스 이름 조회
    pub fn indexes_for_label(&self, label: &str) -> Vec<String> {
        let indexes = self.indexes.read();
        indexes
            .values()
            .filter(|index| index.label() == label)
            .map(|index| index.name().to_string())
            .collect()
    }

    /// 특정 프로퍼티에 대한 모든 함수 인덱스 이름 조회
    pub fn indexes_for_property(&self, label: &str, property: &str) -> Vec<String> {
        let prop_to_indexes = self.prop_to_indexes.read();
        let key = (label.to_string(), property.to_string());
        prop_to_indexes.get(&key).cloned().unwrap_or_default()
    }
}

impl Default for FunctionIndexManager {
    fn default() -> Self {
        Self::new()
    }
}
