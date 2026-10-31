// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Reading Clause Planning
//!
//! Reading clause 및 패턴 매칭 실행 계획 생성

use super::QueryPlanner;
use crate::ast::*;
use zeta4j_graph::error::Result;

impl QueryPlanner {
    /// Reading 절 (MATCH) 실행 계획
    pub(crate) fn plan_reading(&self, reading: &ReadingClause) -> Result<PlanNode> {
        match reading {
            ReadingClause::Match {
                patterns,
                optional,
                where_clause,
            } => {
                let mut combined_node: Option<PlanNode> = None;
                let mut index_scan_used = false;

                for pattern in patterns {
                    let pattern_node = if let Some(where_expr) = where_clause {
                        if let Some(index_scan) = self.try_plan_index_scan(pattern, where_expr) {
                            index_scan_used = true;
                            index_scan
                        } else {
                            self.plan_pattern_scan(pattern)?
                        }
                    } else {
                        self.plan_pattern_scan(pattern)?
                    };

                    combined_node = Some(match combined_node {
                        None => pattern_node,
                        Some(prev) => {
                            PlanNode::new(PlanNodeType::CartesianProduct, "cross join".to_string())
                                .with_child(prev)
                                .with_child(pattern_node)
                        }
                    });
                }

                let mut scan_node = combined_node.unwrap_or_else(|| {
                    PlanNode::new(PlanNodeType::AllNodesScan, "empty pattern".to_string())
                });

                if *optional {
                    scan_node.node_type = match scan_node.node_type {
                        PlanNodeType::Expand => PlanNodeType::OptionalExpand,
                        _ => scan_node.node_type,
                    };
                }

                if !index_scan_used && let Some(where_expr) = where_clause {
                    let filter_desc = self.expr_to_string(where_expr);
                    let filter_node =
                        PlanNode::new(PlanNodeType::Filter, filter_desc).with_child(scan_node);
                    return Ok(filter_node);
                }
                Ok(scan_node)
            }
            ReadingClause::LoadCsv {
                url,
                variable,
                with_headers,
                ..
            } => {
                let desc = format!(
                    "LOAD CSV{} FROM '{}' AS {}",
                    if *with_headers { " WITH HEADERS" } else { "" },
                    url,
                    variable
                );
                Ok(PlanNode::new(PlanNodeType::LoadCsv, desc))
            }
        }
    }

    /// 패턴에서 스캔 노드 생성
    pub(crate) fn plan_pattern_scan(&self, pattern: &Pattern) -> Result<PlanNode> {
        if !pattern.elements.is_empty() {
            return self.plan_path_pattern(&pattern.elements);
        }

        if let Some(node_pattern) = pattern.parts.first() {
            return self.plan_node_pattern(node_pattern);
        }

        Ok(PlanNode::new(PlanNodeType::AllNodesScan, ""))
    }

    /// 경로 패턴 실행 계획 (노드-엣지-노드)
    pub(crate) fn plan_path_pattern(&self, elements: &[PatternElement]) -> Result<PlanNode> {
        let mut current: Option<PlanNode> = None;

        for element in elements {
            match element {
                PatternElement::Node(node_pattern) => {
                    let node_plan = self.plan_node_pattern(node_pattern)?;
                    if current.is_none() {
                        current = Some(node_plan);
                    }
                }
                PatternElement::Edge(edge_pattern) => {
                    let edge_desc = self.edge_pattern_to_string(edge_pattern);
                    let expand_node = PlanNode::new(PlanNodeType::Expand, edge_desc);

                    if let Some(child) = current.take() {
                        current = Some(expand_node.with_child(child));
                    } else {
                        current = Some(expand_node);
                    }
                }
            }
        }

        current.ok_or_else(|| zeta4j_graph::error::GraphError::Parse {
            message: "Empty pattern".to_string(),
        })
    }

