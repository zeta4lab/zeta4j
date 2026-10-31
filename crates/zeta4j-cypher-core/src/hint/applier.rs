// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 힌트 적용기 모듈

use super::types::{HintApplication, QueryHint};
use crate::ast::{PlanNode, PlanNodeType, QueryPlan};
use zeta4j_graph::error::Result;

/// 힌트 적용기
///
/// 실행 계획에 힌트를 적용하여 스캔 전략을 변경합니다.
pub struct HintApplier;

impl HintApplier {
    /// 단일 힌트 적용
    pub fn apply(plan: &mut QueryPlan, hint: &QueryHint) -> Result<HintApplication> {
        match hint {
            QueryHint::UseIndex {
                variable,
                label,
                property,
            } => Self::apply_index_hint(&mut plan.root, variable, label, property),
            QueryHint::UseScan { variable, label } => {
                Self::apply_scan_hint(&mut plan.root, variable, label)
            }
            QueryHint::UseJoin { variable } => Self::apply_join_hint(&mut plan.root, variable),
        }
    }

    /// 여러 힌트 적용
    pub fn apply_all(plan: &mut QueryPlan, hints: &[QueryHint]) -> Result<Vec<HintApplication>> {
        hints.iter().map(|hint| Self::apply(plan, hint)).collect()
    }

    /// INDEX 힌트 적용
    ///
    /// LabelScan 또는 AllNodesScan을 IndexScan으로 변경
    fn apply_index_hint(
        node: &mut PlanNode,
        variable: &str,
        label: &str,
        property: &str,
    ) -> Result<HintApplication> {
        let hint = QueryHint::use_index(variable, label, property);
        let original_description = Self::find_scan_node_description(node, variable);

        let modified = Self::transform_to_index_scan(node, variable, label, property);

        if modified {
            let new_description = format!("{}:{}({})", variable, label, property);
            Ok(HintApplication::applied(
                hint,
                original_description.unwrap_or_else(|| "LabelScan".to_string()),
                format!("IndexScan {}", new_description),
            ))
        } else {
            Ok(HintApplication::not_applied(
                hint,
                format!("No scan node found for variable '{}'", variable),
            ))
        }
    }

    /// SCAN 힌트 적용
    ///
    /// IndexScan을 LabelScan으로 변경 (인덱스 사용 방지)
    fn apply_scan_hint(
        node: &mut PlanNode,
        variable: &str,
        label: &str,
    ) -> Result<HintApplication> {
        let hint = QueryHint::use_scan(variable, label);
        let original_description = Self::find_scan_node_description(node, variable);

        let modified = Self::transform_to_label_scan(node, variable, label);

        if modified {
            let new_description = format!("{}:{}", variable, label);
            Ok(HintApplication::applied(
                hint,
                original_description.unwrap_or_else(|| "IndexScan".to_string()),
                format!("LabelScan {}", new_description),
            ))
        } else {
            Ok(HintApplication::not_applied(
                hint,
                format!("No scan node found for variable '{}'", variable),
            ))
        }
    }

    /// JOIN 힌트 적용
    ///
    /// Expand 노드에 조인 힌트 표시
    fn apply_join_hint(node: &mut PlanNode, variable: &str) -> Result<HintApplication> {
        let hint = QueryHint::use_join(variable);

        let modified = Self::mark_join_hint(node, variable);

        if modified {
            Ok(HintApplication::applied(
                hint,
                "Expand",
                format!("Expand (JOIN ON {})", variable),
            ))
        } else {
            Ok(HintApplication::not_applied(
                hint,
                format!("No Expand node found for variable '{}'", variable),
            ))
        }
    }

    /// 스캔 노드의 설명 찾기
    fn find_scan_node_description(node: &PlanNode, variable: &str) -> Option<String> {
        // 현재 노드가 스캔이고 변수와 일치하면 설명 반환
        if Self::is_scan_node(&node.node_type) && node.description.contains(variable) {
            return Some(format!("{} ({})", node.node_type, node.description));
        }

        // 자식 노드에서 재귀 검색
        for child in &node.children {
            if let Some(desc) = Self::find_scan_node_description(child, variable) {
                return Some(desc);
            }
        }

        None
    }

    /// 스캔 노드인지 확인
    fn is_scan_node(node_type: &PlanNodeType) -> bool {
        matches!(
            node_type,
            PlanNodeType::AllNodesScan
                | PlanNodeType::NodeByLabelScan
                | PlanNodeType::NodeByIndexScan
                | PlanNodeType::NodeByIdSeek
        )
    }

    /// IndexScan으로 변환
    fn transform_to_index_scan(
        node: &mut PlanNode,
        variable: &str,
        label: &str,
        property: &str,
    ) -> bool {
        // 현재 노드가 스캔이고 변수와 일치하면 변환
        if Self::is_scan_node(&node.node_type) && node.description.contains(variable) {
            node.node_type = PlanNodeType::NodeByIndexScan;
            node.description = format!("{}:{}({})", variable, label, property);
            return true;
        }

        // 자식 노드에서 재귀 변환
        for child in &mut node.children {
            if Self::transform_to_index_scan(child, variable, label, property) {
                return true;
            }
        }

        false
    }

    /// LabelScan으로 변환
    fn transform_to_label_scan(node: &mut PlanNode, variable: &str, label: &str) -> bool {
        // 현재 노드가 스캔이고 변수와 일치하면 변환
        if Self::is_scan_node(&node.node_type) && node.description.contains(variable) {
            node.node_type = PlanNodeType::NodeByLabelScan;
            node.description = format!("{}:{}", variable, label);
            return true;
        }

        // 자식 노드에서 재귀 변환
        for child in &mut node.children {
            if Self::transform_to_label_scan(child, variable, label) {
                return true;
            }
        }

        false
    }

    /// JOIN 힌트 표시
    fn mark_join_hint(node: &mut PlanNode, variable: &str) -> bool {
        // Expand 노드에서 변수와 관련된 노드 찾기
        if matches!(
            node.node_type,
            PlanNodeType::Expand | PlanNodeType::OptionalExpand
        ) {
            // description에 변수가 포함되어 있으면 힌트 표시
            if node.description.contains(variable) || node.variables.contains(&variable.to_string())
            {
                // JOIN 힌트 정보 추가
                if !node.description.contains("JOIN ON") {
                    node.description = format!("{} (JOIN ON {})", node.description, variable);
                }
                return true;
            }
        }

        // 자식 노드에서 재귀 검색
        for child in &mut node.children {
            if Self::mark_join_hint(child, variable) {
                return true;
            }
        }

        false
    }

    /// 적용 결과 요약
    pub fn summarize(applications: &[HintApplication]) -> String {
        let total = applications.len();
        let applied_count = applications.iter().filter(|a| a.applied).count();
        let not_applied_count = total - applied_count;

        let mut summary = format!(
            "Hint application: {} total, {} applied",
            total, applied_count
        );

        if not_applied_count > 0 {
            summary.push_str(&format!(", {} not applied\n", not_applied_count));
            for a in applications.iter().filter(|a| !a.applied) {
                summary.push_str(&format!("  - {}: {}\n", a.hint, a.original_plan));
            }
        }

        summary
    }
}
