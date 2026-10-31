// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 추론 엔진 (Inference Engine)
//!
//! v0.52.0: RDF/OWL 스타일의 추론 기능 지원
//!
//! # 지원하는 추론 규칙
//!
//! - 라벨 계층 (Label Hierarchy): subLabelOf 관계
//! - 관계 타입 계층 (Relationship Hierarchy): subRelationshipOf 관계
//! - 트랜지티브 관계 (Transitive): 경로 추론
//! - 대칭 관계 (Symmetric): 양방향 추론
//! - 역관계 (Inverse): inverseOf 관계
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::schema::InferenceEngine;
//!
//! let mut engine = InferenceEngine::new();
//!
//! // 라벨 계층 정의
//! engine.add_label_hierarchy("Employee", "Person");
//! engine.add_label_hierarchy("Manager", "Employee");
//!
//! // Manager는 Employee와 Person을 포함
//! assert!(engine.has_inferred_label(&["Manager".into()], "Person"));
//!
//! // 관계 특성 정의
//! engine.set_transitive("ANCESTOR_OF");
//! engine.set_symmetric("KNOWS");
//! engine.set_inverse("PARENT_OF", "CHILD_OF");
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// 라벨 계층 구조
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelHierarchy {
    /// 부모 라벨 매핑 (child -> direct parents)
    parents: HashMap<String, HashSet<String>>,
    /// 자식 라벨 매핑 (parent -> direct children)
    children: HashMap<String, HashSet<String>>,
    /// 모든 조상 캐시 (child -> all ancestors)
    ancestors_cache: HashMap<String, HashSet<String>>,
    /// 모든 자손 캐시 (parent -> all descendants)
    descendants_cache: HashMap<String, HashSet<String>>,
}

impl LabelHierarchy {
    /// 새로운 라벨 계층 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 라벨 계층 추가
    ///
    /// child가 parent의 하위 라벨임을 정의합니다.
    /// 예: add("Employee", "Person") => Employee는 Person의 하위 라벨
    pub fn add(&mut self, child: &str, parent: &str) {
        // 순환 참조 체크
        if self.has_ancestor(parent, child) {
            return; // 순환 참조 무시
        }

        self.parents
            .entry(child.to_string())
            .or_default()
            .insert(parent.to_string());

        self.children
            .entry(parent.to_string())
            .or_default()
            .insert(child.to_string());

        // 캐시 무효화
        self.invalidate_cache();
    }

    /// 라벨 계층 제거
    pub fn remove(&mut self, child: &str, parent: &str) {
        if let Some(parents) = self.parents.get_mut(child) {
            parents.remove(parent);
            if parents.is_empty() {
                self.parents.remove(child);
            }
        }

        if let Some(children) = self.children.get_mut(parent) {
            children.remove(child);
            if children.is_empty() {
                self.children.remove(parent);
            }
        }

        self.invalidate_cache();
    }

    /// 캐시 무효화
    fn invalidate_cache(&mut self) {
        self.ancestors_cache.clear();
        self.descendants_cache.clear();
    }

    /// 직접 부모 라벨 조회
    pub fn get_direct_parents(&self, label: &str) -> HashSet<String> {
        self.parents.get(label).cloned().unwrap_or_default()
    }

    /// 직접 자식 라벨 조회
    pub fn get_direct_children(&self, label: &str) -> HashSet<String> {
        self.children.get(label).cloned().unwrap_or_default()
    }

    /// 모든 조상 라벨 조회 (재귀적)
    pub fn get_all_ancestors(&mut self, label: &str) -> HashSet<String> {
        // 캐시 확인
        if let Some(cached) = self.ancestors_cache.get(label) {
            return cached.clone();
        }

        let mut ancestors = HashSet::new();
        let mut queue = VecDeque::new();

        // 직접 부모들 추가
        if let Some(parents) = self.parents.get(label) {
            for parent in parents {
                queue.push_back(parent.clone());
            }
        }

        // BFS로 모든 조상 탐색
        while let Some(current) = queue.pop_front() {
            if ancestors.insert(current.clone())
                && let Some(parents) = self.parents.get(&current)
            {
                for parent in parents {
                    queue.push_back(parent.clone());
                }
            }
        }

        // 캐시 저장
        self.ancestors_cache
            .insert(label.to_string(), ancestors.clone());

        ancestors
    }

    /// 모든 자손 라벨 조회 (재귀적)
    pub fn get_all_descendants(&mut self, label: &str) -> HashSet<String> {
        // 캐시 확인
        if let Some(cached) = self.descendants_cache.get(label) {
            return cached.clone();
        }

        let mut descendants = HashSet::new();
        let mut queue = VecDeque::new();

        // 직접 자식들 추가
        if let Some(children) = self.children.get(label) {
            for child in children {
                queue.push_back(child.clone());
            }
        }

        // BFS로 모든 자손 탐색
        while let Some(current) = queue.pop_front() {
            if descendants.insert(current.clone())
                && let Some(children) = self.children.get(&current)
            {
                for child in children {
                    queue.push_back(child.clone());
                }
            }
        }

        // 캐시 저장
        self.descendants_cache
            .insert(label.to_string(), descendants.clone());

        descendants
    }

