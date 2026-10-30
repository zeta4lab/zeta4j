//! 힌트 통합 관리자 모듈

use super::applier::HintApplier;
use super::parser::HintParser;
use super::types::{HintApplication, HintProcessResult, HintValidation, IndexInfo, QueryHint};
use super::validator::HintValidator;
use crate::ast::QueryPlan;
use std::collections::HashSet;
use zeta4j_graph::error::Result;

/// 힌트 통합 관리자
///
/// 힌트의 파싱, 검증, 적용을 통합 관리합니다.
pub struct HintManager;

impl HintManager {
    /// 새 HintManager 생성
    pub fn new() -> Self {
        Self
    }

    /// 힌트 파싱
    pub fn parse(&self, hint_text: &str) -> Result<Vec<QueryHint>> {
        HintParser::parse_all(hint_text)
    }

    /// 쿼리에서 힌트 추출
    pub fn extract_hints(&self, query: &str) -> Result<Vec<QueryHint>> {
        HintParser::extract_from_query(query)
    }

    /// 힌트 검증
    pub fn validate(
        &self,
        hints: &[QueryHint],
        query_variables: &HashSet<String>,
        available_indexes: &[IndexInfo],
    ) -> Vec<HintValidation> {
        HintValidator::validate_all(hints, query_variables, available_indexes)
    }

    /// 힌트 충돌 검사
    pub fn check_conflicts(&self, hints: &[QueryHint]) -> Vec<String> {
        HintValidator::check_conflicts(hints)
    }

    /// 힌트 적용
    pub fn apply(&self, plan: &mut QueryPlan, hints: &[QueryHint]) -> Result<Vec<HintApplication>> {
        HintApplier::apply_all(plan, hints)
    }

    /// 전체 프로세스 (파싱 -> 검증 -> 적용)
    pub fn process(
        &self,
        hint_text: &str,
        plan: &mut QueryPlan,
        query_variables: &HashSet<String>,
        available_indexes: &[IndexInfo],
    ) -> Result<HintProcessResult> {
        let mut result = HintProcessResult::new();

        // 1. 파싱
        let hints = self.parse(hint_text)?;
        for hint in &hints {
            result.add_hint(hint.clone());
        }

        if hints.is_empty() {
            return Ok(result);
        }

        // 2. 충돌 검사
        let conflicts = self.check_conflicts(&hints);
        for conflict in conflicts {
            result.add_warning(conflict);
        }

        // 3. 검증
        let validations = self.validate(&hints, query_variables, available_indexes);
        for validation in &validations {
            result.add_validation(validation.clone());
        }

        // 유효한 힌트만 적용
        let valid_hints: Vec<_> = validations
            .iter()
            .filter(|v| v.valid)
            .map(|v| v.hint.clone())
            .collect();

        // 4. 적용
        let applications = self.apply(plan, &valid_hints)?;
        for application in applications {
            result.add_application(application);
        }

        Ok(result)
    }

    /// 쿼리에서 힌트를 추출하여 전체 프로세스 실행
    pub fn process_query(
        &self,
        query: &str,
        plan: &mut QueryPlan,
        query_variables: &HashSet<String>,
        available_indexes: &[IndexInfo],
    ) -> Result<HintProcessResult> {
        let mut result = HintProcessResult::new();

        // 1. 쿼리에서 힌트 추출
        let hints = self.extract_hints(query)?;
        for hint in &hints {
            result.add_hint(hint.clone());
        }

        if hints.is_empty() {
            return Ok(result);
        }

        // 2. 충돌 검사
        let conflicts = self.check_conflicts(&hints);
        for conflict in conflicts {
            result.add_warning(conflict);
        }

        // 3. 검증
        let validations = self.validate(&hints, query_variables, available_indexes);
        for validation in &validations {
            result.add_validation(validation.clone());
        }

        // 유효한 힌트만 적용
        let valid_hints: Vec<_> = validations
            .iter()
            .filter(|v| v.valid)
            .map(|v| v.hint.clone())
            .collect();

        // 4. 적용
        let applications = self.apply(plan, &valid_hints)?;
        for application in applications {
            result.add_application(application);
        }

        Ok(result)
    }

    /// 결과 요약 문자열 생성
    pub fn summarize(&self, result: &HintProcessResult) -> String {
        let mut summary = String::new();

        // 힌트 개수 요약
        summary.push_str(&format!(
            "Hint processing result: {} total\n",
            result.hints.len()
        ));

        // 검증 요약
        if !result.validations.is_empty() {
            summary.push_str(&HintValidator::summarize(&result.validations));
        }

        // 적용 요약
        if !result.applications.is_empty() {
            summary.push_str(&HintApplier::summarize(&result.applications));
        }

        // 경고 출력
        if !result.warnings.is_empty() {
            summary.push_str("\nWarnings:\n");
            for warning in &result.warnings {
                summary.push_str(&format!("  - {}\n", warning));
            }
        }

        summary
    }
}

impl Default for HintManager {
    fn default() -> Self {
        Self::new()
    }
}
