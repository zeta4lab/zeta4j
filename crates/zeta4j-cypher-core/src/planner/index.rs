//! Index Scan Planning
//!
//! 인덱스 스캔 최적화 및 관련 함수들

use super::QueryPlanner;
use crate::ast::*;

impl QueryPlanner {
    /// 인덱스 스캔 가능 여부 확인 및 계획 생성
    pub(crate) fn try_plan_index_scan(
        &self,
        pattern: &Pattern,
        where_expr: &Expression,
    ) -> Option<PlanNode> {
        let node_pattern = pattern.parts.first()?;
        let variable = node_pattern.variable.as_ref()?;
        let label = node_pattern.labels.first()?;

        let conditions = self.flatten_and_conditions(where_expr);

        use std::collections::HashMap;
        let mut equality_conditions: HashMap<String, String> = HashMap::new();
        let mut range_conditions: HashMap<String, IndexCondition> = HashMap::new();
        let mut remaining_conditions: Vec<Expression> = Vec::new();

        for cond in conditions {
            if let Some((prop, value_desc)) = self.extract_simple_equality(&cond, variable)
                && self.has_index(label, &prop)
            {
                equality_conditions.insert(prop, value_desc);
                continue;
            }

            if let Some(range_cond) = self.extract_simple_range(&cond, variable) {
                let prop = range_cond.property().to_string();
                if self.has_index(label, &prop) {
                    if let Some(existing) = range_conditions.get(&prop) {
                        if let Some(merged) = self.merge_range_conditions(existing, &range_cond) {
                            range_conditions.insert(prop, merged);
                        } else {
                            range_conditions.insert(prop, range_cond);
                        }
                    } else {
                        range_conditions.insert(prop, range_cond);
                    }
                    continue;
                }
            }

            remaining_conditions.push(cond);
        }

        let mut index_scans: Vec<PlanNode> = Vec::new();

        for (prop, value_desc) in equality_conditions {
            let desc = format!(
                "{}:{} USING INDEX ON :{}({}) WHERE {}.{} = {}",
                variable, label, label, prop, variable, prop, value_desc
            );
            index_scans.push(PlanNode::new(PlanNodeType::NodeByIndexScan, desc));
        }

        for (_, range_cond) in range_conditions {
            let desc = self.format_range_condition_desc(variable, label, &range_cond);
            index_scans.push(PlanNode::new(PlanNodeType::NodeByIndexRangeScan, desc));
        }

        if index_scans.is_empty() {
            return None;
        }

        let scan_node = if index_scans.len() == 1 {
            index_scans.remove(0)
        } else {
            let mut intersection = PlanNode::new(
                PlanNodeType::IndexIntersection,
                format!("{}:{} ({} indexes)", variable, label, index_scans.len()),
            );
            for scan in index_scans {
                intersection = intersection.with_child(scan);
            }
            intersection
        };

        if remaining_conditions.is_empty() {
            Some(scan_node)
        } else {
            let remaining_expr = self.combine_and_conditions(remaining_conditions);
            let filter_desc = self.expr_to_string(&remaining_expr);
            Some(PlanNode::new(PlanNodeType::Filter, filter_desc).with_child(scan_node))
        }
    }

    /// AND 조건을 평탄화하여 개별 조건 목록으로 분리
    pub(crate) fn flatten_and_conditions(&self, expr: &Expression) -> Vec<Expression> {
        match expr {
            Expression::Logical {
                left: Some(left),
                op: LogicalOp::And,
                right,
            } => {
                let mut result = self.flatten_and_conditions(left);
                result.extend(self.flatten_and_conditions(right));
                result
            }
            _ => vec![expr.clone()],
        }
    }

    /// 개별 조건들을 AND로 결합
    pub(crate) fn combine_and_conditions(&self, conditions: Vec<Expression>) -> Expression {
        conditions
            .into_iter()
            .reduce(|acc, cond| Expression::Logical {
                left: Some(Box::new(acc)),
                op: LogicalOp::And,
                right: Box::new(cond),
            })
            .unwrap_or(Expression::Literal(zeta4j_graph::core::Value::Boolean(
                true,
            )))
    }

