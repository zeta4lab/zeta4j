// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 필터 푸시다운 최적화
//!
//! WHERE 절의 조건을 가능한 일찍 적용하여 Expand 전에 불필요한 노드를 걸러냅니다.
//!
//! # 최적화 전략
//!
//! 1. **조건 분류**: 각 조건이 참조하는 변수 분석
//! 2. **푸시다운 결정**: 단일 변수 조건은 해당 노드 스캔 직후 적용
//! 3. **조인 조건 보존**: 다중 변수 조건은 Expand 후 적용

use crate::ast::{Expression, VectorExpression};
use std::collections::{HashMap, HashSet};

/// 필터 조건 분석기
#[derive(Debug, Default)]
pub struct FilterAnalyzer;

impl FilterAnalyzer {
    /// 새 분석기 생성
    pub fn new() -> Self {
        Self
    }

    /// 표현식에서 참조하는 변수 추출
    pub fn extract_variables(expr: &Expression) -> HashSet<String> {
        let mut vars = HashSet::new();
        Self::collect_variables(expr, &mut vars);
        vars
    }

    /// 재귀적으로 변수 수집
    fn collect_variables(expr: &Expression, vars: &mut HashSet<String>) {
        match expr {
            Expression::Variable(name) => {
                vars.insert(name.clone());
            }
            Expression::PropertyAccess(var, _prop) => {
                vars.insert(var.clone());
            }
            Expression::Comparison { left, right, .. } => {
                Self::collect_variables(left, vars);
                Self::collect_variables(right, vars);
            }
            Expression::Logical { left, right, .. } => {
                if let Some(l) = left {
                    Self::collect_variables(l, vars);
                }
                Self::collect_variables(right, vars);
            }
            Expression::IsNull { expr, .. } => {
                Self::collect_variables(expr, vars);
            }
            Expression::List(items) => {
                for item in items {
                    Self::collect_variables(item, vars);
                }
            }
            Expression::Map(pairs) => {
                for (_, v) in pairs {
                    Self::collect_variables(v, vars);
                }
            }
            Expression::FunctionCall(call) => {
                for arg in &call.arguments {
                    Self::collect_variables(arg, vars);
                }
            }
            Expression::Case(case_expr) => {
                if let Some(op) = &case_expr.operand {
                    Self::collect_variables(op, vars);
                }
                for when_clause in &case_expr.when_clauses {
                    Self::collect_variables(&when_clause.condition, vars);
                    Self::collect_variables(&when_clause.result, vars);
                }
                if let Some(else_expr) = &case_expr.else_result {
                    Self::collect_variables(else_expr, vars);
                }
            }
            Expression::Aggregate(agg) => {
                if let Some(arg) = &agg.argument {
                    Self::collect_variables(arg, vars);
                }
            }
            Expression::Exists(exists) => {
                // EXISTS 내부는 별도 스코프이므로 외부 변수만 수집
                if let Some(where_expr) = &exists.where_clause {
                    Self::collect_variables(where_expr, vars);
                }
            }
            Expression::Literal(_) => {
                // 변수 없음
            }
            Expression::Parameter(_) => {
                // 파라미터는 변수가 아님
            }
            Expression::Arithmetic { left, right, .. } => {
                Self::collect_variables(left, vars);
                Self::collect_variables(right, vars);
            }
            Expression::ListComprehension(comp) => {
                // 리스트 표현식에서 변수 수집
                Self::collect_variables(&comp.list_expr, vars);
                if let Some(filter) = &comp.filter {
                    Self::collect_variables(filter, vars);
                }
                if let Some(map_expr) = &comp.map_expr {
                    Self::collect_variables(map_expr, vars);
                }
                // 반복 변수는 내부 스코프이므로 제외
                vars.remove(&comp.variable);
            }
            Expression::Subscript { expr, index } => {
                Self::collect_variables(expr, vars);
                Self::collect_variables(index, vars);
            }
            Expression::SubscriptRange { expr, start, end } => {
                Self::collect_variables(expr, vars);
                if let Some(s) = start {
                    Self::collect_variables(s, vars);
                }
                if let Some(e) = end {
                    Self::collect_variables(e, vars);
                }
            }
            // Phase 3: 벡터 리터럴
            Expression::Vector(vec_expr) => {
                match vec_expr {
                    VectorExpression::Literal(elements) => {
                        for elem in elements {
                            Self::collect_variables(elem, vars);
                        }
                    }
                    VectorExpression::Parameter(_) => {
                        // 파라미터는 변수가 아님
                    }
                }
            }
        }
    }

    /// 조건이 단일 변수만 참조하는지 확인
    pub fn is_single_var_condition(expr: &Expression) -> bool {
        Self::extract_variables(expr).len() == 1
    }