    /// 노드 패턴 실행 계획
    pub(crate) fn plan_node_pattern(&self, node_pattern: &NodePattern) -> Result<PlanNode> {
        let var = node_pattern.variable.as_deref().unwrap_or("");

        if !node_pattern.labels.is_empty() {
            let labels = node_pattern.labels.join(":");
            let desc = if var.is_empty() {
                format!(":{}", labels)
            } else {
                format!("{}:{}", var, labels)
            };
            Ok(PlanNode::new(PlanNodeType::NodeByLabelScan, desc))
        } else {
            let desc = var.to_string();
            Ok(PlanNode::new(PlanNodeType::AllNodesScan, desc))
        }
    }

    /// Updating 절 (CREATE, DELETE, SET, MERGE) 실행 계획
    pub(crate) fn plan_updating(&self, updating: &UpdatingClause) -> Result<PlanNode> {
        match updating {
            UpdatingClause::Create(pattern) => {
                let desc = self.pattern_to_string(pattern);
                Ok(PlanNode::new(PlanNodeType::Create, desc))
            }
            UpdatingClause::Delete { variables, detach } => {
                let desc = if *detach {
                    format!("DETACH {}", variables.join(", "))
                } else {
                    variables.join(", ")
                };
                Ok(PlanNode::new(PlanNodeType::Delete, desc))
            }
            UpdatingClause::Set { items } => {
                let mut property_items = Vec::new();
                let mut all_props_items = Vec::new();
                let mut merge_props_items = Vec::new();
                let mut label_items = Vec::new();

                for item in items {
                    match item {
                        SetItem::Property { .. } => property_items.push(item),
                        SetItem::AllProperties { .. } => all_props_items.push(item),
                        SetItem::MergeProperties { .. } => merge_props_items.push(item),
                        SetItem::AddLabel { .. } => label_items.push(item),
                    }
                }

                let mut nodes = Vec::new();

                if !property_items.is_empty() {
                    let desc = property_items
                        .iter()
                        .map(|item| self.set_item_to_string(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    nodes.push(PlanNode::new(PlanNodeType::SetProperty, desc));
                }

                if !all_props_items.is_empty() {
                    let desc = all_props_items
                        .iter()
                        .map(|item| self.set_item_to_string(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    nodes.push(PlanNode::new(PlanNodeType::SetNodeProperties, desc));
                }

                if !merge_props_items.is_empty() {
                    let desc = merge_props_items
                        .iter()
                        .map(|item| self.set_item_to_string(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    nodes.push(PlanNode::new(PlanNodeType::SetNodePropertiesMerge, desc));
                }

                if !label_items.is_empty() {
                    let desc = label_items
                        .iter()
                        .map(|item| self.set_item_to_string(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    nodes.push(PlanNode::new(PlanNodeType::SetLabels, desc));
                }

                if nodes.is_empty() {
                    Ok(PlanNode::new(PlanNodeType::SetProperty, "".to_string()))
                } else if nodes.len() == 1 {
                    Ok(nodes.remove(0))
                } else {
                    let mut iter = nodes.into_iter().rev();
                    let Some(mut current) = iter.next() else {
                        return Ok(PlanNode::new(PlanNodeType::SetProperty, ""));
                    };
                    for parent in iter {
                        current = parent.with_child(current);
                    }
                    Ok(current)
                }
            }
            UpdatingClause::Merge { pattern, .. } => {
                let desc = self.pattern_to_string(pattern);
                Ok(PlanNode::new(PlanNodeType::Merge, desc))
            }
            UpdatingClause::Remove { items } => {
                let mut property_items = Vec::new();
                let mut label_items = Vec::new();

                for item in items {
                    match item {
                        RemoveItem::Property { .. } => property_items.push(item),
                        RemoveItem::Label { .. } => label_items.push(item),
                    }
                }

                let mut nodes = Vec::new();

                if !property_items.is_empty() {
                    let desc = property_items
                        .iter()
                        .map(|item| self.remove_item_to_string(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    nodes.push(PlanNode::new(PlanNodeType::RemoveProperty, desc));
                }

                if !label_items.is_empty() {
                    let desc = label_items
                        .iter()
                        .map(|item| self.remove_item_to_string(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    nodes.push(PlanNode::new(PlanNodeType::RemoveLabels, desc));
                }

                if nodes.is_empty() {
                    Ok(PlanNode::new(PlanNodeType::RemoveProperty, "".to_string()))
                } else if nodes.len() == 1 {
                    Ok(nodes.remove(0))
                } else {
                    let mut iter = nodes.into_iter().rev();
                    let Some(mut current) = iter.next() else {
                        return Ok(PlanNode::new(PlanNodeType::RemoveProperty, ""));
                    };
                    for parent in iter {
                        current = parent.with_child(current);
                    }
                    Ok(current)
                }
            }
            UpdatingClause::Foreach {
                variable,
                list,
                updates,
            } => {
                let list_str = format!("{:?}", list);
                let updates_count = updates.len();
                let desc = format!("({} IN {} | {} updates)", variable, list_str, updates_count);
                Ok(PlanNode::new(PlanNodeType::Foreach, desc))
            }
        }
    }

    /// WITH 절 실행 계획
    pub(crate) fn plan_with(&self, with_clause: &WithClause) -> Result<PlanNode> {
        let items_desc = self.return_items_to_string(&with_clause.items);
        let mut node = PlanNode::new(PlanNodeType::Projection, format!("WITH {}", items_desc));

        if with_clause.distinct {
            node = PlanNode::new(PlanNodeType::Distinct, "").with_child(node);
        }

        if let Some(order_by) = &with_clause.order_by {
            let order_desc = self.order_items_to_string(order_by);
            node = PlanNode::new(PlanNodeType::Sort, order_desc).with_child(node);
        }

        if let Some(skip) = with_clause.skip {
            node = PlanNode::new(PlanNodeType::Skip, skip.to_string()).with_child(node);
        }

        if let Some(limit) = with_clause.limit {
            node = PlanNode::new(PlanNodeType::Limit, limit.to_string()).with_child(node);
        }

        Ok(node)
    }

    /// UNWIND 절 실행 계획
    pub(crate) fn plan_unwind(&self, unwind: &UnwindClause) -> Result<PlanNode> {
        let desc = format!("{} AS {}", self.expr_to_string(&unwind.expr), unwind.alias);
        Ok(PlanNode::new(PlanNodeType::Unwind, desc))
    }

    /// RETURN 절 실행 계획
    pub(crate) fn plan_return(&self, return_clause: &ReturnClause) -> Result<PlanNode> {
        let items_desc = self.return_items_to_string(&return_clause.items);
        let mut node = PlanNode::new(PlanNodeType::ProduceResults, items_desc);

        if self.has_aggregate(&return_clause.items) {
            let agg_node = PlanNode::new(PlanNodeType::Aggregation, "");
            node = node.with_child(agg_node);
        }

        if return_clause.distinct {
            let inner = node.children.pop();
            let distinct = if let Some(child) = inner {
                PlanNode::new(PlanNodeType::Distinct, "").with_child(child)
            } else {
                PlanNode::new(PlanNodeType::Distinct, "")
            };
            node = node.with_child(distinct);
        }

        if let Some(order_by) = &return_clause.order_by {
            let order_desc = self.order_items_to_string(order_by);
            let inner = node.children.pop();
            let sort = if let Some(child) = inner {
                PlanNode::new(PlanNodeType::Sort, order_desc).with_child(child)
            } else {
                PlanNode::new(PlanNodeType::Sort, order_desc)
            };
            node = node.with_child(sort);
        }

        if let Some(skip) = return_clause.skip {
            let inner = node.children.pop();
            let skip_node = if let Some(child) = inner {
                PlanNode::new(PlanNodeType::Skip, skip.to_string()).with_child(child)
            } else {
                PlanNode::new(PlanNodeType::Skip, skip.to_string())
            };
            node = node.with_child(skip_node);
        }

        if let Some(limit) = return_clause.limit {
            let inner = node.children.pop();
            let limit_node = if let Some(child) = inner {
                PlanNode::new(PlanNodeType::Limit, limit.to_string()).with_child(child)
            } else {
                PlanNode::new(PlanNodeType::Limit, limit.to_string())
            };
            node = node.with_child(limit_node);
        }

        Ok(node)
    }
}
