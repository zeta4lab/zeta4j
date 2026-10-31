// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 쿼리 실행 계획 생성기
//!
//! AST를 분석하여 실행 계획(QueryPlan)을 생성합니다.
//!
//! # 개요
//!
//! [`QueryPlanner`]는 파싱된 AST를 받아 최적화된 실행 계획을 생성합니다.
//! 실행 계획은 트리 구조의 [`PlanNode`]들로 구성됩니다.
//!
//! # 최적화 규칙
//!
//! 플래너는 다음 최적화를 수행합니다:
//!
//! 1. **인덱스 스캔 선택**: WHERE 조건에 인덱스된 속성이 있으면 인덱스 스캔 사용
//! 2. **조건 푸시다운**: WHERE 조건을 가능한 일찍 적용
//! 3. **프로젝션 최적화**: 필요한 속성만 선택
//!
//! # 실행 계획 노드 타입
//!
//! - `NodeScan`: 모든 노드 스캔
//! - `LabelScan`: 특정 레이블 노드 스캔
//! - `IndexScan`: 인덱스를 사용한 스캔
//! - `Filter`: 조건 필터링
//! - `Projection`: 결과 프로젝션
//! - `Join`: 패턴 매칭 조인
//! - `Create`/`Update`/`Delete`: 쓰기 연산
//! - `Aggregate`: 집계 연산
//! - `Sort`/`Limit`/`Skip`: 결과 정렬 및 페이징
//! - `Union`/`UnionAll`: 쿼리 합집합
//!
//! # Modules
//!
//! - `statement`: Statement 계획 함수들
//! - `reading`: Reading clause 계획
//! - `index`: 인덱스 스캔 최적화
//! - `cardinality`: 카디널리티 추정 및 비용 계산
//! - `helpers`: 문자열 변환 헬퍼 함수들

mod cardinality;
mod helpers;
mod index;
mod reading;
mod statement;

use std::collections::HashSet;

use super::ast::*;
use zeta4j_graph::error::Result;

/// 인덱스 정보 - (라벨, 프로퍼티) 쌍의 집합
pub type IndexRegistry = HashSet<(String, String)>;

/// 쿼리 플래너 - AST에서 실행 계획 생성
pub struct QueryPlanner {
    /// 사용 가능한 인덱스 정보 (라벨, 프로퍼티)
    pub(crate) available_indexes: IndexRegistry,
}

impl QueryPlanner {
    /// 새 플래너 생성
    pub fn new() -> Self {
        Self {
            available_indexes: HashSet::new(),
        }
    }

    /// 인덱스 정보와 함께 플래너 생성
    pub fn with_indexes(indexes: IndexRegistry) -> Self {
        Self {
            available_indexes: indexes,
        }
    }

    /// 인덱스 추가
    pub fn add_index(&mut self, label: &str, property: &str) {
        self.available_indexes
            .insert((label.to_string(), property.to_string()));
    }

    /// 인덱스 존재 여부 확인
    pub fn has_index(&self, label: &str, property: &str) -> bool {
        self.available_indexes
            .contains(&(label.to_string(), property.to_string()))
    }

    /// 쿼리에서 실행 계획 생성
    pub fn plan(&self, query: &Query) -> Result<QueryPlan> {
        let root = self.plan_query_body(&query.body)?;
        Ok(QueryPlan::new(root))
    }

    /// 쿼리 본체의 실행 계획 생성
    fn plan_query_body(&self, body: &QueryBody) -> Result<PlanNode> {
        match body {
            QueryBody::Single(single) => self.plan_single_query(single),
            QueryBody::Union { left, right, all } => {
                let left_plan = self.plan_query_body(left)?;
                let right_plan = self.plan_query_body(right)?;
                let node_type = if *all {
                    PlanNodeType::UnionAll
                } else {
                    PlanNodeType::Union
                };
                Ok(PlanNode::new(node_type, "")
                    .with_child(left_plan)
                    .with_child(right_plan))
            }
        }
    }

    /// 단일 쿼리의 실행 계획 생성
    fn plan_single_query(&self, query: &SingleQuery) -> Result<PlanNode> {
        let mut children: Vec<PlanNode> = Vec::new();

        for statement in &query.statements {
            let plan = self.plan_statement(statement)?;
            children.push(plan);
        }

        if children.is_empty() {
            return Ok(PlanNode::new(PlanNodeType::ProduceResults, ""));
        }

        let Some(mut result) = children.pop() else {
            return Ok(PlanNode::new(PlanNodeType::ProduceResults, ""));
        };
        while let Some(child) = children.pop() {
            result = self.insert_child_under_modifiers(result, child);
        }

        Ok(result)
    }

    /// 모디파이어 노드(Limit, Skip, Sort 등) 아래에 자식을 삽입
    fn insert_child_under_modifiers(&self, mut parent: PlanNode, child: PlanNode) -> PlanNode {
        let is_modifier = |t: &PlanNodeType| {
            matches!(
                t,
                PlanNodeType::Limit
                    | PlanNodeType::Skip
                    | PlanNodeType::Sort
                    | PlanNodeType::Distinct
                    | PlanNodeType::Aggregation
            )
        };

        if let Some(modifier_idx) = parent
            .children
            .iter()
            .position(|c| is_modifier(&c.node_type) && c.children.is_empty())
        {
            parent.children[modifier_idx] = parent.children[modifier_idx].clone().with_child(child);
            parent
        } else {
            parent.with_child(child)
        }
    }
}

impl Default for QueryPlanner {
    fn default() -> Self {
        Self::new()
    }
}