    /// 단순 등호 조건 추출 (n.prop = value)
    pub(crate) fn extract_simple_equality(
        &self,
        expr: &Expression,
        variable: &str,
    ) -> Option<(String, String)> {
        match expr {
            Expression::Comparison {
                left,
                op: ComparisonOp::Equal,
                right,
            } => {
                if let Expression::PropertyAccess(var, prop) = left.as_ref()
                    && var == variable
                {
                    return Some((prop.clone(), self.expr_to_string(right)));
                }
                if let Expression::PropertyAccess(var, prop) = right.as_ref()
                    && var == variable
                {
                    return Some((prop.clone(), self.expr_to_string(left)));
                }
                None
            }
            _ => None,
        }
    }

    /// 단순 범위 조건 추출 (n.prop > value 등)
    pub(crate) fn extract_simple_range(
        &self,
        expr: &Expression,
        variable: &str,
    ) -> Option<IndexCondition> {
        match expr {
            Expression::Comparison { left, op, right } => {
                if let Expression::PropertyAccess(var, prop) = left.as_ref()
                    && var == variable
                {
                    return self.make_simple_range_condition(prop, op, right, false);
                }
                if let Expression::PropertyAccess(var, prop) = right.as_ref()
                    && var == variable
                {
                    return self.make_simple_range_condition(prop, op, left, true);
                }
                None
            }
            _ => None,
        }
    }

    /// 비교 연산자에서 단순 범위 조건 생성
    pub(crate) fn make_simple_range_condition(
        &self,
        prop: &str,
        op: &ComparisonOp,
        value: &Expression,
        inverted: bool,
    ) -> Option<IndexCondition> {
        match (op, inverted) {
            (ComparisonOp::GreaterThan, false) | (ComparisonOp::LessThan, true) => {
                Some(IndexCondition::greater_than(prop, value.clone(), false))
            }
            (ComparisonOp::GreaterEqual, false) | (ComparisonOp::LessEqual, true) => {
                Some(IndexCondition::greater_than(prop, value.clone(), true))
            }
            (ComparisonOp::LessThan, false) | (ComparisonOp::GreaterThan, true) => {
                Some(IndexCondition::less_than(prop, value.clone(), false))
            }
            (ComparisonOp::LessEqual, false) | (ComparisonOp::GreaterEqual, true) => {
                Some(IndexCondition::less_than(prop, value.clone(), true))
            }
            _ => None,
        }
    }

    /// 범위 조건 설명 문자열 생성
    pub(crate) fn format_range_condition_desc(
        &self,
        variable: &str,
        label: &str,
        condition: &IndexCondition,
    ) -> String {
        match condition {
            IndexCondition::Equality { property, value } => {
                format!(
                    "{}:{} USING INDEX ON :{}({}) WHERE {}.{} = {}",
                    variable,
                    label,
                    label,
                    property,
                    variable,
                    property,
                    self.expr_to_string(value)
                )
            }
            IndexCondition::Range {
                property,
                lower,
                upper,
            } => {
                let mut parts = Vec::new();

                if let Some(bound) = lower {
                    let op = if bound.inclusive { ">=" } else { ">" };
                    parts.push(format!(
                        "{}.{} {} {}",
                        variable,
                        property,
                        op,
                        self.expr_to_string(&bound.value)
                    ));
                }
                if let Some(bound) = upper {
                    let op = if bound.inclusive { "<=" } else { "<" };
                    parts.push(format!(
                        "{}.{} {} {}",
                        variable,
                        property,
                        op,
                        self.expr_to_string(&bound.value)
                    ));
                }

                let where_clause = parts.join(" AND ");
                format!(
                    "{}:{} USING INDEX ON :{}({}) WHERE {}",
                    variable, label, label, property, where_clause
                )
            }
        }
    }

