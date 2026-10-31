// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 힌트 시스템 타입 정의

use std::fmt;

/// 쿼리 힌트 타입
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QueryHint {
    /// 인덱스 힌트: USING INDEX var:Label(property)
    UseIndex {
        /// 변수명
        variable: String,
        /// 레이블
        label: String,
        /// 속성명
        property: String,
    },
    /// 스캔 힌트: USING SCAN var:Label
    UseScan {
        /// 변수명
        variable: String,
        /// 레이블
        label: String,
    },
    /// 조인 힌트: USING JOIN ON var
    UseJoin {
        /// 변수명
        variable: String,
    },
}

impl QueryHint {
    /// 인덱스 힌트 생성
    pub fn use_index(
        variable: impl Into<String>,
        label: impl Into<String>,
        property: impl Into<String>,
    ) -> Self {
        Self::UseIndex {
            variable: variable.into(),
            label: label.into(),
            property: property.into(),
        }
    }

    /// 스캔 힌트 생성
    pub fn use_scan(variable: impl Into<String>, label: impl Into<String>) -> Self {
        Self::UseScan {
            variable: variable.into(),
            label: label.into(),
        }
    }

    /// 조인 힌트 생성
    pub fn use_join(variable: impl Into<String>) -> Self {
        Self::UseJoin {
            variable: variable.into(),
        }
    }

    /// 힌트 대상 변수
    pub fn variable(&self) -> &str {
        match self {
            QueryHint::UseIndex { variable, .. } => variable,
            QueryHint::UseScan { variable, .. } => variable,
            QueryHint::UseJoin { variable } => variable,
        }
    }

    /// 힌트 타입 이름
    pub fn hint_type(&self) -> &'static str {
        match self {
            QueryHint::UseIndex { .. } => "INDEX",
            QueryHint::UseScan { .. } => "SCAN",
            QueryHint::UseJoin { .. } => "JOIN",
        }
    }

    /// 힌트 설명
    pub fn description(&self) -> String {
        match self {
            QueryHint::UseIndex {
                variable,
                label,
                property,
            } => {
                format!("USING INDEX {}:{}({})", variable, label, property)
            }
            QueryHint::UseScan { variable, label } => {
                format!("USING SCAN {}:{}", variable, label)
            }
            QueryHint::UseJoin { variable } => {
                format!("USING JOIN ON {}", variable)
            }
        }
    }

    /// 레이블 반환 (있는 경우)
    pub fn label(&self) -> Option<&str> {
        match self {
            QueryHint::UseIndex { label, .. } => Some(label),
            QueryHint::UseScan { label, .. } => Some(label),
            QueryHint::UseJoin { .. } => None,
        }
    }

    /// 속성 반환 (있는 경우)
    pub fn property(&self) -> Option<&str> {
        match self {
            QueryHint::UseIndex { property, .. } => Some(property),
            _ => None,
        }
    }
}

impl fmt::Display for QueryHint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// 인덱스 정보
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexInfo {
    /// 레이블
    pub label: String,
    /// 속성명
    pub property: String,
    /// 인덱스 이름
    pub name: Option<String>,
}

impl IndexInfo {
    /// 새 인덱스 정보 생성
    pub fn new(label: impl Into<String>, property: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            property: property.into(),
            name: None,
        }
    }

    /// 이름과 함께 생성
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// 힌트와 일치하는지 확인
    pub fn matches_hint(&self, hint: &QueryHint) -> bool {
        match hint {
            QueryHint::UseIndex {
                label, property, ..
            } => &self.label == label && &self.property == property,
            _ => false,
        }
    }
}

/// 힌트 검증 결과
#[derive(Debug, Clone)]
pub struct HintValidation {
    /// 원본 힌트
    pub hint: QueryHint,
    /// 유효 여부
    pub valid: bool,
    /// 이유 (무효한 경우)
    pub reason: Option<String>,
}

impl HintValidation {
    /// 유효한 검증 결과 생성
    pub fn valid(hint: QueryHint) -> Self {
        Self {
            hint,
            valid: true,
            reason: None,
        }
    }

    /// 무효한 검증 결과 생성
    pub fn invalid(hint: QueryHint, reason: impl Into<String>) -> Self {
        Self {
            hint,
            valid: false,
            reason: Some(reason.into()),
        }
    }
}

/// 힌트 적용 결과
#[derive(Debug, Clone)]
pub struct HintApplication {
    /// 원본 힌트
    pub hint: QueryHint,
    /// 적용 여부
    pub applied: bool,
    /// 원본 계획 설명
    pub original_plan: String,
    /// 변경된 계획 설명
    pub modified_plan: String,
}

impl HintApplication {
    /// 적용 성공
    pub fn applied(
        hint: QueryHint,
        original: impl Into<String>,
        modified: impl Into<String>,
    ) -> Self {
        Self {
            hint,
            applied: true,
            original_plan: original.into(),
            modified_plan: modified.into(),
        }
    }

    /// 적용 실패
    pub fn not_applied(hint: QueryHint, reason: impl Into<String>) -> Self {
        Self {
            hint,
            applied: false,
            original_plan: reason.into(),
            modified_plan: String::new(),
        }
    }
}

/// 힌트 처리 결과
#[derive(Debug, Clone, Default)]
pub struct HintProcessResult {
    /// 파싱된 힌트들
    pub hints: Vec<QueryHint>,
    /// 검증 결과들
    pub validations: Vec<HintValidation>,
    /// 적용 결과들
    pub applications: Vec<HintApplication>,
    /// 경고 메시지들
    pub warnings: Vec<String>,
}

impl HintProcessResult {
    /// 새 결과 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 힌트 추가
    pub fn add_hint(&mut self, hint: QueryHint) {
        self.hints.push(hint);
    }

    /// 검증 결과 추가
    pub fn add_validation(&mut self, validation: HintValidation) {
        self.validations.push(validation);
    }

    /// 적용 결과 추가
    pub fn add_application(&mut self, application: HintApplication) {
        self.applications.push(application);
    }

    /// 경고 추가
    pub fn add_warning(&mut self, warning: impl Into<String>) {
        self.warnings.push(warning.into());
    }

    /// 모든 힌트가 유효한지 확인
    pub fn all_valid(&self) -> bool {
        self.validations.iter().all(|v| v.valid)
    }

    /// 모든 힌트가 적용되었는지 확인
    pub fn all_applied(&self) -> bool {
        self.applications.iter().all(|a| a.applied)
    }

    /// 유효한 힌트 수
    pub fn valid_count(&self) -> usize {
        self.validations.iter().filter(|v| v.valid).count()
    }

    /// 적용된 힌트 수
    pub fn applied_count(&self) -> usize {
        self.applications.iter().filter(|a| a.applied).count()
    }
}
