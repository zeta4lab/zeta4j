// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 관계 타입 스키마
//!
//! 관계 타입에 대한 프로퍼티 스키마 정의
//!
//! # v1.58.0 업데이트
//!
//! - 스키마 버전 관리 필드 추가
//! - 스키마 간 호환성 체크 API 추가
//!
//! # v1.59.0 업데이트
//!
//! - 인덱스 힌트 통합: index_hints 필드 추가
//! - with_index() 메서드로 인덱스 힌트 추가
//! - get_index_hints() 메서드로 인덱스 힌트 조회
//!
//! # v1.60.0 업데이트
//!
//! - ElementSchema 트레이트 구현으로 공통 메서드 통합

use indexmap::IndexMap;

use super::common::ElementSchema;
use super::evolution::{CompatibilityLevel, SchemaVersion};
use super::index_hint::IndexHints;
use super::node_schema::{PropertyChange, PropertyChangeType, SchemaDiff};
use super::types::PropertyDefinition;
use crate::core::Value;
use crate::impl_element_schema_builder;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// 관계 타입 스키마
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipTypeSchema {
    /// 관계 타입 이름
    pub relationship_type: String,
    /// 프로퍼티 정의 (이름 -> 정의)
    pub properties: HashMap<String, PropertyDefinition>,
    /// 필수 프로퍼티 이름들
    pub required_properties: HashSet<String>,
    /// 허용되는 소스 노드 라벨 (None = 모든 라벨 허용)
    pub source_labels: Option<HashSet<String>>,
    /// 허용되는 타겟 노드 라벨 (None = 모든 라벨 허용)
    pub target_labels: Option<HashSet<String>>,
    /// 스키마 버전
    #[serde(default)]
    pub version: SchemaVersion,
    /// 인덱스 힌트
    #[serde(default)]
    pub index_hints: IndexHints,
}

impl RelationshipTypeSchema {
    /// 새 관계 타입 스키마 생성
    pub fn new(relationship_type: impl Into<String>) -> Self {
        Self {
            relationship_type: relationship_type.into(),
            properties: HashMap::new(),
            required_properties: HashSet::new(),
            source_labels: None,
            target_labels: None,
            version: SchemaVersion::initial(),
            index_hints: IndexHints::new(),
        }
    }

    // 빌더 패턴 메서드들 (with_version, with_version_str, with_index, with_property)
    impl_element_schema_builder!();

