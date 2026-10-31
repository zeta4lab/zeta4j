// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Record - 쿼리 결과 레코드
//!
//! 쿼리 결과의 단일 레코드

use indexmap::IndexMap;

use std::collections::HashMap;
use std::fmt;

use super::error::{DriverError, DriverResult};
use super::types::{Node, Path, Relationship, Value};

// ============================================================================
// Record - 단일 레코드
// ============================================================================

/// 쿼리 결과 레코드
#[derive(Debug, Clone)]
pub struct Record {
    /// 컬럼 키
    keys: Vec<String>,
    /// 값들
    values: Vec<Value>,
    /// 키-인덱스 매핑
    key_index: HashMap<String, usize>,
}

impl Record {
    /// 새 레코드 생성
    pub fn new(keys: Vec<String>, values: Vec<Value>) -> Self {
        let key_index = keys
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), i))
            .collect();

        Self {
            keys,
            values,
            key_index,
        }
    }

    /// 빈 레코드 생성
    pub fn empty() -> Self {
        Self::new(vec![], vec![])
    }

    /// 키 목록
    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// 값 목록
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// 레코드 길이
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 빈 레코드 여부
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// 키로 값 가져오기
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.key_index.get(key).and_then(|&i| self.values.get(i))
    }

    /// 인덱스로 값 가져오기
    pub fn get_by_index(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    /// 키로 타입 변환된 값 가져오기
    pub fn get_as<T>(&self, key: &str) -> DriverResult<T>
    where
        T: TryFrom<Value, Error = DriverError>,
    {
        self.get(key)
            .cloned()
            .ok_or_else(|| DriverError::type_conversion(format!("Key '{}' not found", key)))
            .and_then(|v| T::try_from(v))
    }

    /// Boolean 값 가져오기
    pub fn get_bool(&self, key: &str) -> DriverResult<bool> {
        self.get_as::<bool>(key)
    }

    /// Integer 값 가져오기
    pub fn get_int(&self, key: &str) -> DriverResult<i64> {
        self.get_as::<i64>(key)
    }

    /// Float 값 가져오기
    pub fn get_float(&self, key: &str) -> DriverResult<f64> {
        self.get_as::<f64>(key)
    }

    /// String 값 가져오기
    pub fn get_string(&self, key: &str) -> DriverResult<String> {
        self.get_as::<String>(key)
    }

    /// Node 값 가져오기
    pub fn get_node(&self, key: &str) -> DriverResult<Node> {
        self.get_as::<Node>(key)
    }

    /// Relationship 값 가져오기
    pub fn get_relationship(&self, key: &str) -> DriverResult<Relationship> {
        self.get_as::<Relationship>(key)
    }

    /// Path 값 가져오기
    pub fn get_path(&self, key: &str) -> DriverResult<Path> {
        self.get_as::<Path>(key)
    }

    /// Optional 값 가져오기 (None은 Null)
    pub fn get_optional<T>(&self, key: &str) -> DriverResult<Option<T>>
    where
        T: TryFrom<Value, Error = DriverError>,
    {
        match self.get(key) {
            Some(Value::Null) | None => Ok(None),
            Some(v) => T::try_from(v.clone()).map(Some),
        }
    }

    /// Map으로 변환
    pub fn to_map(&self) -> IndexMap<String, Value> {
        self.keys
            .iter()
            .cloned()
            .zip(self.values.iter().cloned())
            .collect()
    }

    /// 키 존재 여부
    pub fn contains_key(&self, key: &str) -> bool {
        self.key_index.contains_key(key)
    }
}

impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pairs: Vec<String> = self
            .keys
            .iter()
            .zip(self.values.iter())
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        write!(f, "{{{}}}", pairs.join(", "))
    }
}

impl IntoIterator for Record {
    type Item = (String, Value);
    type IntoIter = std::iter::Zip<std::vec::IntoIter<String>, std::vec::IntoIter<Value>>;

    fn into_iter(self) -> Self::IntoIter {
        self.keys.into_iter().zip(self.values)
    }
}

impl<'a> IntoIterator for &'a Record {
    type Item = (&'a String, &'a Value);
    type IntoIter = std::iter::Zip<std::slice::Iter<'a, String>, std::slice::Iter<'a, Value>>;

    fn into_iter(self) -> Self::IntoIter {
        self.keys.iter().zip(self.values.iter())
    }
}

// ============================================================================
// RecordStream - 레코드 스트림
// ============================================================================

/// 레코드 스트림 (결과 반복자)
#[derive(Debug)]
pub struct RecordStream {
    records: Vec<Record>,
    current: usize,
}

impl RecordStream {
    /// 새 스트림 생성
    pub fn new(records: Vec<Record>) -> Self {
        Self {
            records,
            current: 0,
        }
    }

    /// 빈 스트림 생성
    pub fn empty() -> Self {
        Self::new(vec![])
    }

    /// 남은 레코드 수
    pub fn remaining(&self) -> usize {
        self.records.len().saturating_sub(self.current)
    }

    /// 모든 레코드 가져오기
    pub fn collect_all(self) -> Vec<Record> {
        self.records
    }

    /// 첫 번째 레코드만 가져오기
    pub fn single(mut self) -> DriverResult<Record> {
        if self.records.len() != 1 {
            return Err(DriverError::type_conversion(format!(
                "Expected single record, got {}",
                self.records.len()
            )));
        }
        Ok(self.records.remove(0))
    }

    /// 첫 번째 레코드 가져오기 (없으면 None)
    pub fn first(mut self) -> Option<Record> {
        if self.records.is_empty() {
            None
        } else {
            Some(self.records.remove(0))
        }
    }

    /// 레코드 키
    pub fn keys(&self) -> Option<&[String]> {
        self.records.first().map(|r| r.keys())
    }
}

impl Iterator for RecordStream {
    type Item = Record;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.records.len() {
            let record = self.records[self.current].clone();
            self.current += 1;
            Some(record)
        } else {
            None
        }
    }
}
