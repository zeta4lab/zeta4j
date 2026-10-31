// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 힌트 검증기 모듈

use super::types::{HintValidation, IndexInfo, QueryHint};
use std::collections::HashSet;

/// 힌트 검증기
///
/// 힌트가 유효한지 검증합니다:
/// - 변수가 쿼리에 존재하는지
/// - 인덱스가 존재하는지 (USING INDEX)
/// - 레이블이 유효한지
pub struct HintValidator;

impl HintValidator {
    /// 단일 힌트 검증
    pub fn validate(
        hint: &QueryHint,
        query_variables: &HashSet<String>,
        available_indexes: &[IndexInfo],
    ) -> HintValidation {
        // 1. 변수 존재 여부 확인
        let variable = hint.variable();
        if !query_variables.contains(variable) {
            return HintValidation::invalid(
                hint.clone(),
                format!("Variable '{}' is not defined in the query", variable),
            );
        }

        // 2. 힌트 타입별 추가 검증
        match hint {
            QueryHint::UseIndex {
                label, property, ..
            } => {
                // 인덱스 존재 여부 확인
                let index_exists = available_indexes
                    .iter()
                    .any(|idx| &idx.label == label && &idx.property == property);

                if !index_exists {
                    return HintValidation::invalid(
                        hint.clone(),
                        format!("Index '{}({})' does not exist", label, property),
                    );
                }
            }
            QueryHint::UseScan { .. } => {
                // SCAN 힌트는 특별한 추가 검증 없음
            }
            QueryHint::UseJoin { .. } => {
                // JOIN 힌트는 특별한 추가 검증 없음
            }
        }

        HintValidation::valid(hint.clone())
    }

    /// 여러 힌트 검증
    pub fn validate_all(
        hints: &[QueryHint],
        query_variables: &HashSet<String>,
        available_indexes: &[IndexInfo],
    ) -> Vec<HintValidation> {
        hints
            .iter()
            .map(|hint| Self::validate(hint, query_variables, available_indexes))
            .collect()
    }

    /// 힌트 간 충돌 검사
    ///
    /// 동일 변수에 대해 INDEX와 SCAN 힌트가 동시에 있으면 충돌
    pub fn check_conflicts(hints: &[QueryHint]) -> Vec<String> {
        let mut conflicts = Vec::new();
        let mut index_vars: HashSet<&str> = HashSet::new();
        let mut scan_vars: HashSet<&str> = HashSet::new();

        for hint in hints {
            match hint {
                QueryHint::UseIndex { variable, .. } => {
                    index_vars.insert(variable);
                }
                QueryHint::UseScan { variable, .. } => {
                    scan_vars.insert(variable);
                }
                _ => {}
            }
        }

        // INDEX와 SCAN이 동일 변수에 있으면 충돌
        for var in index_vars.intersection(&scan_vars) {
            conflicts.push(format!(
                "Conflict: INDEX and SCAN hints are both specified for variable '{}'",
                var
            ));
        }

        conflicts
    }

    /// 검증 결과 요약
    pub fn summarize(validations: &[HintValidation]) -> String {
        let total = validations.len();
        let valid_count = validations.iter().filter(|v| v.valid).count();
        let invalid_count = total - valid_count;

        let mut summary = format!("Hint validation: {} total, {} valid", total, valid_count);

        if invalid_count > 0 {
            summary.push_str(&format!(", {} invalid\n", invalid_count));
            for v in validations.iter().filter(|v| !v.valid) {
                if let Some(reason) = &v.reason {
                    summary.push_str(&format!("  - {}: {}\n", v.hint, reason));
                }
            }
        }

        summary
    }
}