    /// WHERE 절에서 인덱스 사용 가능한 조건 추출
    #[allow(dead_code)]
    pub(crate) fn extract_index_condition(
        &self,
        expr: &Expression,
        variable: &str,
    ) -> Option<(String, String, Option<Expression>)> {
        match expr {
            Expression::Comparison {
                left,
                op: ComparisonOp::Equal,
                right,
            } => {
                if let Expression::PropertyAccess(var, prop) = left.as_ref()
                    && var == variable
                {
                    let value_desc = self.expr_to_string(right);
                    return Some((prop.clone(), value_desc, None));
                }
                if let Expression::PropertyAccess(var, prop) = right.as_ref()
                    && var == variable
                {
                    let value_desc = self.expr_to_string(left);
                    return Some((prop.clone(), value_desc, None));
                }
                None
            }
            Expression::Logical {
                left: Some(left),
                op: LogicalOp::And,
                right,
            } => {
                if let Some((prop, value, _)) = self.extract_index_condition(left, variable) {
                    return Some((prop, value, Some(right.as_ref().clone())));
                }
                if let Some((prop, value, _)) = self.extract_index_condition(right, variable) {
                    return Some((prop, value, Some(left.as_ref().clone())));
                }
                None
            }
            _ => None,
        }
    }

    /// WHERE 절에서 범위 인덱스 조건 추출
    #[allow(dead_code)]
    pub(crate) fn extract_range_condition(
        &self,
        expr: &Expression,
        variable: &str,
    ) -> Option<(IndexCondition, Option<Expression>)> {
        match expr {
            Expression::Comparison { left, op, right } => {
                if let Expression::PropertyAccess(var, prop) = left.as_ref()
                    && var == variable
                {
                    return self.make_range_condition(prop, op, right, false);
                }
                if let Expression::PropertyAccess(var, prop) = right.as_ref()
                    && var == variable
                {
                    return self.make_range_condition(prop, op, left, true);
                }
                None
            }
            Expression::Logical {
                left: Some(left_expr),
                op: LogicalOp::And,
                right: right_expr,
            } => {
                if let Some((left_cond, _)) = self.extract_range_condition(left_expr, variable) {
                    if let Some((right_cond, _)) =
                        self.extract_range_condition(right_expr, variable)
                        && let Some(merged) = self.merge_range_conditions(&left_cond, &right_cond)
                    {
                        return Some((merged, None));
                    }
                    return Some((left_cond, Some(right_expr.as_ref().clone())));
                }
                if let Some((right_cond, _)) = self.extract_range_condition(right_expr, variable) {
                    return Some((right_cond, Some(left_expr.as_ref().clone())));
                }
                None
            }
            _ => None,
        }
    }

    /// 비교 연산자에서 범위 조건 생성
    #[allow(dead_code)]
    pub(crate) fn make_range_condition(
        &self,
        prop: &str,
        op: &ComparisonOp,
        value: &Expression,
        inverted: bool,
    ) -> Option<(IndexCondition, Option<Expression>)> {
        let condition = match (op, inverted) {
            (ComparisonOp::GreaterThan, false) | (ComparisonOp::LessThan, true) => {
                IndexCondition::greater_than(prop, value.clone(), false)
            }
            (ComparisonOp::GreaterEqual, false) | (ComparisonOp::LessEqual, true) => {
                IndexCondition::greater_than(prop, value.clone(), true)
            }
            (ComparisonOp::LessThan, false) | (ComparisonOp::GreaterThan, true) => {
                IndexCondition::less_than(prop, value.clone(), false)
            }
            (ComparisonOp::LessEqual, false) | (ComparisonOp::GreaterEqual, true) => {
                IndexCondition::less_than(prop, value.clone(), true)
            }
            _ => return None,
        };
        Some((condition, None))
    }

    /// 두 범위 조건 병합 (같은 프로퍼티일 경우)
    pub(crate) fn merge_range_conditions(
        &self,
        cond1: &IndexCondition,
        cond2: &IndexCondition,
    ) -> Option<IndexCondition> {
        match (cond1, cond2) {
            (
                IndexCondition::Range {
                    property: prop1,
                    lower: lower1,
                    upper: upper1,
                },
                IndexCondition::Range {
                    property: prop2,
                    lower: lower2,
                    upper: upper2,
                },
            ) if prop1 == prop2 => {
                let merged_lower = lower1.clone().or_else(|| lower2.clone());
                let merged_upper = upper1.clone().or_else(|| upper2.clone());

                if merged_lower.is_some() || merged_upper.is_some() {
                    Some(IndexCondition::Range {
                        property: prop1.clone(),
                        lower: merged_lower,
                        upper: merged_upper,
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
