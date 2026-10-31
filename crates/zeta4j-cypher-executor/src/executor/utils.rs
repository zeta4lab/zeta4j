// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 유틸리티 함수 모듈
//!
//! 값 비교, 변환, 중복 제거 등 헬퍼 함수들

use super::{ExecutionContext, QueryExecutor};
use std::collections::HashSet;
use zeta4j_common::{ZetaError, codes};
use zeta4j_cypher_core::ast::*;
use zeta4j_graph::GraphError;
use zeta4j_graph::core::Value;

// =============================================================================
// Subscript 헬퍼 함수
// =============================================================================

/// 배열/문자열/맵에서 subscript 접근 (음수 인덱싱 지원)
///
/// # 예시
///
/// ```ignore
/// // 배열 접근
/// apply_subscript(&Value::Array(vec![1, 2, 3]), &Value::Integer(0)) // => 1
/// apply_subscript(&Value::Array(vec![1, 2, 3]), &Value::Integer(-1)) // => 3
///
/// // 문자열 접근
/// apply_subscript(&Value::String("hello".into()), &Value::Integer(1)) // => "e"
///
/// // 맵 접근
/// apply_subscript(&map_value, &Value::String("key".into())) // => map["key"]
/// ```
#[allow(dead_code)]
pub fn apply_subscript(container: &Value, index: &Value) -> Result<Value, GraphError> {
    // NULL 전파
    if matches!(container, Value::Null) || matches!(index, Value::Null) {
        return Ok(Value::Null);
    }

    match (container, index) {
        // 배열[정수]
        (Value::Array(arr), Value::Integer(idx)) => {
            let actual_idx = resolve_index(*idx, arr.len())?;
            Ok(arr.get(actual_idx).cloned().unwrap_or(Value::Null))
        }
        // 문자열[정수]
        (Value::String(s), Value::Integer(idx)) => {
            let chars: Vec<char> = s.chars().collect();
            let actual_idx = resolve_index(*idx, chars.len())?;
            Ok(chars
                .get(actual_idx)
                .map(|c| Value::String(c.to_string()))
                .unwrap_or(Value::Null))
        }
        // 맵[문자열]
        (Value::Map(map), Value::String(key)) => Ok(map.get(key).cloned().unwrap_or(Value::Null)),
        // 타입 불일치
        _ => Err(GraphError::Zeta(ZetaError::TypeMismatch {
            code: codes::cypher::TYPE_MISMATCH,
            expected: "Array, String, or Map with valid index".to_string(),
            found: format!("{}[{}]", container.type_name(), index.type_name()),
        })),
    }
}

/// 음수 인덱스를 양수로 변환
///
/// Cypher 시맨틱: 음수 인덱스는 끝에서부터 계산
/// - `arr[-1]` => 마지막 요소
/// - `arr[-2]` => 뒤에서 두 번째 요소
#[allow(dead_code)]
fn resolve_index(idx: i64, len: usize) -> Result<usize, GraphError> {
    let actual = if idx < 0 {
        let pos = len as i64 + idx;
        if pos < 0 {
            return Err(GraphError::Zeta(ZetaError::QueryExecution {
                code: codes::cypher::INDEX_ACCESS_FAILED,
                message: format!("Index {} out of bounds (len: {})", idx, len),
            }));
        }
        pos as usize
    } else {
        idx as usize
    };

    if actual >= len {
        Err(GraphError::Zeta(ZetaError::QueryExecution {
            code: codes::cypher::INDEX_ACCESS_FAILED,
            message: format!("Index {} out of bounds (len: {})", idx, len),
        }))
    } else {
        Ok(actual)
    }
}

impl QueryExecutor {
    /// 결과 행에서 중복 제거 (RETURN DISTINCT 용)
    pub(super) fn apply_distinct_rows(&self, rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
        let mut seen: HashSet<String> = HashSet::new();
        rows.into_iter()
            .filter(|row| {
                let key = self.row_to_key(row);
                seen.insert(key)
            })
            .collect()
    }

    /// 결과 행을 해시 키로 변환
    pub(super) fn row_to_key(&self, row: &[Value]) -> String {
        row.iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join("|")
    }

    /// 컨텍스트에서 중복 제거 (WITH DISTINCT 용)
    pub(super) fn apply_distinct_contexts(
        &self,
        contexts: Vec<ExecutionContext>,
    ) -> Vec<ExecutionContext> {
        let mut seen: HashSet<String> = HashSet::new();
        contexts
            .into_iter()
            .filter(|ctx| {
                let key = self.context_to_key(ctx);
                seen.insert(key)
            })
            .collect()
    }

    /// 컨텍스트를 해시 키로 변환
    pub(super) fn context_to_key(&self, ctx: &ExecutionContext) -> String {
        let mut parts: Vec<String> = ctx
            .variables
            .iter()
            .map(|(k, v)| format!("{}={:?}", k, v))
            .collect();
        parts.sort();
        parts.join("|")
    }

    /// RETURN 절에 집계 함수가 있는지 확인
    pub(super) fn has_aggregate_in_return(&self, clause: &ReturnClause) -> bool {
        for item in &clause.items {
            if let ReturnItem::Expression { expr, .. } = item
                && self.contains_aggregate(expr)
            {
                return true;
            }
        }
        false
    }

