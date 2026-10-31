// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 해시 기반 고성능 인덱스

use parking_lot::RwLock;
use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;

/// 범용 해시 인덱스 구조
///
/// BTreeSet을 사용하여 값들이 정렬된 순서로 반환됩니다.
/// NodeId/EdgeId는 삽입 순서대로 증가하므로 BTreeSet 정렬 = 삽입 순서
#[derive(Debug)]
pub struct HashIndex<K, V>
where
    K: Hash + Eq + Clone + Ord,
    V: Hash + Eq + Clone + Ord,
{
    /// 키-값 매핑 (키 -> 값들의 집합, 정렬됨)
    index: RwLock<HashMap<K, BTreeSet<V>>>,

    /// 역방향 매핑 (값 -> 키들의 집합, 정렬됨)
    reverse_index: RwLock<HashMap<V, BTreeSet<K>>>,
}

impl<K, V> HashIndex<K, V>
where
    K: Hash + Eq + Clone + Ord,
    V: Hash + Eq + Clone + Ord,
{
    /// 새로운 해시 인덱스 생성
    pub fn new() -> Self {
        Self {
            index: RwLock::new(HashMap::new()),
            reverse_index: RwLock::new(HashMap::new()),
        }
    }

    /// 초기 용량을 지정한 해시 인덱스 생성
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            index: RwLock::new(HashMap::with_capacity(capacity)),
            reverse_index: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }

    /// 키-값 매핑 추가
    pub fn insert(&self, key: K, value: V) {
        let mut index = self.index.write();
        let mut reverse_index = self.reverse_index.write();

        // 정방향 인덱스에 추가
        index.entry(key.clone()).or_default().insert(value.clone());

        // 역방향 인덱스에 추가
        reverse_index.entry(value).or_default().insert(key);
    }

    /// 키에 해당하는 모든 값들 조회
    pub fn get(&self, key: &K) -> Vec<V> {
        let index = self.index.read();
        index
            .get(key)
            .map_or(Vec::new(), |values| values.iter().cloned().collect())
    }

    /// 값에 해당하는 모든 키들 조회 (역방향)
    pub fn get_reverse(&self, value: &V) -> Vec<K> {
        let reverse_index = self.reverse_index.read();
        reverse_index
            .get(value)
            .map_or(Vec::new(), |keys| keys.iter().cloned().collect())
    }

    /// 키-값 매핑 제거
    pub fn remove(&self, key: &K, value: &V) -> bool {
        let mut index = self.index.write();
        let mut reverse_index = self.reverse_index.write();

        let mut removed = false;

        // 정방향 인덱스에서 제거
        if let Some(values) = index.get_mut(key)
            && values.remove(value)
        {
            removed = true;
            // 빈 집합이면 키 자체를 제거
            if values.is_empty() {
                index.remove(key);
            }
        }

        // 역방향 인덱스에서 제거
        if let Some(keys) = reverse_index.get_mut(value) {
            keys.remove(key);
            // 빈 집합이면 값 자체를 제거
            if keys.is_empty() {
                reverse_index.remove(value);
            }
        }

        removed
    }

    /// 키의 모든 값들 제거
    pub fn remove_key(&self, key: &K) -> Vec<V> {
        let mut index = self.index.write();
        let mut reverse_index = self.reverse_index.write();

        match index.remove(key) {
            Some(values) => {
                // 역방향 인덱스에서도 제거
                for value in &values {
                    if let Some(keys) = reverse_index.get_mut(value) {
                        keys.remove(key);
                        if keys.is_empty() {
                            reverse_index.remove(value);
                        }
                    }
                }

                values.into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    /// 값의 모든 키들 제거 (역방향)
    pub fn remove_value(&self, value: &V) -> Vec<K> {
        let mut index = self.index.write();
        let mut reverse_index = self.reverse_index.write();

        match reverse_index.remove(value) {
            Some(keys) => {
                // 정방향 인덱스에서도 제거
                for key in &keys {
                    if let Some(values) = index.get_mut(key) {
                        values.remove(value);
                        if values.is_empty() {
                            index.remove(key);
                        }
                    }
                }

                keys.into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    /// 키가 존재하는지 확인
    pub fn contains_key(&self, key: &K) -> bool {
        let index = self.index.read();
        index.contains_key(key)
    }

    /// 값이 존재하는지 확인
    pub fn contains_value(&self, value: &V) -> bool {
        let reverse_index = self.reverse_index.read();
        reverse_index.contains_key(value)
    }

    /// 특정 키-값 매핑이 존재하는지 확인
    pub fn contains(&self, key: &K, value: &V) -> bool {
        let index = self.index.read();
        index.get(key).is_some_and(|values| values.contains(value))
    }

    /// 모든 키들 조회
    pub fn keys(&self) -> Vec<K> {
        let index = self.index.read();
        index.keys().cloned().collect()
    }

    /// 모든 값들 조회
    pub fn values(&self) -> Vec<V> {
        let reverse_index = self.reverse_index.read();
        reverse_index.keys().cloned().collect()
    }

    /// 키 개수
    pub fn key_count(&self) -> usize {
        let index = self.index.read();
        index.len()
    }

    /// 값 개수 (고유한 값들)
    pub fn value_count(&self) -> usize {
        let reverse_index = self.reverse_index.read();
        reverse_index.len()
    }

    /// 총 매핑 개수
    pub fn total_mappings(&self) -> usize {
        let index = self.index.read();
        index.values().map(|values| values.len()).sum()
    }

    /// 키에 해당하는 값 개수
    pub fn key_value_count(&self, key: &K) -> usize {
        let index = self.index.read();
        index.get(key).map_or(0, |values| values.len())
    }

    /// 인덱스가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        let index = self.index.read();
        index.is_empty()
    }

    /// 모든 매핑 제거
    pub fn clear(&self) {
        let mut index = self.index.write();
        let mut reverse_index = self.reverse_index.write();

        index.clear();
        reverse_index.clear();
    }

    /// 메모리 사용량 추정 (대략적)
    pub fn memory_usage(&self) -> usize {
        let index = self.index.read();
        let reverse_index = self.reverse_index.read();

        let mut size = std::mem::size_of::<Self>();
        size += index.capacity() * std::mem::size_of::<(K, BTreeSet<V>)>();
        size += reverse_index.capacity() * std::mem::size_of::<(V, BTreeSet<K>)>();

        // 각 BTreeSet의 크기도 추가 (대략적)
        for values in index.values() {
            size += values.len() * std::mem::size_of::<V>();
        }

        for keys in reverse_index.values() {
            size += keys.len() * std::mem::size_of::<K>();
        }

        size
    }

    /// 인덱스 압축 (메모리 최적화)
    ///
    /// HashMap 자체의 용량을 줄입니다.
    /// BTreeSet은 자동으로 메모리를 관리하므로 별도 압축 불필요.
    pub fn compact(&self) {
        let mut index = self.index.write();
        let mut reverse_index = self.reverse_index.write();

        // HashMap 자체 압축
        index.shrink_to_fit();
        reverse_index.shrink_to_fit();
    }
}

impl<K, V> Default for HashIndex<K, V>
where
    K: Hash + Eq + Clone + Ord,
    V: Hash + Eq + Clone + Ord,
{
    fn default() -> Self {
        Self::new()
    }
}
