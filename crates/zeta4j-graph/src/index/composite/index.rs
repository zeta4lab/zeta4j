// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 복합 인덱스 모듈

use super::key::CompositeKey;
use crate::core::Value;
use crate::error::{GraphError, Result};
use indexmap::IndexMap;
use std::collections::BTreeMap;

/// 복합 인덱스 정의
#[derive(Debug, Clone)]
pub struct CompositeIndexDef {
    /// 인덱스 이름
    pub name: String,
    /// 대상 레이블
    pub label: String,
    /// 속성 이름들 (순서 중요)
    pub properties: Vec<String>,
    /// 유니크 여부
    pub unique: bool,
    /// 생성 시간
    pub created_at: u64,
}

impl CompositeIndexDef {
    /// 새 복합 인덱스 정의 생성
    pub fn new(name: String, label: String, properties: Vec<String>) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());

        Self {
            name,
            label,
            properties,
            unique: false,
            created_at,
        }
    }

    /// 유니크 설정
    pub fn with_unique(mut self, unique: bool) -> Self {
        self.unique = unique;
        self
    }

    /// 속성 수
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }

    /// 속성 포함 여부
    pub fn contains_property(&self, property: &str) -> bool {
        self.properties.iter().any(|p| p == property)
    }

    /// 첫 번째 속성
    pub fn first_property(&self) -> Option<&str> {
        self.properties.first().map(|s| s.as_str())
    }

    /// 속성들이 쿼리 패턴과 일치하는지 확인
    ///
    /// 쿼리 속성이 인덱스 속성의 프리픽스이면 사용 가능
    pub fn matches_query_properties(&self, query_properties: &[String]) -> bool {
        if query_properties.is_empty() || query_properties.len() > self.properties.len() {
            return false;
        }

        for (i, prop) in query_properties.iter().enumerate() {
            if &self.properties[i] != prop {
                return false;
            }
        }

        true
    }
}

/// 복합 인덱스 통계
#[derive(Debug, Clone, Default)]
pub struct CompositeIndexStats {
    /// 총 엔트리 수
    pub entry_count: usize,
    /// 유니크 키 수
    pub unique_keys: usize,
    /// 키당 평균 값 수
    pub avg_values_per_key: f64,
}

/// 복합 인덱스
#[derive(Clone)]
pub struct CompositeIndex {
    /// 인덱스 정의
    def: CompositeIndexDef,
    /// B-Tree 기반 인덱스 (CompositeKey -> Vec<NodeId>)
    entries: BTreeMap<CompositeKey, Vec<u64>>,
    /// 총 엔트리 수
    entry_count: usize,
}

impl CompositeIndex {
    /// 새 복합 인덱스 생성
    pub fn new(def: CompositeIndexDef) -> Self {
        Self {
            def,
            entries: BTreeMap::new(),
            entry_count: 0,
        }
    }

    /// 인덱스 정의 반환
    pub fn definition(&self) -> &CompositeIndexDef {
        &self.def
    }

    /// 인덱스 이름
    pub fn name(&self) -> &str {
        &self.def.name
    }

    /// 대상 레이블
    pub fn label(&self) -> &str {
        &self.def.label
    }

    /// 속성들
    pub fn properties(&self) -> &[String] {
        &self.def.properties
    }

    /// 유니크 여부
    pub fn is_unique(&self) -> bool {
        self.def.unique
    }

    /// 속성들로부터 키 생성
    pub fn build_key(&self, properties: &IndexMap<String, Value>) -> Option<CompositeKey> {
        let mut values = Vec::with_capacity(self.def.properties.len());

        for prop_name in &self.def.properties {
            {
                let value = properties.get(prop_name)?;
                values.push(value.clone())
            }
        }

        Some(CompositeKey::new(values))
    }

    /// 엔트리 삽입
    pub fn insert(&mut self, key: CompositeKey, node_id: u64) -> Result<()> {
        if self.def.unique {
            // 유니크 인덱스인 경우 중복 체크
            if let Some(existing) = self.entries.get(&key)
                && !existing.is_empty()
                && !existing.contains(&node_id)
            {
                return Err(GraphError::Constraint {
                    constraint_name: "validation".to_string(),
                    message: format!(
                        "Unique constraint violation: index '{}', key {:?}",
                        self.def.name, key
                    ),
                });
            }
        }

        let entry = self.entries.entry(key).or_default();
        if !entry.contains(&node_id) {
            entry.push(node_id);
            self.entry_count += 1;
        }

        Ok(())
    }

