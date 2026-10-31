// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 실행 계획 노드
//!
//! 쿼리 실행 계획의 개별 노드

use super::node_type::PlanNodeType;

/// 실행 계획 노드
#[derive(Debug, Clone, PartialEq)]
pub struct PlanNode {
    /// 노드 타입
    pub node_type: PlanNodeType,
    /// 연산 설명 (예: ":Person", "n.age > 30")
    pub description: String,
    /// 예상 행 수
    pub estimated_rows: u64,
    /// 하위 노드
    pub children: Vec<PlanNode>,
    /// 이 단계에서 사용 가능한 변수
    pub variables: Vec<String>,
}

impl PlanNode {
    /// 새 실행 계획 노드 생성
    pub fn new(node_type: PlanNodeType, description: impl Into<String>) -> Self {
        Self {
            node_type,
            description: description.into(),
            estimated_rows: 0,
            children: vec![],
            variables: vec![],
        }
    }

    /// 하위 노드 추가
    pub fn with_child(mut self, child: PlanNode) -> Self {
        self.children.push(child);
        self
    }

    /// 예상 행 수 설정
    pub fn with_rows(mut self, rows: u64) -> Self {
        self.estimated_rows = rows;
        self
    }

    /// 변수 설정
    pub fn with_variables(mut self, vars: Vec<String>) -> Self {
        self.variables = vars;
        self
    }
}
