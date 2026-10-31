// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 공통 스키마 트레이트
//!
//! NodeLabelSchema와 RelationshipTypeSchema의 공통 기능을 정의합니다.
//!
//! # v1.60.0 추가
//!
//! - ElementSchema 트레이트: 공통 접근자 및 기본 구현
//! - impl_element_schema_builder! 매크로: 빌더 패턴 메서드

use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

use super::evolution::SchemaVersion;
use super::index_hint::{IndexHint, IndexHints};
use super::types::PropertyDefinition;
use crate::core::Value;

/// 스키마 요소의 공통 인터페이스
///
/// NodeLabelSchema와 RelationshipTypeSchema가 공유하는 메서드를 정의합니다.
pub trait ElementSchema {
    // ========== 필수 접근자 ==========

    /// 스키마 이름 (라벨 또는 관계 타입)
    fn name(&self) -> &str;

    /// 프로퍼티 정의 맵 참조
    fn properties(&self) -> &HashMap<String, PropertyDefinition>;

    /// 프로퍼티 정의 맵 가변 참조
    fn properties_mut(&mut self) -> &mut HashMap<String, PropertyDefinition>;

    /// 필수 프로퍼티 집합 참조
    fn required_properties(&self) -> &HashSet<String>;

    /// 필수 프로퍼티 집합 가변 참조
    fn required_properties_mut(&mut self) -> &mut HashSet<String>;

    /// 스키마 버전 참조
    fn version(&self) -> &SchemaVersion;

    /// 스키마 버전 가변 참조
    fn version_mut(&mut self) -> &mut SchemaVersion;

    /// 인덱스 힌트 참조
    fn index_hints(&self) -> &IndexHints;

    /// 인덱스 힌트 가변 참조
    fn index_hints_mut(&mut self) -> &mut IndexHints;

    // ========== 버전 관리 기본 구현 ==========

    /// 버전 증가 (주 버전)
    fn bump_major(&mut self) {
        let new_version = self.version().bump_major();
        *self.version_mut() = new_version;
    }

    /// 버전 증가 (부 버전)
    fn bump_minor(&mut self) {
        let new_version = self.version().bump_minor();
        *self.version_mut() = new_version;
    }

    /// 버전 증가 (패치 버전)
    fn bump_patch(&mut self) {
        let new_version = self.version().bump_patch();
        *self.version_mut() = new_version;
    }

    // ========== 인덱스 힌트 기본 구현 ==========

    /// 인덱스 힌트 추가
    fn add_index(&mut self, hint: IndexHint) {
        self.index_hints_mut().add(hint);
    }

    /// 인덱스 힌트 목록 반환
    fn get_index_hints(&self) -> &IndexHints {
        self.index_hints()
    }

    /// 특정 프로퍼티에 대한 인덱스 힌트 조회
    fn get_index_for_property(&self, property: &str) -> Option<&IndexHint> {
        self.index_hints().get_for_property(property)
    }

    /// 인덱스 힌트가 있는지 확인
    fn has_index_hints(&self) -> bool {
        !self.index_hints().is_empty()
    }

    // ========== 프로퍼티 기본 구현 ==========

    /// 프로퍼티 추가
    fn add_property(&mut self, definition: PropertyDefinition) {
        let name = definition.name.clone();
        if !definition.nullable && definition.default_value.is_none() {
            self.required_properties_mut().insert(name.clone());
        }
        self.properties_mut().insert(name, definition);
    }

    /// 프로퍼티 정의 조회
    fn get_property(&self, name: &str) -> Option<&PropertyDefinition> {
        self.properties().get(name)
    }

    /// 프로퍼티 존재 여부 확인
    fn has_property(&self, name: &str) -> bool {
        self.properties().contains_key(name)
    }

    /// 모든 프로퍼티 이름 반환
    fn property_names(&self) -> Vec<&str> {
        self.properties().keys().map(|s| s.as_str()).collect()
    }

    /// 기본값 적용
    fn apply_defaults(&self, properties: &mut IndexMap<String, Value>) {
        for (name, def) in self.properties() {
            if (!properties.contains_key(name) || properties.get(name) == Some(&Value::Null))
                && let Some(default) = &def.default_value
            {
                properties.insert(name.clone(), default.clone());
            }
        }
    }
}

/// 빌더 패턴 메서드 구현 매크로
///
/// `Self`를 반환하는 빌더 메서드들을 생성합니다.
/// 트레이트의 기본 구현에서는 `Self` 반환이 불가능하므로 매크로로 제공합니다.
#[macro_export]
macro_rules! impl_element_schema_builder {
    () => {
        /// 버전 지정하여 스키마 생성
        pub fn with_version(mut self, version: $crate::schema::evolution::SchemaVersion) -> Self {
            *<Self as $crate::schema::ElementSchema>::version_mut(&mut self) = version;
            self
        }

        /// 버전 문자열로 스키마 생성 (예: "1.0.0")
        pub fn with_version_str(mut self, version: &str) -> Self {
            if let Ok(v) = $crate::schema::evolution::SchemaVersion::parse(version) {
                *<Self as $crate::schema::ElementSchema>::version_mut(&mut self) = v;
            }
            self
        }

        /// 빌더 패턴: 인덱스 힌트 추가
        pub fn with_index(mut self, hint: $crate::schema::index_hint::IndexHint) -> Self {
            <Self as $crate::schema::ElementSchema>::add_index(&mut self, hint);
            self
        }

        /// 빌더 패턴: 프로퍼티 추가
        pub fn with_property(
            mut self,
            definition: $crate::schema::types::PropertyDefinition,
        ) -> Self {
            <Self as $crate::schema::ElementSchema>::add_property(&mut self, definition);
            self
        }
    };
}

pub use impl_element_schema_builder;