    /// 엔트리 삭제
    pub fn remove(&mut self, key: &CompositeKey, node_id: u64) -> Result<bool> {
        if let Some(entry) = self.entries.get_mut(key)
            && let Some(pos) = entry.iter().position(|&id| id == node_id)
        {
            entry.remove(pos);
            self.entry_count -= 1;

            // 빈 엔트리 정리
            if entry.is_empty() {
                self.entries.remove(key);
            }

            return Ok(true);
        }

        Ok(false)
    }

    /// 정확한 키로 검색
    pub fn lookup(&self, key: &CompositeKey) -> Vec<u64> {
        self.entries.get(key).cloned().unwrap_or_default()
    }

    /// 프리픽스 검색 (첫 N개 속성 일치)
    pub fn lookup_prefix(&self, prefix: &[Value]) -> Vec<u64> {
        if prefix.is_empty() {
            // 빈 프리픽스는 모든 엔트리 반환
            return self.entries.values().flatten().copied().collect();
        }

        let mut result = Vec::new();

        for (key, node_ids) in &self.entries {
            if key.matches_prefix(prefix) {
                result.extend_from_slice(node_ids);
            }
        }

        result
    }

    /// 범위 검색
    pub fn lookup_range(&self, start: &CompositeKey, end: &CompositeKey) -> Vec<u64> {
        let mut result = Vec::new();

        for (key, node_ids) in self.entries.range(start..=end) {
            let _ = key; // key는 범위 내에 있음이 보장됨
            result.extend_from_slice(node_ids);
        }

        result
    }

    /// 첫 번째 속성만으로 검색
    #[allow(clippy::cloned_ref_to_slice_refs)]
    pub fn lookup_first(&self, value: &Value) -> Vec<u64> {
        self.lookup_prefix(&[value.clone()])
    }

    /// 엔트리 수
    pub fn len(&self) -> usize {
        self.entry_count
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }

    /// 유니크 키 수
    pub fn unique_key_count(&self) -> usize {
        self.entries.len()
    }

    /// 모든 엔트리 반복
    pub fn iter(&self) -> impl Iterator<Item = (&CompositeKey, &Vec<u64>)> {
        self.entries.iter()
    }

    /// 통계 정보
    pub fn stats(&self) -> CompositeIndexStats {
        let unique_keys = self.entries.len();
        let avg_values = if unique_keys > 0 {
            self.entry_count as f64 / unique_keys as f64
        } else {
            0.0
        };

        CompositeIndexStats {
            entry_count: self.entry_count,
            unique_keys,
            avg_values_per_key: avg_values,
        }
    }

    /// 인덱스 클리어
    pub fn clear(&mut self) {
        self.entries.clear();
        self.entry_count = 0;
    }

    pub fn memory_size(&self) -> usize {
        let definition = std::mem::size_of::<CompositeIndexDef>()
            .saturating_add(self.def.name.len())
            .saturating_add(self.def.label.len())
            .saturating_add(
                self.def
                    .properties
                    .iter()
                    .map(|property| std::mem::size_of::<String>().saturating_add(property.len()))
                    .fold(0usize, usize::saturating_add),
            );
        let entries = self
            .entries
            .iter()
            .map(|(key, node_ids)| {
                std::mem::size_of::<CompositeKey>()
                    .saturating_add(
                        key.values()
                            .iter()
                            .map(Value::memory_size)
                            .fold(0usize, usize::saturating_add),
                    )
                    .saturating_add(
                        node_ids
                            .capacity()
                            .saturating_mul(std::mem::size_of::<u64>()),
                    )
            })
            .fold(0usize, usize::saturating_add);
        definition.saturating_add(entries)
    }
}

impl std::fmt::Debug for CompositeIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeIndex")
            .field("name", &self.def.name)
            .field("label", &self.def.label)
            .field("properties", &self.def.properties)
            .field("unique", &self.def.unique)
            .field("entry_count", &self.entry_count)
            .field("unique_keys", &self.entries.len())
            .finish()
    }
}
