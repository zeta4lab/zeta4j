// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 쿼리 실행 계획
//!
//! 전체 쿼리의 실행 계획 트리

use super::plan_node::PlanNode;

/// 실행 계획
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPlan {
    /// 루트 노드
    pub root: PlanNode,
}

impl QueryPlan {
    /// 새 실행 계획 생성
    pub fn new(root: PlanNode) -> Self {
        Self { root }
    }

    /// 트리 형식으로 실행 계획 출력
    pub fn to_tree_string(&self) -> String {
        let mut result = String::new();
        self.format_node(&self.root, &mut result, "", true);
        result
    }

    fn format_node(&self, node: &PlanNode, result: &mut String, prefix: &str, is_last: bool) {
        let connector = if prefix.is_empty() {
            ""
        } else if is_last {
            "└── "
        } else {
            "├── "
        };

        let desc = if node.description.is_empty() {
            String::new()
        } else {
            format!(" ({})", node.description)
        };

        let rows = if node.estimated_rows > 0 {
            format!(" [~{} rows]", node.estimated_rows)
        } else {
            String::new()
        };

        result.push_str(&format!(
            "{}{}{}{}{}\n",
            prefix, connector, node.node_type, desc, rows
        ));

        let child_prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{}{}   ", prefix, if is_last { " " } else { "│" })
        };

        for (i, child) in node.children.iter().enumerate() {
            let is_last_child = i == node.children.len() - 1;
            self.format_node(child, result, &child_prefix, is_last_child);
        }
    }

    /// 상세 EXPLAIN 출력 (비용 정보 포함)
    ///
    /// Neo4j 스타일의 상세 실행 계획을 출력합니다.
    /// 각 연산자의 예상 행 수와 비용 정보를 포함합니다.
    pub fn to_explain_detail(&self, total_cost: f64) -> String {
        let mut result = String::new();

        // 헤더
        result.push_str("+-------------------------------------------+\n");
        result.push_str("|               EXECUTION PLAN              |\n");
        result.push_str("+-------------------------------------------+\n");
        result.push_str(&format!("| Total Cost: {:<28.4} |\n", total_cost));
        result.push_str("+-------------------------------------------+\n\n");

        // 트리 출력
        self.format_node_detail(&self.root, &mut result, "", true);

        // 통계 요약
        result.push_str("\n+-------------------------------------------+\n");
        result.push_str("|                 SUMMARY                   |\n");
        result.push_str("+-------------------------------------------+\n");

        let (node_count, total_rows) = self.count_nodes_and_rows(&self.root);
        result.push_str(&format!("| Operators: {:<30} |\n", node_count));
        result.push_str(&format!("| Estimated Rows: {:<25} |\n", total_rows));
        result.push_str(&format!("| Total Cost: {:<28.4} |\n", total_cost));
        result.push_str("+-------------------------------------------+\n");

        result
    }

    fn format_node_detail(
        &self,
        node: &PlanNode,
        result: &mut String,
        prefix: &str,
        is_last: bool,
    ) {
        let connector = if prefix.is_empty() {
            ""
        } else if is_last {
            "└── "
        } else {
            "├── "
        };

        // 노드 타입 (굵은 표시용 대문자)
        let node_type_str = format!("{}", node.node_type);

        // 설명
        let desc = if node.description.is_empty() {
            String::new()
        } else {
            format!(" ({})", node.description)
        };

        // 행 수 정보
        let rows_info = if node.estimated_rows > 0 {
            format!(" ~{} rows", node.estimated_rows)
        } else {
            String::new()
        };

        result.push_str(&format!(
            "{}{}{}{}\n",
            prefix, connector, node_type_str, desc
        ));

        // 상세 정보 (들여쓰기)
        if !rows_info.is_empty() {
            let detail_prefix = if prefix.is_empty() {
                "    ".to_string()
            } else {
                format!("{}{}   ", prefix, if is_last { " " } else { "│" })
            };
            result.push_str(&format!(
                "{}├── Estimated Rows:{}\n",
                detail_prefix, rows_info
            ));

            // 변수 정보
            if !node.variables.is_empty() {
                result.push_str(&format!(
                    "{}└── Variables: {}\n",
                    detail_prefix,
                    node.variables.join(", ")
                ));
            }
        }

        let child_prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{}{}   ", prefix, if is_last { " " } else { "│" })
        };

        for (i, child) in node.children.iter().enumerate() {
            let is_last_child = i == node.children.len() - 1;
            self.format_node_detail(child, result, &child_prefix, is_last_child);
        }
    }

    fn count_nodes_and_rows(&self, node: &PlanNode) -> (usize, u64) {
        let mut count = 1usize;
        let mut rows = node.estimated_rows;

        for child in &node.children {
            let (child_count, child_rows) = self.count_nodes_and_rows(child);
            count += child_count;
            rows = rows.max(child_rows);
        }

        (count, rows)
    }
}
