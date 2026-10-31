// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 복합 인덱스 관리자 모듈

use super::index::{CompositeIndex, CompositeIndexDef, CompositeIndexStats};
use super::key::CompositeKey;
use crate::core::Value;
use crate::error::{GraphError, Result};
use indexmap::IndexMap;
use std::collections::HashMap;

/// 복합 인덱스 관리자
///
/// 여러 복합 인덱스를 통합 관리하고, 노드 변경 시 자동으로 인덱스를 업데이트합니다.
#[derive(Clone)]
pub struct CompositeIndexManager {
    /// 인덱스 이름 -> 인덱스
    indexes_by_name: HashMap<String, CompositeIndex>,
    /// 레이블 -> 인덱스 이름들
    indexes_by_label: HashMap<String, Vec<String>>,
}

impl CompositeIndexManager {
    /// 새 관리자 생성
    pub fn new() -> Self {
        Self {
            indexes_by_name: HashMap::new(),
            indexes_by_label: HashMap::new(),
        }
    }

    /// 복합 인덱스 생성
    pub fn create_index(&mut self, def: CompositeIndexDef) -> Result<()> {
        if self.indexes_by_name.contains_key(&def.name) {
            return Err(GraphError::Index {
                message: format!("Index '{}' already exists", def.name),
            });
        }

        if def.properties.is_empty() {
            return Err(GraphError::Index {
                message: "Composite index requires at least one property".to_string(),
            });
        }

        let name = def.name.clone();
        let label = def.label.clone();
        let index = CompositeIndex::new(def);

        self.indexes_by_name.insert(name.clone(), index);
        self.indexes_by_label.entry(label).or_default().push(name);

        Ok(())
    }