    /// 특정 라벨이 다른 라벨의 조상인지 확인
    pub fn has_ancestor(&mut self, label: &str, potential_ancestor: &str) -> bool {
        if label == potential_ancestor {
            return true;
        }
        self.get_all_ancestors(label).contains(potential_ancestor)
    }

    /// 특정 라벨이 다른 라벨의 자손인지 확인
    pub fn has_descendant(&mut self, label: &str, potential_descendant: &str) -> bool {
        if label == potential_descendant {
            return true;
        }
        self.get_all_descendants(label)
            .contains(potential_descendant)
    }

    /// 라벨과 그 모든 조상 반환 (자신 포함)
    pub fn get_label_with_ancestors(&mut self, label: &str) -> HashSet<String> {
        let mut result = self.get_all_ancestors(label);
        result.insert(label.to_string());
        result
    }

    /// 라벨과 그 모든 자손 반환 (자신 포함)
    pub fn get_label_with_descendants(&mut self, label: &str) -> HashSet<String> {
        let mut result = self.get_all_descendants(label);
        result.insert(label.to_string());
        result
    }
}

/// 관계 타입 계층 구조
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipHierarchy {
    /// 부모 관계 타입 매핑
    parents: HashMap<String, HashSet<String>>,
    /// 자식 관계 타입 매핑
    children: HashMap<String, HashSet<String>>,
}

impl RelationshipHierarchy {
    /// 새로운 관계 타입 계층 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 관계 타입 계층 추가
    pub fn add(&mut self, child: &str, parent: &str) {
        self.parents
            .entry(child.to_string())
            .or_default()
            .insert(parent.to_string());

        self.children
            .entry(parent.to_string())
            .or_default()
            .insert(child.to_string());
    }

    /// 모든 상위 관계 타입 조회
    pub fn get_all_super_types(&self, rel_type: &str) -> HashSet<String> {
        let mut result = HashSet::new();
        let mut queue = VecDeque::new();

        if let Some(parents) = self.parents.get(rel_type) {
            for parent in parents {
                queue.push_back(parent.clone());
            }
        }

        while let Some(current) = queue.pop_front() {
            if result.insert(current.clone())
                && let Some(parents) = self.parents.get(&current)
            {
                for parent in parents {
                    queue.push_back(parent.clone());
                }
            }
        }

        result
    }

    /// 모든 하위 관계 타입 조회
    pub fn get_all_sub_types(&self, rel_type: &str) -> HashSet<String> {
        let mut result = HashSet::new();
        let mut queue = VecDeque::new();

        if let Some(children) = self.children.get(rel_type) {
            for child in children {
                queue.push_back(child.clone());
            }
        }

        while let Some(current) = queue.pop_front() {
            if result.insert(current.clone())
                && let Some(children) = self.children.get(&current)
            {
                for child in children {
                    queue.push_back(child.clone());
                }
            }
        }

        result
    }
}

/// 관계 특성
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipCharacteristics {
    /// 트랜지티브 여부
    pub is_transitive: bool,
    /// 대칭 여부
    pub is_symmetric: bool,
    /// 역관계 타입
    pub inverse_of: Option<String>,
}

impl RelationshipCharacteristics {
    /// 새로운 관계 특성 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 트랜지티브 설정
    pub fn transitive(mut self) -> Self {
        self.is_transitive = true;
        self
    }

    /// 대칭 설정
    pub fn symmetric(mut self) -> Self {
        self.is_symmetric = true;
        self
    }

    /// 역관계 설정
    pub fn inverse(mut self, inverse_type: impl Into<String>) -> Self {
        self.inverse_of = Some(inverse_type.into());
        self
    }
}

/// 추론 엔진
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InferenceEngine {
    /// 라벨 계층 구조
    pub label_hierarchy: LabelHierarchy,
    /// 관계 타입 계층 구조
    pub relationship_hierarchy: RelationshipHierarchy,
    /// 관계 특성 매핑
    pub relationship_characteristics: HashMap<String, RelationshipCharacteristics>,
}

impl InferenceEngine {
    /// 새로운 추론 엔진 생성
    pub fn new() -> Self {
        Self::default()
    }

    // ===== 라벨 계층 관련 =====

    /// 라벨 계층 추가
    ///
    /// # Arguments
    /// * `child` - 하위 라벨
    /// * `parent` - 상위 라벨
    ///
    /// # Example
    /// ```rust,ignore
    /// engine.add_label_hierarchy("Employee", "Person");
    /// // Employee는 Person의 하위 라벨
    /// ```
    pub fn add_label_hierarchy(&mut self, child: &str, parent: &str) {
        self.label_hierarchy.add(child, parent);
    }

