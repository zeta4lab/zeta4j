// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Helper Methods
//!
//! 문자열 변환 및 기타 헬퍼 함수들

use super::QueryPlanner;
use crate::ast::*;

impl QueryPlanner {
    /// 집계 함수 포함 여부 검사
    pub(crate) fn has_aggregate(&self, items: &[ReturnItem]) -> bool {
        items.iter().any(|item| {
            if let ReturnItem::Expression { expr, .. } = item {
                self.expr_has_aggregate(expr)
            } else {
                false
            }
        })
    }

    fn expr_has_aggregate(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Aggregate(_) => true,
            Expression::Comparison { left, right, .. } => {
                self.expr_has_aggregate(left) || self.expr_has_aggregate(right)
            }
            Expression::Logical { left, right, .. } => {
                left.as_ref().is_some_and(|l| self.expr_has_aggregate(l))
                    || self.expr_has_aggregate(right)
            }
            _ => false,
        }
    }

    /// 표현식을 문자열로 변환
    pub(crate) fn expr_to_string(&self, expr: &Expression) -> String {
        match expr {
            Expression::Literal(v) => format!("{:?}", v),
            Expression::Variable(v) => v.clone(),
            Expression::PropertyAccess(var, prop) => format!("{}.{}", var, prop),
            Expression::Comparison { left, op, right } => {
                format!(
                    "{} {} {}",
                    self.expr_to_string(left),
                    self.op_to_string(op),
                    self.expr_to_string(right)
                )
            }
            Expression::Logical { left, op, right } => {
                let left_str = left
                    .as_ref()
                    .map_or(String::new(), |l| self.expr_to_string(l));
                let right_str = self.expr_to_string(right);
                match op {
                    LogicalOp::Not => format!("NOT {}", right_str),
                    LogicalOp::And => format!("{} AND {}", left_str, right_str),
                    LogicalOp::Or => format!("{} OR {}", left_str, right_str),
                    LogicalOp::Xor => format!("{} XOR {}", left_str, right_str),
                }
            }
            Expression::IsNull { expr, negated } => {
                if *negated {
                    format!("{} IS NOT NULL", self.expr_to_string(expr))
                } else {
                    format!("{} IS NULL", self.expr_to_string(expr))
                }
            }
            Expression::Aggregate(agg) => match agg.function {
                AggregateFunction::CountAll => "COUNT(*)".to_string(),
                AggregateFunction::Count => format!(
                    "COUNT({})",
                    agg.argument
                        .as_ref()
                        .map_or("".to_string(), |a| self.expr_to_string(a))
                ),
                AggregateFunction::Sum => format!(
                    "SUM({})",
                    agg.argument
                        .as_ref()
                        .map_or("".to_string(), |a| self.expr_to_string(a))
                ),
                AggregateFunction::Avg => format!(
                    "AVG({})",
                    agg.argument
                        .as_ref()
                        .map_or("".to_string(), |a| self.expr_to_string(a))
                ),
                AggregateFunction::Min => format!(
                    "MIN({})",
                    agg.argument
                        .as_ref()
                        .map_or("".to_string(), |a| self.expr_to_string(a))
                ),
                AggregateFunction::Max => format!(
                    "MAX({})",
                    agg.argument
                        .as_ref()
                        .map_or("".to_string(), |a| self.expr_to_string(a))
                ),
                AggregateFunction::Collect => format!(
                    "COLLECT({})",
                    agg.argument
                        .as_ref()
                        .map_or("".to_string(), |a| self.expr_to_string(a))
                ),
            },
            Expression::FunctionCall(call) => {
                let args = call
                    .arguments
                    .iter()
                    .map(|a| self.expr_to_string(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{:?}({})", call.function, args)
            }
            _ => "...".to_string(),
        }
    }

    pub(crate) fn op_to_string(&self, op: &ComparisonOp) -> &'static str {
        match op {
            ComparisonOp::Equal => "=",
            ComparisonOp::NotEqual => "<>",
            ComparisonOp::LessThan => "<",
            ComparisonOp::LessEqual => "<=",
            ComparisonOp::GreaterThan => ">",
            ComparisonOp::GreaterEqual => ">=",
            ComparisonOp::Contains => "CONTAINS",
            ComparisonOp::StartsWith => "STARTS WITH",
            ComparisonOp::EndsWith => "ENDS WITH",
            ComparisonOp::In => "IN",
            ComparisonOp::IsNull => "IS NULL",
            ComparisonOp::IsNotNull => "IS NOT NULL",
        }
    }

    pub(crate) fn edge_pattern_to_string(&self, edge: &EdgePattern) -> String {
        let var = edge.variable.as_deref().unwrap_or("");
        let types = if edge.rel_types.is_empty() {
            String::new()
        } else {
            format!(":{}", edge.rel_types.join("|"))
        };

        let dir = match edge.direction {
            EdgeDirection::Outgoing => "->",
            EdgeDirection::Incoming => "<-",
            EdgeDirection::Both => "--",
        };

        format!("{}[{}{}]{}", dir, var, types, dir)
    }

    pub(crate) fn pattern_to_string(&self, pattern: &Pattern) -> String {
        if !pattern.elements.is_empty() {
            pattern
                .elements
                .iter()
                .map(|elem| match elem {
                    PatternElement::Node(n) => self.node_pattern_to_string(n),
                    PatternElement::Edge(e) => self.edge_pattern_to_string(e),
                })
                .collect::<Vec<_>>()
                .join("")
        } else {
            pattern
                .parts
                .iter()
                .map(|p| self.node_pattern_to_string(p))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    pub(crate) fn node_pattern_to_string(&self, node: &NodePattern) -> String {
        let var = node.variable.as_deref().unwrap_or("");
        let labels = if node.labels.is_empty() {
            String::new()
        } else {
            format!(":{}", node.labels.join(":"))
        };
        format!("({}{})", var, labels)
    }

    pub(crate) fn set_item_to_string(&self, item: &SetItem) -> String {
        match item {
            SetItem::Property {
                variable,
                property,
                value,
            } => {
                format!("{}.{} = {}", variable, property, self.expr_to_string(value))
            }
            SetItem::AllProperties {
                variable,
                properties,
            } => {
                format!("{} = {}", variable, self.expr_to_string(properties))
            }
            SetItem::MergeProperties {
                variable,
                properties,
            } => {
                format!("{} += {}", variable, self.expr_to_string(properties))
            }
            SetItem::AddLabel { variable, labels } => {
                format!("{}:{}", variable, labels.join(":"))
            }
        }
    }

    pub(crate) fn remove_item_to_string(&self, item: &RemoveItem) -> String {
        match item {
            RemoveItem::Property { variable, property } => {
                format!("{}.{}", variable, property)
            }
            RemoveItem::Label { variable, labels } => {
                format!("{}:{}", variable, labels.join(":"))
            }
        }
    }

    pub(crate) fn return_items_to_string(&self, items: &[ReturnItem]) -> String {
        items
            .iter()
            .map(|item| match item {
                ReturnItem::Star => "*".to_string(),
                ReturnItem::Expression { expr, alias } => {
                    let expr_str = self.expr_to_string(expr);
                    if let Some(a) = alias {
                        format!("{} AS {}", expr_str, a)
                    } else {
                        expr_str
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn order_items_to_string(&self, items: &[OrderItem]) -> String {
        items
            .iter()
            .map(|item| {
                let expr_str = self.expr_to_string(&item.expr);
                if item.ascending {
                    expr_str
                } else {
                    format!("{} DESC", expr_str)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}
