//! 그래프 속성 시스템

use crate::core::{MemorySized, Value};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// 노드나 엣지의 속성을 나타내는 타입 (삽입 순서 유지)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    /// 속성 키-값 맵 (삽입 순서 유지)
    properties: IndexMap<String, Value>,
}

impl Property {
    /// 새로운 빈 속성 객체 생성
    pub fn new() -> Self {
        Self {
            properties: IndexMap::new(),
        }
    }

    /// 속성 설정
    pub fn set<K: Into<String>, V: Into<Value>>(&mut self, key: K, value: V) {
        self.properties.insert(key.into(), value.into());
    }

    /// 속성 값 조회
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }

    /// 속성 제거
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.properties.shift_remove(key)
    }

    /// 속성이 존재하는지 확인
    pub fn contains_key(&self, key: &str) -> bool {
        self.properties.contains_key(key)
    }

    /// 모든 속성 키 반복자
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.properties.keys()
    }

    /// 모든 속성 값 반복자
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.properties.values()
    }

    /// 모든 속성 키-값 쌍 반복자
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.properties.iter()
    }

    /// 속성 개수
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// 속성이 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// 모든 속성 제거
    pub fn clear(&mut self) {
        self.properties.clear();
    }

    /// 다른 속성과 병합
    pub fn merge(&mut self, other: &Property) {
        for (key, value) in &other.properties {
            self.properties.insert(key.clone(), value.clone());
        }
    }

    /// 속성의 메모리 사용량 추정 (바이트)
    pub fn memory_size(&self) -> usize {
        self.properties
            .iter()
            .map(|(key, value)| {
                std::mem::size_of::<String>()
                    .saturating_add(key.len())
                    .saturating_add(value.memory_size())
            })
            .fold(0usize, usize::saturating_add)
    }

    /// 속성을 IndexMap으로 변환
    pub fn as_map(&self) -> &IndexMap<String, Value> {
        &self.properties
    }

    /// 속성을 가변 IndexMap으로 변환
    pub fn as_map_mut(&mut self) -> &mut IndexMap<String, Value> {
        &mut self.properties
    }
}

impl Default for Property {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySized for Property {
    fn memory_size(&self) -> usize {
        Self::memory_size(self)
    }
}

impl From<IndexMap<String, Value>> for Property {
    fn from(properties: IndexMap<String, Value>) -> Self {
        Self { properties }
    }
}

impl From<Property> for IndexMap<String, Value> {
    fn from(prop: Property) -> Self {
        prop.properties
    }
}