    /// 조건이 푸시다운 가능한지 확인
    /// 집계 함수나 서브쿼리가 포함되면 불가능
    pub fn is_pushable(expr: &Expression) -> bool {
        match expr {
            Expression::Aggregate(_) => false,
            Expression::Exists(_) => false,
            Expression::Comparison { left, right, .. } => {
                Self::is_pushable(left) && Self::is_pushable(right)
            }
            Expression::Logical { left, right, .. } => {
                left.as_ref().is_none_or(|l| Self::is_pushable(l)) && Self::is_pushable(right)
            }
            Expression::IsNull { expr, .. } => Self::is_pushable(expr),
            Expression::FunctionCall(call) => call.arguments.iter().all(Self::is_pushable),
            Expression::Case(case_expr) => {
                case_expr
                    .operand
                    .as_ref()
                    .is_none_or(|o| Self::is_pushable(o))
                    && case_expr
                        .when_clauses
                        .iter()
                        .all(|w| Self::is_pushable(&w.condition) && Self::is_pushable(&w.result))
                    && case_expr
                        .else_result
                        .as_ref()
                        .is_none_or(|e| Self::is_pushable(e))
            }
            _ => true,
        }
    }

    /// AND로 연결된 조건들을 분리
    pub fn split_and_conditions(expr: &Expression) -> Vec<Expression> {
        let mut conditions = Vec::new();
        Self::collect_and_conditions(expr, &mut conditions);
        conditions
    }

    /// 재귀적으로 AND 조건 수집
    fn collect_and_conditions(expr: &Expression, conditions: &mut Vec<Expression>) {
        match expr {
            Expression::Logical {
                left: Some(left),
                op: crate::ast::LogicalOp::And,
                right,
            } => {
                Self::collect_and_conditions(left, conditions);
                Self::collect_and_conditions(right, conditions);
            }
            _ => {
                conditions.push(expr.clone());
            }
        }
    }

    /// 조건들을 변수별로 분류
    pub fn classify_by_variable(
        conditions: &[Expression],
    ) -> (HashMap<String, Vec<Expression>>, Vec<Expression>) {
        let mut single_var: HashMap<String, Vec<Expression>> = HashMap::new();
        let mut multi_var: Vec<Expression> = Vec::new();

        for cond in conditions {
            let vars = Self::extract_variables(cond);

            if vars.len() == 1
                && Self::is_pushable(cond)
                && let Some(var) = vars.into_iter().next()
            {
                single_var.entry(var).or_default().push(cond.clone());
            } else {
                multi_var.push(cond.clone());
            }
        }

        (single_var, multi_var)
    }
}

/// 필터 푸시다운 계획
#[derive(Debug, Clone, Default)]
pub struct FilterPlan {
    /// 노드별 푸시다운된 필터 (변수명 → 조건들)
    pub node_filters: HashMap<String, Vec<Expression>>,
    /// Expand 후 적용할 필터 (다중 변수 조건)
    pub post_expand_filters: Vec<Expression>,
    /// 필터로 인한 예상 선택도 (변수명 → 선택도)
    pub selectivity: HashMap<String, f64>,
}

impl FilterPlan {
    /// 빈 필터 계획
    pub fn empty() -> Self {
        Self::default()
    }

    /// 필터 계획 생성
    pub fn new(
        node_filters: HashMap<String, Vec<Expression>>,
        post_expand_filters: Vec<Expression>,
    ) -> Self {
        let mut selectivity = HashMap::new();

        // 각 변수별 선택도 계산 (조건 수에 따라 추정)
        for (var, filters) in &node_filters {
            // 조건당 0.1 선택도 가정
            let sel = 0.1_f64.powi(filters.len() as i32);
            selectivity.insert(var.clone(), sel);
        }

        Self {
            node_filters,
            post_expand_filters,
            selectivity,
        }
    }

    /// 특정 변수에 적용할 필터 조건들
    pub fn get_filters_for(&self, variable: &str) -> &[Expression] {
        self.node_filters
            .get(variable)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// 특정 변수의 필터 선택도
    pub fn get_selectivity(&self, variable: &str) -> f64 {
        *self.selectivity.get(variable).unwrap_or(&1.0)
    }

    /// 필터가 있는지 확인
    pub fn has_filters(&self) -> bool {
        !self.node_filters.is_empty() || !self.post_expand_filters.is_empty()
    }

    /// 전체 조건 수
    pub fn total_conditions(&self) -> usize {
        self.node_filters.values().map(|v| v.len()).sum::<usize>() + self.post_expand_filters.len()
    }
}

/// 필터 푸시다운 최적화기
#[derive(Debug, Default)]
pub struct FilterPushdown;

impl FilterPushdown {
    /// 새 최적화기 생성
    pub fn new() -> Self {
        Self
    }

    /// WHERE 절을 분석하여 필터 푸시다운 계획 생성
    pub fn optimize(&self, where_clause: &Expression) -> FilterPlan {
        // 1. AND 조건 분리
        let conditions = FilterAnalyzer::split_and_conditions(where_clause);

        // 2. 변수별 분류
        let (single_var, multi_var) = FilterAnalyzer::classify_by_variable(&conditions);

        // 3. 필터 계획 생성
        FilterPlan::new(single_var, multi_var)
    }

    /// 선택적으로 WHERE 절 최적화 (None이면 빈 계획)
    pub fn optimize_optional(&self, where_clause: Option<&Expression>) -> FilterPlan {
        match where_clause {
            Some(expr) => self.optimize(expr),
            None => FilterPlan::empty(),
        }
    }
}
