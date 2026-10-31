// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 함수 인덱스 구현
//!
//! 단일 함수 표현식에 대한 인덱스를 관리합니다.

use super::{FunctionEvaluator, FunctionExpression};
use crate::core::{NodeId, Value};
use crate::index::IndexValue;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};

/// 함수 인덱스
///
/// 특정 함수 표현식으로 계산된 값에 대한 인덱스를 유지합니다.
#[derive(Debug)]
pub struct FunctionIndex {
    /// 인덱스 이름
    name: String,
    /// 대상 노드 라벨
    label: String,
    /// 함수 표현식
    expression: FunctionExpression,
    /// 함수 평가기
    evaluator: FunctionEvaluator,
    /// 계산된 값 -> 노드 ID 집합 매핑
    computed_to_nodes: RwLock<HashMap<IndexValue, HashSet<NodeId>>>,
    /// 노드 ID -> 계산된 값 (업데이트 시 이전 값 제거용)
    node_to_computed: RwLock<HashMap<NodeId, IndexValue>>,
}

impl FunctionIndex {
    /// 새 함수 인덱스 생성
    ///
    /// # Arguments
    /// * `name` - 인덱스 이름
    /// * `label` - 대상 노드 라벨
    /// * `expression` - 함수 표현식
    pub fn new(name: &str, label: &str, expression: FunctionExpression) -> Self {
        Self {
            name: name.to_string(),
            label: label.to_string(),
            expression,
            evaluator: FunctionEvaluator::new(),
            computed_to_nodes: RwLock::new(HashMap::new()),
            node_to_computed: RwLock::new(HashMap::new()),
        }
    }

    /// 인덱스 이름 반환
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 대상 라벨 반환
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 함수 표현식 반환
    pub fn expression(&self) -> &FunctionExpression {
        &self.expression
    }

    /// 원본 프로퍼티 이름 반환
    pub fn source_property(&self) -> &str {
        self.expression.source_property()
    }

    /// 노드를 인덱스에 추가
    ///
    /// # Arguments
    /// * `node_id` - 노드 ID
    /// * `property_value` - 원본 프로퍼티 값
    ///
    /// # Returns
    /// 인덱싱 성공 여부 (타입 불일치 시 false)
    pub fn insert(&self, node_id: NodeId, property_value: &Value) -> bool {
        // 함수 평가
        let computed_value = match self.evaluator.evaluate(&self.expression, property_value) {
            Some(v) => v,
            None => return false, // 타입 불일치
        };

        let index_value = IndexValue::from(&computed_value);

        // 기존 값이 있으면 제거
        self.remove(node_id);

        // 새 값 추가
        {
            let mut computed_to_nodes = self.computed_to_nodes.write();
            computed_to_nodes
                .entry(index_value.clone())
                .or_default()
                .insert(node_id);
        }

        {
            let mut node_to_computed = self.node_to_computed.write();
            node_to_computed.insert(node_id, index_value);
        }

        true
    }

    /// 노드를 인덱스에서 제거
    ///
    /// # Arguments
    /// * `node_id` - 노드 ID
    ///
    /// # Returns
    /// 제거 성공 여부 (노드가 인덱스에 없었으면 false)
    pub fn remove(&self, node_id: NodeId) -> bool {
        let mut node_to_computed = self.node_to_computed.write();

        if let Some(old_value) = node_to_computed.remove(&node_id) {
            let mut computed_to_nodes = self.computed_to_nodes.write();
            if let Some(nodes) = computed_to_nodes.get_mut(&old_value) {
                nodes.remove(&node_id);
                if nodes.is_empty() {
                    computed_to_nodes.remove(&old_value);
                }
            }
            true
        } else {
            false
        }
    }

    /// 계산된 값으로 노드 검색
    ///
    /// # Arguments
    /// * `computed_value` - 검색할 계산된 값
    ///
    /// # Returns
    /// 일치하는 노드 ID 목록
    pub fn find(&self, computed_value: &Value) -> Vec<NodeId> {
        let index_value = IndexValue::from(computed_value);
        let computed_to_nodes = self.computed_to_nodes.read();

        computed_to_nodes
            .get(&index_value)
            .map(|nodes| nodes.iter().copied().collect())
            .unwrap_or_default()
    }

    /// 인덱스에 저장된 노드 수
    pub fn len(&self) -> usize {
        self.node_to_computed.read().len()
    }

    /// 인덱스가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.node_to_computed.read().is_empty()
    }

    /// 인덱스 초기화
    pub fn clear(&self) {
        self.computed_to_nodes.write().clear();
        self.node_to_computed.write().clear();
    }

    /// 고유 값 개수
    pub fn distinct_values(&self) -> usize {
        self.computed_to_nodes.read().len()
    }

    /// 노드가 인덱스에 있는지 확인
    pub fn contains(&self, node_id: NodeId) -> bool {
        self.node_to_computed.read().contains_key(&node_id)
    }

    /// 노드의 현재 계산된 값 조회
    pub fn get_computed_value(&self, node_id: NodeId) -> Option<IndexValue> {
        self.node_to_computed.read().get(&node_id).cloned()
    }
}
