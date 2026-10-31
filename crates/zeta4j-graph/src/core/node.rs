// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 그래프 노드 구조

use crate::core::serialization::PostcardSerializable;
use crate::core::{MemorySized, Property, PropertyContainer};
use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

/// 노드 ID 타입 (64비트 정수로 최대 성능 보장)
pub type NodeId = u64;

/// 그래프 노드
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// 노드 고유 ID
    id: NodeId,

    /// 노드 레이블 집합 (삽입 순서 유지)
    labels: IndexSet<String>,

    /// 노드 속성
    properties: Property,
}

impl Node {
    /// 새로운 노드 생성 (ID는 외부에서 할당)
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            labels: IndexSet::new(),
            properties: Property::new(),
        }
    }

    /// 노드 ID 조회
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// 레이블 추가
    pub fn add_label<S: Into<String>>(&mut self, label: S) {
        self.labels.insert(label.into());
    }

    /// 레이블 제거 (순서 유지)
    pub fn remove_label(&mut self, label: &str) -> bool {
        self.labels.shift_remove(label)
    }

    /// 레이블이 있는지 확인
    pub fn has_label(&self, label: &str) -> bool {
        self.labels.contains(label)
    }

    /// 모든 레이블 조회
    pub fn labels(&self) -> &IndexSet<String> {
        &self.labels
    }

    /// 레이블 개수
    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    // Property 메서드들은 PropertyContainer 트레이트로 이동됨
    // set_property, get_property, remove_property, has_property,
    // properties, properties_mut, property_count

    /// 노드의 메모리 사용량 추정 (바이트)
    pub fn memory_size(&self) -> usize {
        let labels = self
            .labels
            .iter()
            .map(|label| std::mem::size_of::<String>().saturating_add(label.len()))
            .fold(0usize, usize::saturating_add);
        std::mem::size_of::<Self>()
            .saturating_add(labels)
            .saturating_add(self.properties.memory_size())
    }

    /// 노드가 특정 조건을 만족하는지 확인
    pub fn matches_label(&self, label: &str) -> bool {
        self.has_label(label)
    }

    /// 노드가 여러 레이블 중 하나라도 가지는지 확인
    pub fn matches_any_label(&self, labels: &[&str]) -> bool {
        labels.iter().any(|&label| self.has_label(label))
    }

    /// 노드가 모든 레이블을 가지는지 확인
    pub fn matches_all_labels(&self, labels: &[&str]) -> bool {
        labels.iter().all(|&label| self.has_label(label))
    }
}

/// Node에 대한 Postcard 직렬화 구현
impl PostcardSerializable for Node {}

impl MemorySized for Node {
    fn memory_size(&self) -> usize {
        Self::memory_size(self)
    }
}

/// Node에 대한 PropertyContainer 구현
impl PropertyContainer for Node {
    fn properties(&self) -> &Property {
        &self.properties
    }

    fn properties_mut(&mut self) -> &mut Property {
        &mut self.properties
    }
}