    /// 복합 인덱스 삭제
    pub fn drop_index(&mut self, name: &str) -> Result<bool> {
        if let Some(index) = self.indexes_by_name.remove(name) {
            let label = index.label().to_string();

            if let Some(names) = self.indexes_by_label.get_mut(&label) {
                names.retain(|n| n != name);
                if names.is_empty() {
                    self.indexes_by_label.remove(&label);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    /// 인덱스 조회
    pub fn get_index(&self, name: &str) -> Option<&CompositeIndex> {
        self.indexes_by_name.get(name)
    }

    /// 인덱스 변경 가능 조회
    pub fn get_index_mut(&mut self, name: &str) -> Option<&mut CompositeIndex> {
        self.indexes_by_name.get_mut(name)
    }

    /// 레이블의 모든 인덱스
    pub fn indexes_for_label(&self, label: &str) -> Vec<&CompositeIndex> {
        match self.indexes_by_label.get(label) {
            Some(names) => names
                .iter()
                .filter_map(|name| self.indexes_by_name.get(name))
                .collect(),
            None => Vec::new(),
        }
    }

    /// 쿼리에 적합한 인덱스 찾기
    ///
    /// 주어진 레이블과 속성 목록에 가장 잘 맞는 인덱스를 반환합니다.
    /// 속성 목록이 인덱스 속성의 프리픽스여야 합니다.
    pub fn find_matching_index(
        &self,
        label: &str,
        properties: &[String],
    ) -> Option<&CompositeIndex> {
        let indexes = self.indexes_for_label(label);

        if indexes.is_empty() {
            return None;
        }

        // 가장 많은 속성을 커버하는 인덱스 선택
        let mut best_match: Option<&CompositeIndex> = None;
        let mut best_match_count = 0;

        for index in indexes {
            if index.definition().matches_query_properties(properties) {
                let match_count = properties.len();
                if match_count > best_match_count {
                    best_match = Some(index);
                    best_match_count = match_count;
                }
            }
        }

        best_match
    }

    /// 첫 번째 속성으로 인덱스 찾기
    pub fn find_index_by_first_property(
        &self,
        label: &str,
        property: &str,
    ) -> Option<&CompositeIndex> {
        self.indexes_for_label(label)
            .into_iter()
            .find(|index| index.definition().first_property() == Some(property))
    }

    /// 노드 삽입 시 인덱스 업데이트
    pub fn on_node_insert(
        &mut self,
        label: &str,
        node_id: u64,
        properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        let index_names: Vec<String> = self
            .indexes_by_label
            .get(label)
            .cloned()
            .unwrap_or_default();

        for name in index_names {
            if let Some(index) = self.indexes_by_name.get_mut(&name)
                && let Some(key) = index.build_key(properties)
            {
                index.insert(key, node_id)?;
            }
        }

        Ok(())
    }

    /// 노드 삭제 시 인덱스 업데이트
    pub fn on_node_delete(
        &mut self,
        label: &str,
        node_id: u64,
        properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        let index_names: Vec<String> = self
            .indexes_by_label
            .get(label)
            .cloned()
            .unwrap_or_default();

        for name in index_names {
            if let Some(index) = self.indexes_by_name.get_mut(&name)
                && let Some(key) = index.build_key(properties)
            {
                index.remove(&key, node_id)?;
            }
        }

        Ok(())
    }

    /// 속성 업데이트 시 인덱스 업데이트
    ///
    /// 변경된 속성이 인덱스에 포함된 경우에만 업데이트합니다.
    pub fn on_property_update(
        &mut self,
        label: &str,
        node_id: u64,
        property: &str,
        old_value: Option<&Value>,
        new_value: Option<&Value>,
        all_properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        let index_names: Vec<String> = self
            .indexes_by_label
            .get(label)
            .cloned()
            .unwrap_or_default();

        for name in index_names {
            if let Some(index) = self.indexes_by_name.get_mut(&name) {
                // 이 인덱스가 변경된 속성을 포함하는지 확인
                if !index.definition().contains_property(property) {
                    continue;
                }

                // 이전 키로 삭제
                if old_value.is_some() {
                    let mut old_props = all_properties.clone();
                    if let Some(old) = old_value {
                        old_props.insert(property.to_string(), old.clone());
                    }

                    if let Some(old_key) = index.build_key(&old_props) {
                        index.remove(&old_key, node_id)?;
                    }
                }

                // 새 키로 삽입
                if new_value.is_some()
                    && let Some(new_key) = index.build_key(all_properties)
                {
                    index.insert(new_key, node_id)?;
                }
            }
        }

        Ok(())
    }

    /// 모든 인덱스 정의 목록
    pub fn list_indexes(&self) -> Vec<&CompositeIndexDef> {
        self.indexes_by_name
            .values()
            .map(|index| index.definition())
            .collect()
    }

    /// 인덱스 수
    pub fn index_count(&self) -> usize {
        self.indexes_by_name.len()
    }

    /// 인덱스 존재 여부
    pub fn has_index(&self, name: &str) -> bool {
        self.indexes_by_name.contains_key(name)
    }

    /// 모든 인덱스 통계
    pub fn all_stats(&self) -> HashMap<String, CompositeIndexStats> {
        self.indexes_by_name
            .iter()
            .map(|(name, index)| (name.clone(), index.stats()))
            .collect()
    }

    /// 인덱스 검색 (정확한 키)
    pub fn lookup(&self, label: &str, properties: &[String], values: &[Value]) -> Option<Vec<u64>> {
        if properties.len() != values.len() {
            return None;
        }

        if let Some(index) = self.find_matching_index(label, properties) {
            let key = CompositeKey::new(values.to_vec());
            Some(index.lookup(&key))
        } else {
            None
        }
    }

    /// 인덱스 범위 검색
    pub fn lookup_range(
        &self,
        index_name: &str,
        start: &CompositeKey,
        end: &CompositeKey,
    ) -> Option<Vec<u64>> {
        self.indexes_by_name
            .get(index_name)
            .map(|index| index.lookup_range(start, end))
    }

    /// 모든 인덱스 클리어
    pub fn clear_all(&mut self) {
        for index in self.indexes_by_name.values_mut() {
            index.clear();
        }
    }

    pub fn memory_size(&self) -> usize {
        let indexes = self
            .indexes_by_name
            .iter()
            .map(|(name, index)| {
                std::mem::size_of::<String>()
                    .saturating_add(name.len())
                    .saturating_add(index.memory_size())
            })
            .fold(0usize, usize::saturating_add);
        let labels = self
            .indexes_by_label
            .iter()
            .map(|(label, names)| {
                label.len().saturating_add(
                    names
                        .iter()
                        .map(|name| std::mem::size_of::<String>().saturating_add(name.len()))
                        .fold(0usize, usize::saturating_add),
                )
            })
            .fold(0usize, usize::saturating_add);
        indexes.saturating_add(labels)
    }
}

impl Default for CompositeIndexManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CompositeIndexManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeIndexManager")
            .field("index_count", &self.indexes_by_name.len())
            .field("labels", &self.indexes_by_label.keys().collect::<Vec<_>>())
            .finish()
    }
}