    /// WITH 절에 집계 함수가 있는지 확인
    pub(super) fn has_aggregate_in_with(&self, clause: &WithClause) -> bool {
        for item in &clause.items {
            if let ReturnItem::Expression { expr, .. } = item
                && self.contains_aggregate(expr)
            {
                return true;
            }
        }
        false
    }

    /// 값이 참인지 확인 (truthy)
    pub(super) fn is_truthy(&self, value: &Value) -> bool {
        match value {
            Value::Boolean(b) => *b,
            Value::Null => false,
            Value::Integer(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(l) => !l.is_empty(),
            _ => true,
        }
    }

    /// 두 값이 같은지 비교
    pub(super) fn values_equal(&self, left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Null, Value::Null) => true,
            (Value::Null, _) | (_, Value::Null) => false,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => (a - b).abs() < f64::EPSILON,
            (Value::Integer(a), Value::Float(b)) | (Value::Float(b), Value::Integer(a)) => {
                ((*a as f64) - b).abs() < f64::EPSILON
            }
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| self.values_equal(x, y))
            }
            (Value::Node(a), Value::Node(b)) => a == b,
            (Value::Edge(a), Value::Edge(b)) => a == b,
            _ => false,
        }
    }

    /// 두 값을 비교 (순서 비교) - 부분 비교, None은 비교 불가
    pub(super) fn compare_values(&self, left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::Integer(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
            (Value::Float(a), Value::Integer(b)) => a.partial_cmp(&(*b as f64)),
            (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    /// 두 값을 비교 (전체 순서 비교) - ORDER BY용, 항상 Ordering 반환
    /// Null은 마지막, 타입이 다르면 타입 순서로 비교
    pub(super) fn compare_values_total(&self, left: &Value, right: &Value) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // Null 처리: Null은 항상 마지막
        match (left, right) {
            (Value::Null, Value::Null) => return Ordering::Equal,
            (Value::Null, _) => return Ordering::Greater, // Null은 뒤로
            (_, Value::Null) => return Ordering::Less,
            _ => {}
        }

        // 같은 타입끼리 비교
        match (left, right) {
            (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),
            (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
            (Value::Float(a), Value::Float(b)) => {
                // NaN 처리: NaN은 마지막으로
                match (a.is_nan(), b.is_nan()) {
                    (true, true) => Ordering::Equal,
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                    (false, false) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
                }
            }
            (Value::Integer(a), Value::Float(b)) => {
                let af = *a as f64;
                if b.is_nan() {
                    Ordering::Less
                } else {
                    af.partial_cmp(b).unwrap_or(Ordering::Equal)
                }
            }
            (Value::Float(a), Value::Integer(b)) => {
                let bf = *b as f64;
                if a.is_nan() {
                    Ordering::Greater
                } else {
                    a.partial_cmp(&bf).unwrap_or(Ordering::Equal)
                }
            }
            (Value::String(a), Value::String(b)) => a.cmp(b),
            (Value::Array(a), Value::Array(b)) => {
                // 배열은 요소별 비교
                for (x, y) in a.iter().zip(b.iter()) {
                    let ord = self.compare_values_total(x, y);
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
                a.len().cmp(&b.len())
            }
            // 다른 타입끼리는 타입 순서로 비교
            _ => self.type_order(left).cmp(&self.type_order(right)),
        }
    }

    /// 값 타입의 순서 (타입이 다를 때 정렬 순서 결정)
    fn type_order(&self, value: &Value) -> u8 {
        match value {
            Value::Null => 255, // 마지막
            Value::Boolean(_) => 0,
            Value::Integer(_) => 1,
            Value::Float(_) => 2,
            Value::String(_) => 3,
            Value::Array(_) => 4,
            Value::Map(_) => 5,
            Value::DateTime(_) => 6,
            Value::Date(_) => 7,
            Value::Time(_) => 8,
            Value::LocalDateTime(_) => 9,
            Value::Duration(_) => 10,
            Value::Node(_) => 11,
            Value::Edge(_) => 12,
            Value::Path(_) => 13,
            Value::Vector(_) => 14,
        }
    }

    /// 문자열 포함 여부 (CONTAINS 연산자)
    pub(super) fn string_contains(&self, left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::String(a), Value::String(b)) => a.contains(b.as_str()),
            _ => false,
        }
    }

    /// 문자열 시작 여부 (STARTS WITH 연산자)
    pub(super) fn string_starts_with(&self, left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::String(a), Value::String(b)) => a.starts_with(b.as_str()),
            _ => false,
        }
    }

    /// 문자열 끝 여부 (ENDS WITH 연산자)
    pub(super) fn string_ends_with(&self, left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::String(a), Value::String(b)) => a.ends_with(b.as_str()),
            _ => false,
        }
    }

    /// 리스트에 값이 포함되어 있는지 (IN 연산자)
    pub(super) fn value_in_list(&self, value: &Value, list: &Value) -> bool {
        match list {
            Value::Array(items) => items.iter().any(|item| self.values_equal(value, item)),
            _ => false,
        }
    }
}
