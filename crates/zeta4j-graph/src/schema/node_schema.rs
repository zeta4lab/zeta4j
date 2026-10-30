//! 노드 라벨 스키마
//!
//! 노드 라벨에 대한 프로퍼티 스키마 정의
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
use super::types::PropertyDefinition;
use crate::core::Value;
use crate::impl_element_schema_builder;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// 노드 라벨 스키마
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeLabelSchema {
    /// 라벨 이름
    pub label: String,
    /// 프로퍼티 정의 (이름 -> 정의)
    pub properties: HashMap<String, PropertyDefinition>,
    /// 필수 프로퍼티 이름들
    pub required_properties: HashSet<String>,
    /// 스키마 버전
    #[serde(default)]
    pub version: SchemaVersion,
    /// 인덱스 힌트
    #[serde(default)]
    pub index_hints: IndexHints,
}

impl NodeLabelSchema {
    /// 새 노드 라벨 스키마 생성
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            properties: HashMap::new(),
            required_properties: HashSet::new(),
            version: SchemaVersion::initial(),
            index_hints: IndexHints::new(),
        }
    }

    // 빌더 패턴 메서드들 (with_version, with_version_str, with_index, with_property)
    impl_element_schema_builder!();

    /// 노드 프로퍼티 검증
    ///
    /// # Arguments
    /// * `properties` - 검증할 프로퍼티 맵
    /// * `strict` - true면 스키마에 정의되지 않은 프로퍼티 거부
    ///
    /// # Returns
    /// * `Ok(())` - 검증 성공
    /// * `Err(Vec<String>)` - 검증 실패 메시지 목록
    pub fn validate(
        &self,
        properties: &IndexMap<String, Value>,
        strict: bool,
    ) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // 필수 프로퍼티 체크
        for required in &self.required_properties {
            if !properties.contains_key(required) {
                errors.push(format!(
                    "Missing required property '{}' for label '{}'",
                    required, self.label
                ));
            }
        }

        // 프로퍼티 타입 검증
        for (name, value) in properties {
            if let Some(def) = self.properties.get(name) {
                if let Err(e) = def.validate(Some(value)) {
                    errors.push(format!("Label '{}': {}", self.label, e));
                }
            } else if strict {
                errors.push(format!(
                    "Unknown property '{}' for label '{}'",
                    name, self.label
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
    ///
    /// # 호환성 규칙
    /// - Full: 두 스키마가 동일하거나 선택적 프로퍼티만 추가됨
    /// - Forward: 새 스키마가 기존 데이터를 읽을 수 있음 (프로퍼티 제거)
    /// - Backward: 기존 스키마가 새 데이터를 읽을 수 있음 (필수 프로퍼티 추가)
    /// - Breaking: 호환 불가 (타입 변경, 필수 프로퍼티 제거 등)
    pub fn check_compatibility(&self, other: &NodeLabelSchema) -> CompatibilityLevel {
        let mut compatibility = CompatibilityLevel::Full;

        // 1. 제거된 프로퍼티 확인
        for name in self.properties.keys() {
            if !other.properties.contains_key(name) {
                // 프로퍼티 제거: Forward 호환 (새 버전이 구 데이터 읽기 가능)
                compatibility = compatibility.merge(&CompatibilityLevel::Forward);
            }
        }

        // 2. 추가된 프로퍼티 확인
        for (name, new_prop) in &other.properties {
            if !self.properties.contains_key(name)
                && !new_prop.nullable
                && new_prop.default_value.is_none()
            {
                // 필수 프로퍼티 추가: Backward 호환 (구 버전이 새 데이터 읽기 가능)
                compatibility = compatibility.merge(&CompatibilityLevel::Backward);
            }
            // 선택적 프로퍼티 추가는 Full 호환 유지
        }

        // 3. 프로퍼티 타입 변경 확인
        for (name, old_prop) in &self.properties {
            if let Some(new_prop) = other.properties.get(name) {
                if old_prop.property_type != new_prop.property_type {
                    // 타입 변경: Breaking
                    compatibility = compatibility.merge(&CompatibilityLevel::Breaking);
                }

                // nullable → not-null 변경: Breaking
                if old_prop.nullable && !new_prop.nullable && new_prop.default_value.is_none() {
                    compatibility = compatibility.merge(&CompatibilityLevel::Breaking);
                }
            }
        }

        compatibility
    }

    /// 두 스키마가 호환되는지 확인
    pub fn is_compatible_with(&self, other: &NodeLabelSchema) -> bool {
        self.check_compatibility(other).is_compatible()
    }

    /// 스키마 차이점 분석
    pub fn diff(&self, other: &NodeLabelSchema) -> SchemaDiff {
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
impl ElementSchema for NodeLabelSchema {
    fn name(&self) -> &str {
        &self.label
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

/// 프로퍼티 변경 유형
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyChangeType {
    /// 프로퍼티 추가
    Added,
    /// 프로퍼티 제거
    Removed,
    /// 타입 변경
    TypeChanged,
    /// 필수 → 선택
    MadeOptional,
    /// 선택 → 필수
    MadeRequired,
}

/// 프로퍼티 변경 정보
#[derive(Debug, Clone)]
pub struct PropertyChange {
    /// 프로퍼티 이름
    pub name: String,
    /// 변경 유형
    pub change_type: PropertyChangeType,
    /// 이전 타입
    pub old_type: Option<super::types::PropertyType>,
    /// 새 타입
    pub new_type: Option<super::types::PropertyType>,
}

/// 스키마 차이점
#[derive(Debug, Clone)]
pub struct SchemaDiff {
    /// 이전 버전
    pub from_version: SchemaVersion,
    /// 새 버전
    pub to_version: SchemaVersion,
    /// 추가된 프로퍼티
    pub added: Vec<PropertyChange>,
    /// 제거된 프로퍼티
    pub removed: Vec<PropertyChange>,
    /// 변경된 프로퍼티
    pub modified: Vec<PropertyChange>,
    /// 호환성 수준
    pub compatibility: CompatibilityLevel,
}

impl SchemaDiff {
    /// 변경 사항이 있는지 확인
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty() || !self.modified.is_empty()
    }

    /// 총 변경 수
    pub fn change_count(&self) -> usize {
        self.added.len() + self.removed.len() + self.modified.len()
    }
}
