// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Cardinality Estimation and Cost Calculation
//!
//! 실행 계획의 카디널리티 추정 및 비용 계산

use super::QueryPlanner;
use crate::ast::*;
use crate::optimizer::{CostModel, StatisticsManager};
use zeta4j_graph::error::Result;

impl QueryPlanner {
    /// 실행 계획의 카디널리티(예상 행 수)를 추정합니다.
    pub fn estimate_cardinality(&self, plan: &mut QueryPlan, stats: &StatisticsManager) {
        self.estimate_node_cardinality(&mut plan.root, stats);
    }

    /// 개별 노드의 카디널리티 추정 (재귀)
    fn estimate_node_cardinality(&self, node: &mut PlanNode, stats: &StatisticsManager) {
        for child in &mut node.children {
            self.estimate_node_cardinality(child, stats);
        }

        let input_rows: u64 = if node.children.is_empty() {
            1
        } else {
            node.children.iter().map(|c| c.estimated_rows).sum()
        };

        node.estimated_rows = match &node.node_type {
            PlanNodeType::AllNodesScan => stats.total_node_count().max(1),
            PlanNodeType::NodeByLabelScan => {
                let label = self.extract_label_from_description(&node.description);
                if let Some(l) = label {
                    stats.label_count(&l).max(1)
                } else {
                    stats.total_node_count() / 10
                }
            }
            PlanNodeType::NodeByIdSeek => 1,
            PlanNodeType::NodeByIndexScan => {
                let label = self.extract_label_from_description(&node.description);
                if let Some(l) = label {
                    let selectivity = stats.equality_selectivity(&l, "");
                    let label_count = stats.label_count(&l);
                    ((label_count as f64 * selectivity) as u64).max(1)
                } else {
                    10
                }
            }
            PlanNodeType::NodeByIndexRangeScan => {
                let label = self.extract_label_from_description(&node.description);
                if let Some(l) = label {
                    let label_count = stats.label_count(&l);
                    ((label_count as f64 * 0.1) as u64).max(1)
                } else {
                    100
                }
            }
            PlanNodeType::IndexIntersection => {
                let min_child_rows = node
                    .children
                    .iter()
                    .map(|c| c.estimated_rows)
                    .min()
                    .unwrap_or(1);
                ((min_child_rows as f64 * 0.5) as u64).max(1)
            }
            PlanNodeType::Filter => ((input_rows as f64 * 0.33) as u64).max(1),
            PlanNodeType::Expand | PlanNodeType::OptionalExpand => {
                let avg_degree = stats.expand_estimate(None, true);
                ((input_rows as f64 * avg_degree) as u64).max(1)
            }
            PlanNodeType::Sort => input_rows,
            PlanNodeType::Aggregation => ((input_rows as f64 * 0.1) as u64).max(1),
            PlanNodeType::Distinct => ((input_rows as f64 * 0.5) as u64).max(1),
            PlanNodeType::Skip => {
                let skip_count = node.description.parse::<u64>().unwrap_or(0);
                input_rows.saturating_sub(skip_count)
            }
            PlanNodeType::Limit => {
                let limit_count = node.description.parse::<u64>().unwrap_or(input_rows);
                input_rows.min(limit_count)
            }
            PlanNodeType::Union => {
                let left = node.children.first().map(|c| c.estimated_rows).unwrap_or(0);
                let right = node.children.get(1).map(|c| c.estimated_rows).unwrap_or(0);
                ((left + right) as f64 * 0.75) as u64
            }
            PlanNodeType::UnionAll => node
                .children
                .iter()
                .map(|c| c.estimated_rows)
                .sum::<u64>()
                .max(1),
            PlanNodeType::CartesianProduct => {
                let left = node.children.first().map(|c| c.estimated_rows).unwrap_or(1);
                let right = node.children.get(1).map(|c| c.estimated_rows).unwrap_or(1);
                left * right
            }
            PlanNodeType::Create => 1,
            PlanNodeType::Delete => input_rows,
            PlanNodeType::SetProperty
            | PlanNodeType::SetNodeProperties
            | PlanNodeType::SetNodePropertiesMerge
            | PlanNodeType::SetLabels
            | PlanNodeType::RemoveProperty
            | PlanNodeType::RemoveLabels => input_rows,
            PlanNodeType::Merge => input_rows.max(1),
            PlanNodeType::Unwind => input_rows * 10,
            PlanNodeType::Call => node.children.first().map(|c| c.estimated_rows).unwrap_or(1),
            PlanNodeType::Projection | PlanNodeType::ProduceResults => input_rows,
            PlanNodeType::Admin => 1,
            PlanNodeType::Foreach => input_rows,
            PlanNodeType::LoadCsv => 1000,
            PlanNodeType::EmptyResult => 0,
        };
    }

    /// description에서 레이블 추출
    pub(crate) fn extract_label_from_description(&self, description: &str) -> Option<String> {
        if let Some(pos) = description.find(':') {
            let after_colon = &description[pos + 1..];
            let label = after_colon
                .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '[' || c == ']')
                .next()
                .unwrap_or("");
            if !label.is_empty() {
                return Some(label.to_string());
            }
        }
        None
    }

    /// 실행 계획의 총 비용을 계산합니다.
    pub fn calculate_cost(&self, plan: &QueryPlan, stats: &StatisticsManager) -> f64 {
        let cost_model = CostModel::new();
        cost_model.total_plan_cost(&plan.root, stats)
    }

    /// 통계와 비용 모델을 적용하여 최적화된 실행 계획을 생성합니다.
    pub fn plan_with_stats(
        &self,
        query: &Query,
        stats: &StatisticsManager,
    ) -> Result<(QueryPlan, f64)> {
        let mut plan = self.plan(query)?;
        self.estimate_cardinality(&mut plan, stats);
        let cost = self.calculate_cost(&plan, stats);
        Ok((plan, cost))
    }

    /// EXPLAIN 쿼리의 상세 실행 계획 출력을 생성합니다.
    pub fn explain(&self, query: &Query, stats: &StatisticsManager) -> Result<String> {
        let (plan, cost) = self.plan_with_stats(query, stats)?;
        Ok(plan.to_explain_detail(cost))
    }
}