    /// 라벨 계층 제거
    pub fn remove_label_hierarchy(&mut self, child: &str, parent: &str) {
        self.label_hierarchy.remove(child, parent);
    }

    /// 모든 상위 라벨 조회 (재귀적)
    pub fn get_all_super_labels(&mut self, label: &str) -> HashSet<String> {
        self.label_hierarchy.get_all_ancestors(label)
    }

    /// 모든 하위 라벨 조회 (재귀적)
    pub fn get_all_sub_labels(&mut self, label: &str) -> HashSet<String> {
        self.label_hierarchy.get_all_descendants(label)
    }

    /// 노드가 특정 라벨을 (추론을 통해) 가지는지 확인
    ///
    /// 노드의 실제 라벨이 target의 하위 라벨이면 true 반환
    pub fn has_inferred_label(&mut self, node_labels: &[String], target: &str) -> bool {
        for label in node_labels {
            if label == target {
                return true;
            }
            if self.label_hierarchy.has_ancestor(label, target) {
                return true;
            }
        }
        false
    }

    /// 특정 라벨로 검색 시 포함해야 할 모든 라벨 반환
    ///
    /// 해당 라벨의 모든 하위 라벨을 포함
    pub fn get_searchable_labels(&mut self, label: &str) -> HashSet<String> {
        self.label_hierarchy.get_label_with_descendants(label)
    }

    // ===== 관계 타입 계층 관련 =====

    /// 관계 타입 계층 추가
    pub fn add_relationship_hierarchy(&mut self, child: &str, parent: &str) {
        self.relationship_hierarchy.add(child, parent);
    }

    /// 모든 상위 관계 타입 조회
    pub fn get_all_super_relationship_types(&self, rel_type: &str) -> HashSet<String> {
        self.relationship_hierarchy.get_all_super_types(rel_type)
    }

    /// 모든 하위 관계 타입 조회
    pub fn get_all_sub_relationship_types(&self, rel_type: &str) -> HashSet<String> {
        self.relationship_hierarchy.get_all_sub_types(rel_type)
    }

    // ===== 관계 특성 관련 =====

    /// 관계 특성 설정
    pub fn set_relationship_characteristics(
        &mut self,
        rel_type: &str,
        chars: RelationshipCharacteristics,
    ) {
        self.relationship_characteristics
            .insert(rel_type.to_string(), chars);
    }

    /// 트랜지티브 관계로 설정
    pub fn set_transitive(&mut self, rel_type: &str) {
        self.relationship_characteristics
            .entry(rel_type.to_string())
            .or_default()
            .is_transitive = true;
    }

    /// 대칭 관계로 설정
    pub fn set_symmetric(&mut self, rel_type: &str) {
        self.relationship_characteristics
            .entry(rel_type.to_string())
            .or_default()
            .is_symmetric = true;
    }

    /// 역관계 설정
    pub fn set_inverse(&mut self, rel_type: &str, inverse_type: &str) {
        self.relationship_characteristics
            .entry(rel_type.to_string())
            .or_default()
            .inverse_of = Some(inverse_type.to_string());

        // 역관계도 설정 (양방향)
        self.relationship_characteristics
            .entry(inverse_type.to_string())
            .or_default()
            .inverse_of = Some(rel_type.to_string());
    }

    /// 트랜지티브 여부 확인
    pub fn is_transitive(&self, rel_type: &str) -> bool {
        self.relationship_characteristics
            .get(rel_type)
            .map(|c| c.is_transitive)
            .unwrap_or(false)
    }

    /// 대칭 여부 확인
    pub fn is_symmetric(&self, rel_type: &str) -> bool {
        self.relationship_characteristics
            .get(rel_type)
            .map(|c| c.is_symmetric)
            .unwrap_or(false)
    }

    /// 역관계 조회
    pub fn get_inverse(&self, rel_type: &str) -> Option<&str> {
        self.relationship_characteristics
            .get(rel_type)
            .and_then(|c| c.inverse_of.as_deref())
    }

    /// 관계 특성 조회
    pub fn get_characteristics(&self, rel_type: &str) -> Option<&RelationshipCharacteristics> {
        self.relationship_characteristics.get(rel_type)
    }

    // ===== 추론 쿼리 =====

    /// 관계 타입으로 검색 시 포함해야 할 모든 관계 타입
    pub fn get_searchable_relationship_types(&self, rel_type: &str) -> HashSet<String> {
        let mut result = self.relationship_hierarchy.get_all_sub_types(rel_type);
        result.insert(rel_type.to_string());
        result
    }

    /// 대칭 관계 추론: 노드 A -> B가 있으면 B -> A도 존재하는 것으로 간주
    pub fn should_check_reverse(&self, rel_type: &str) -> bool {
        self.is_symmetric(rel_type)
    }

    /// 역관계로 변환: PARENT_OF -> CHILD_OF
    pub fn get_reverse_relationship(&self, rel_type: &str) -> Option<&str> {
        self.get_inverse(rel_type)
    }
}