    /// 빌더 패턴: 소스 라벨 제한 설정
    pub fn from_labels<I, S>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.source_labels = Some(labels.into_iter().map(|s| s.into()).collect());
        self
    }

    /// 빌더 패턴: 타겟 라벨 제한 설정
    pub fn to_labels<I, S>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.target_labels = Some(labels.into_iter().map(|s| s.into()).collect());
        self
    }

    // get_property(), has_property(), property_names()는 ElementSchema 트레이트에서 제공됨

    /// 소스 라벨이 허용되는지 확인
    pub fn allows_source_label(&self, label: &str) -> bool {
        match &self.source_labels {
            None => true,
            Some(labels) => labels.contains(label),
        }
    }

    /// 타겟 라벨이 허용되는지 확인
    pub fn allows_target_label(&self, label: &str) -> bool {
        match &self.target_labels {
            None => true,
            Some(labels) => labels.contains(label),
        }
    }

    /// 관계 프로퍼티 검증
    ///
    /// # Arguments
    /// * `properties` - 검증할 프로퍼티 맵
    /// * `source_label` - 소스 노드 라벨 (옵션)
    /// * `target_label` - 타겟 노드 라벨 (옵션)
    /// * `strict` - true면 스키마에 정의되지 않은 프로퍼티 거부
    ///
    /// # Returns
    /// * `Ok(())` - 검증 성공
    /// * `Err(Vec<String>)` - 검증 실패 메시지 목록
    pub fn validate(
        &self,
        properties: &IndexMap<String, Value>,
        source_label: Option<&str>,
        target_label: Option<&str>,
        strict: bool,
    ) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // 소스 라벨 검증
        if let Some(label) = source_label
            && !self.allows_source_label(label)
        {
            errors.push(format!(
                "Source label '{}' not allowed for relationship type '{}'. Allowed: {:?}",
                label, self.relationship_type, self.source_labels
            ));
        }

        // 타겟 라벨 검증
        if let Some(label) = target_label
            && !self.allows_target_label(label)
        {
            errors.push(format!(
                "Target label '{}' not allowed for relationship type '{}'. Allowed: {:?}",
                label, self.relationship_type, self.target_labels
            ));
        }

        // 필수 프로퍼티 체크
        for required in &self.required_properties {
            if !properties.contains_key(required) {
                errors.push(format!(
                    "Missing required property '{}' for relationship type '{}'",
                    required, self.relationship_type
                ));
            }
        }

        // 프로퍼티 타입 검증
        for (name, value) in properties {
            if let Some(def) = self.properties.get(name) {
                if let Err(e) = def.validate(Some(value)) {
                    errors.push(format!(
                        "Relationship type '{}': {}",
                        self.relationship_type, e
                    ));
                }
            } else if strict {
                errors.push(format!(
                    "Unknown property '{}' for relationship type '{}'",
                    name, self.relationship_type
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    // apply_defaults()는 ElementSchema 트레이트에서 제공됨

    // ========== 호환성 체크 API ==========

    /// 다른 스키마와의 호환성 수준 계산
    ///
    /// # Arguments
    /// * `other` - 비교할 스키마 (newer 버전)
    ///
    /// # Returns
    /// * `CompatibilityLevel` - 호환성 수준
    pub fn check_compatibility(&self, other: &RelationshipTypeSchema) -> CompatibilityLevel {
        let mut compatibility = CompatibilityLevel::Full;

        // 1. 제거된 프로퍼티 확인
        for name in self.properties.keys() {
            if !other.properties.contains_key(name) {
                compatibility = compatibility.merge(&CompatibilityLevel::Forward);
            }
        }

        // 2. 추가된 프로퍼티 확인
        for (name, new_prop) in &other.properties {
            if !self.properties.contains_key(name)
                && !new_prop.nullable
                && new_prop.default_value.is_none()
            {
                compatibility = compatibility.merge(&CompatibilityLevel::Backward);
            }
        }

        // 3. 프로퍼티 타입 변경 확인
        for (name, old_prop) in &self.properties {
            if let Some(new_prop) = other.properties.get(name) {
                if old_prop.property_type != new_prop.property_type {
                    compatibility = compatibility.merge(&CompatibilityLevel::Breaking);
                }

                if old_prop.nullable && !new_prop.nullable && new_prop.default_value.is_none() {
                    compatibility = compatibility.merge(&CompatibilityLevel::Breaking);
                }
            }
        }

        // 4. 라벨 제한 변경 확인
        if self.source_labels != other.source_labels || self.target_labels != other.target_labels {
            // 라벨 제한 변경은 Breaking으로 간주
            compatibility = compatibility.merge(&CompatibilityLevel::Breaking);
        }

        compatibility
    }

    /// 두 스키마가 호환되는지 확인
    pub fn is_compatible_with(&self, other: &RelationshipTypeSchema) -> bool {
        self.check_compatibility(other).is_compatible()
    }

    /// 스키마 차이점 분석
    pub fn diff(&self, other: &RelationshipTypeSchema) -> SchemaDiff {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();

        // 제거된 프로퍼티
        for (name, prop) in &self.properties {
            if !other.properties.contains_key(name) {
                removed.push(PropertyChange {
                    name: name.clone(),
                    change_type: PropertyChangeType::Removed,
                    old_type: Some(prop.property_type.clone()),
                    new_type: None,
                });
            }
        }

        // 추가된 프로퍼티
        for (name, prop) in &other.properties {
            if !self.properties.contains_key(name) {
                added.push(PropertyChange {
                    name: name.clone(),
                    change_type: PropertyChangeType::Added,
                    old_type: None,
                    new_type: Some(prop.property_type.clone()),
                });
            }
        }

        // 변경된 프로퍼티
        for (name, old_prop) in &self.properties {
            if let Some(new_prop) = other.properties.get(name) {
                if old_prop.property_type != new_prop.property_type {
                    modified.push(PropertyChange {
                        name: name.clone(),
                        change_type: PropertyChangeType::TypeChanged,
                        old_type: Some(old_prop.property_type.clone()),
                        new_type: Some(new_prop.property_type.clone()),
                    });
                } else if old_prop.nullable != new_prop.nullable {
                    modified.push(PropertyChange {
                        name: name.clone(),
                        change_type: if new_prop.nullable {
                            PropertyChangeType::MadeOptional
                        } else {
                            PropertyChangeType::MadeRequired
                        },
                        old_type: Some(old_prop.property_type.clone()),
                        new_type: Some(new_prop.property_type.clone()),
                    });
                }
            }
        }

        SchemaDiff {
            from_version: self.version.clone(),
            to_version: other.version.clone(),
            added,
            removed,
            modified,
            compatibility: self.check_compatibility(other),
        }
    }
}

/// ElementSchema 트레이트 구현
impl ElementSchema for RelationshipTypeSchema {
    fn name(&self) -> &str {
        &self.relationship_type
    }

    fn properties(&self) -> &HashMap<String, PropertyDefinition> {
        &self.properties
    }

    fn properties_mut(&mut self) -> &mut HashMap<String, PropertyDefinition> {
        &mut self.properties
    }

    fn required_properties(&self) -> &HashSet<String> {
        &self.required_properties
    }

    fn required_properties_mut(&mut self) -> &mut HashSet<String> {
        &mut self.required_properties
    }

    fn version(&self) -> &SchemaVersion {
        &self.version
    }

    fn version_mut(&mut self) -> &mut SchemaVersion {
        &mut self.version
    }

    fn index_hints(&self) -> &IndexHints {
        &self.index_hints
    }

    fn index_hints_mut(&mut self) -> &mut IndexHints {
        &mut self.index_hints
    }
}
