// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! UNION 쿼리 최적화 모듈
//!
//! UNION 쿼리의 실행 계획을 최적화합니다.
//!
//! # 최적화 전략
//!
//! 1. **브랜치 정렬 (Branch Reordering)**
//!    - 예상 카디널리티가 작은 브랜치를 먼저 실행
//!    - 조기 종료 가능성 향상
//!
//! 2. **병렬 실행 (Parallel Execution)**
//!    - 독립적인 브랜치는 병렬로 실행
//!    - tokio::join!을 활용한 동시 실행
//!
//! 3. **공통 하위 표현식 추출 (Common Subexpression Extraction)**
//!    - 동일한 MATCH 패턴을 공유하는 브랜치 식별
//!    - 공유 스캔을 통한 중복 I/O 제거
//!
//! # Example
//!
//! ```ignore
//! // 최적화 전:
//! // MATCH (a:Person) WHERE a.age > 30 RETURN a.name
//! // UNION
//! // MATCH (a:Person) WHERE a.age < 20 RETURN a.name
//!
//! // 최적화 후:
//! // 1. Person 레이블 스캔 1회 (공유)
//! // 2. 필터 분기 (age > 30 OR age < 20)
//! // 3. 결과 병합
//! ```

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::cost_model::CostModel;
use crate::ast::*;

/// UNION 최적화 결과
#[derive(Debug, Clone)]
pub struct UnionOptimizationResult {
    /// 최적화된 브랜치 순서 (인덱스)
    pub branch_order: Vec<usize>,
    /// 병렬 실행 가능 여부
    pub can_parallelize: bool,
    /// 공통 하위 표현식 그룹
    pub common_subexpressions: Vec<CommonSubexpressionGroup>,
    /// 예상 총 비용
    pub estimated_cost: f64,
    /// 최적화 적용 내역
    pub optimizations_applied: Vec<String>,
}

/// 공통 하위 표현식 그룹
#[derive(Debug, Clone)]
pub struct CommonSubexpressionGroup {
    /// 그룹 ID
    pub group_id: usize,
    /// 공유되는 패턴 (정규화된 문자열)
    pub shared_pattern: String,
    /// 이 패턴을 공유하는 브랜치 인덱스들
    pub branch_indices: Vec<usize>,
    /// 예상 절감 비용
    pub estimated_savings: f64,
}

/// 브랜치 정보
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// 브랜치 인덱스
    pub index: usize,
    /// 예상 카디널리티
    pub estimated_rows: u64,
    /// 예상 비용
    pub estimated_cost: f64,
    /// MATCH 패턴 해시 (공통 표현식 감지용)
    pub pattern_hash: u64,
    /// 정규화된 패턴 문자열
    pub normalized_pattern: String,
    /// 필터 조건 (WHERE)
    pub filter_conditions: Vec<String>,
    /// 읽기 전용 여부 (쓰기 연산 없음)
    pub is_read_only: bool,
}

/// UNION 최적화기
pub struct UnionOptimizer {
    /// 비용 모델
    cost_model: CostModel,
    /// 병렬 실행 임계값 (브랜치 수가 이 이상이면 병렬화)
    parallel_threshold: usize,
    /// 공통 표현식 추출 활성화
    enable_cse: bool,
}

impl Default for UnionOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl UnionOptimizer {
    /// 새 UNION 최적화기 생성
    pub fn new() -> Self {
        Self {
            cost_model: CostModel::new(),
            parallel_threshold: 2,
            enable_cse: true,
        }
    }

    /// 커스텀 설정으로 최적화기 생성
    pub fn with_config(parallel_threshold: usize, enable_cse: bool) -> Self {
        Self {
            cost_model: CostModel::new(),
            parallel_threshold,
            enable_cse,
        }
    }

    /// UNION 쿼리 최적화
    ///
    /// QueryBody가 Union인 경우 최적화를 적용합니다.
    pub fn optimize(&self, body: &QueryBody) -> UnionOptimizationResult {
        // 브랜치 정보 수집
        let branches = self.collect_branches(body);

        if branches.is_empty() {
            return UnionOptimizationResult {
                branch_order: vec![],
                can_parallelize: false,
                common_subexpressions: vec![],
                estimated_cost: 0.0,
                optimizations_applied: vec![],
            };
        }

        let mut optimizations_applied = Vec::new();

        // 1. 비용 기반 브랜치 정렬
        let branch_order = self.order_branches_by_cost(&branches);
        if branch_order != (0..branches.len()).collect::<Vec<_>>() {
            optimizations_applied.push("branch_reordering".to_string());
        }

        // 2. 병렬 실행 가능 여부 판단
        let can_parallelize = self.can_parallelize(&branches);
        if can_parallelize {
            optimizations_applied.push("parallel_execution".to_string());
        }

        // 3. 공통 하위 표현식 추출
        let common_subexpressions = if self.enable_cse {
            let cse = self.extract_common_subexpressions(&branches);
            if !cse.is_empty() {
                optimizations_applied.push("common_subexpression_extraction".to_string());
            }
            cse
        } else {
            vec![]
        };

        // 4. 총 비용 추정
        let estimated_cost = self.estimate_total_cost(&branches, &common_subexpressions);

        UnionOptimizationResult {
            branch_order,
            can_parallelize,
            common_subexpressions,
            estimated_cost,
            optimizations_applied,
        }
    }

    /// QueryBody에서 모든 브랜치 수집
    fn collect_branches(&self, body: &QueryBody) -> Vec<BranchInfo> {
        let mut branches = Vec::new();
        self.collect_branches_recursive(body, &mut branches, 0);
        branches
    }

    /// 재귀적으로 브랜치 수집
    fn collect_branches_recursive(
        &self,
        body: &QueryBody,
        branches: &mut Vec<BranchInfo>,
        _depth: usize,
    ) {
        match body {
            QueryBody::Single(single) => {
                let info = self.analyze_single_query(single, branches.len());
                branches.push(info);
            }
            QueryBody::Union { left, right, .. } => {
                self.collect_branches_recursive(left, branches, _depth + 1);
                self.collect_branches_recursive(right, branches, _depth + 1);
            }
        }
    }

    /// 단일 쿼리 분석
    fn analyze_single_query(&self, query: &SingleQuery, index: usize) -> BranchInfo {
        let mut pattern_parts = Vec::new();
        let mut filter_conditions = Vec::new();
        let mut is_read_only = true;
        let mut estimated_rows: u64 = 100; // 기본값

        for stmt in &query.statements {
            match stmt {
                Statement::Reading(reading) => {
                    let (pattern, filters) = self.extract_reading_info(reading);
                    pattern_parts.extend(pattern);
                    filter_conditions.extend(filters);
                }
                Statement::Updating(_) => {
                    is_read_only = false;
                }
                _ => {}
            }
        }

        let normalized_pattern = pattern_parts.join(";");
        let pattern_hash = self.hash_pattern(&normalized_pattern);

        // 필터 조건에 따른 카디널리티 조정
        if !filter_conditions.is_empty() {
            estimated_rows = (estimated_rows as f64 * 0.3) as u64; // 필터링 추정
        }

        let estimated_cost = self.cost_model.all_nodes_scan_cost(estimated_rows)
            + filter_conditions.len() as f64 * self.cost_model.cpu_tuple_cost;

        BranchInfo {
            index,
            estimated_rows,
            estimated_cost,
            pattern_hash,
            normalized_pattern,
            filter_conditions,
            is_read_only,
        }
    }

    /// Reading 절에서 패턴과 필터 정보 추출
    fn extract_reading_info(&self, reading: &ReadingClause) -> (Vec<String>, Vec<String>) {
        let mut patterns_out = Vec::new();
        let mut filters = Vec::new();

        match reading {
            ReadingClause::Match {
                patterns,
                optional: _,
                where_clause,
            } => {
                // 패턴 추출
                for pattern in patterns {
                    let pattern_str = self.pattern_to_string(pattern);
                    patterns_out.push(pattern_str);
                }

                // WHERE 조건 추출
                if let Some(condition) = where_clause {
                    let condition_str = self.expression_to_string(condition);
                    filters.push(condition_str);
                }
            }
            ReadingClause::LoadCsv { url, variable, .. } => {
                patterns_out.push(format!("LOAD CSV FROM '{}' AS {}", url, variable));
            }
        }

        (patterns_out, filters)
    }

    /// 패턴을 문자열로 변환
    fn pattern_to_string(&self, pattern: &Pattern) -> String {
        let mut parts = Vec::new();

        for element in &pattern.elements {
            match element {
                PatternElement::Node(node) => {
                    let label = node.labels.first().map(|l| l.as_str()).unwrap_or("*");
                    parts.push(format!("(:{label})"));
                }
                PatternElement::Edge(edge) => {
                    let rel_type = edge.rel_types.first().map(|t| t.as_str()).unwrap_or("*");
                    let dir = match edge.direction {
                        EdgeDirection::Outgoing => "->",
                        EdgeDirection::Incoming => "<-",
                        EdgeDirection::Both => "--",
                    };
                    parts.push(format!("-[:{rel_type}]{dir}"));
                }
            }
        }

        parts.join("")
    }

    /// Expression을 문자열로 변환 (단순화)
    fn expression_to_string(&self, expr: &Expression) -> String {
        match expr {
            Expression::Variable(name) => name.clone(),
            Expression::Literal(value) => format!("{:?}", value),
            Expression::PropertyAccess(var, prop) => format!("{}.{}", var, prop),
            Expression::Comparison { left, op, right } => {
                format!(
                    "{} {:?} {}",
                    self.expression_to_string(left),
                    op,
                    self.expression_to_string(right)
                )
            }
            Expression::Logical { left, op, right } => {
                if let Some(l) = left {
                    format!(
                        "{} {:?} {}",
                        self.expression_to_string(l),
                        op,
                        self.expression_to_string(right)
                    )
                } else {
                    format!("{:?} {}", op, self.expression_to_string(right))
                }
            }
            Expression::Arithmetic { left, op, right } => {
                format!(
                    "{} {:?} {}",
                    self.expression_to_string(left),
                    op,
                    self.expression_to_string(right)
                )
            }
            _ => "expr".to_string(),
        }
    }

    /// 패턴 해시 계산
    fn hash_pattern(&self, pattern: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        pattern.hash(&mut hasher);
        hasher.finish()
    }

    /// 비용 기반 브랜치 정렬
    fn order_branches_by_cost(&self, branches: &[BranchInfo]) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..branches.len()).collect();

        // 비용이 낮은 순서로 정렬
        indices.sort_by(|&a, &b| {
            branches[a]
                .estimated_cost
                .partial_cmp(&branches[b].estimated_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        indices
    }

    /// 병렬 실행 가능 여부 판단
    fn can_parallelize(&self, branches: &[BranchInfo]) -> bool {
        // 조건:
        // 1. 브랜치 수가 임계값 이상
        // 2. 모든 브랜치가 읽기 전용
        branches.len() >= self.parallel_threshold && branches.iter().all(|b| b.is_read_only)
    }

    /// 공통 하위 표현식 추출
    fn extract_common_subexpressions(
        &self,
        branches: &[BranchInfo],
    ) -> Vec<CommonSubexpressionGroup> {
        // 패턴 해시별로 브랜치 그룹화
        let mut pattern_groups: HashMap<u64, Vec<usize>> = HashMap::new();

        for branch in branches {
            pattern_groups
                .entry(branch.pattern_hash)
                .or_default()
                .push(branch.index);
        }

        // 2개 이상의 브랜치가 공유하는 패턴만 추출
        let mut groups = Vec::new();
        let mut group_id = 0;

        for (hash, indices) in pattern_groups {
            if indices.len() >= 2 {
                // 공유 패턴 찾기
                let shared_pattern = branches
                    .iter()
                    .find(|b| b.pattern_hash == hash)
                    .map(|b| b.normalized_pattern.clone())
                    .unwrap_or_default();

                // 예상 절감 비용 = (공유 브랜치 수 - 1) * 스캔 비용
                let avg_cost: f64 = indices
                    .iter()
                    .map(|&i| branches[i].estimated_cost)
                    .sum::<f64>()
                    / indices.len() as f64;

                let estimated_savings = avg_cost * (indices.len() - 1) as f64 * 0.7;

                groups.push(CommonSubexpressionGroup {
                    group_id,
                    shared_pattern,
                    branch_indices: indices,
                    estimated_savings,
                });

                group_id += 1;
            }
        }

        groups
    }

    /// 총 비용 추정
    fn estimate_total_cost(
        &self,
        branches: &[BranchInfo],
        common_subexpressions: &[CommonSubexpressionGroup],
    ) -> f64 {
        let base_cost: f64 = branches.iter().map(|b| b.estimated_cost).sum();

        // 공통 표현식 절감
        let savings: f64 = common_subexpressions
            .iter()
            .map(|g| g.estimated_savings)
            .sum();

        (base_cost - savings).max(0.0)
    }

    /// 최적화 결과를 적용하여 실행 순서 반환
    pub fn get_execution_order(&self, result: &UnionOptimizationResult) -> Vec<usize> {
        result.branch_order.clone()
    }

    /// 병렬 실행 그룹 생성
    ///
    /// 공통 표현식이 없는 브랜치들을 병렬 그룹으로 묶음
    pub fn create_parallel_groups(
        &self,
        result: &UnionOptimizationResult,
        total_branches: usize,
    ) -> Vec<Vec<usize>> {
        if !result.can_parallelize {
            // 병렬화 불가 시 순차 실행
            return result.branch_order.iter().map(|&i| vec![i]).collect();
        }

        // 공통 표현식에 속하는 브랜치들
        let mut grouped_branches: Vec<usize> = result
            .common_subexpressions
            .iter()
            .flat_map(|g| g.branch_indices.iter().copied())
            .collect();
        grouped_branches.sort();
        grouped_branches.dedup();

        // 그룹에 속하지 않는 브랜치들은 병렬 실행 가능
        let independent_branches: Vec<usize> = (0..total_branches)
            .filter(|i| !grouped_branches.contains(i))
            .collect();

        let mut groups = Vec::new();

        // 공통 표현식 그룹 (순차 실행)
        for cse_group in &result.common_subexpressions {
            groups.push(cse_group.branch_indices.clone());
        }

        // 독립 브랜치들 (병렬 실행)
        if !independent_branches.is_empty() {
            groups.push(independent_branches);
        }

        groups
    }
}

/// UNION 실행 계획
#[derive(Debug, Clone)]
pub struct UnionExecutionPlan {
    /// 실행 그룹 (각 그룹 내는 순차, 그룹 간은 병렬 가능)
    pub execution_groups: Vec<ExecutionGroup>,
    /// UNION ALL 여부
    pub is_union_all: bool,
    /// 최적화 결과
    pub optimization_result: UnionOptimizationResult,
}

/// 실행 그룹
#[derive(Debug, Clone)]
pub struct ExecutionGroup {
    /// 그룹 ID
    pub group_id: usize,
    /// 브랜치 인덱스 목록
    pub branch_indices: Vec<usize>,
    /// 공유 패턴 (있는 경우)
    pub shared_pattern: Option<String>,
    /// 병렬 실행 가능 여부
    pub can_run_parallel: bool,
}

impl UnionExecutionPlan {
    /// 새 실행 계획 생성
    pub fn from_optimization(
        result: UnionOptimizationResult,
        is_union_all: bool,
        total_branches: usize,
    ) -> Self {
        let optimizer = UnionOptimizer::new();
        let parallel_groups = optimizer.create_parallel_groups(&result, total_branches);

        let mut execution_groups = Vec::new();

        for (group_id, branch_indices) in parallel_groups.into_iter().enumerate() {
            // 공통 표현식 그룹인지 확인
            let shared_pattern = result
                .common_subexpressions
                .iter()
                .find(|g| {
                    g.branch_indices.len() == branch_indices.len()
                        && g.branch_indices.iter().all(|i| branch_indices.contains(i))
                })
                .map(|g| g.shared_pattern.clone());

            let can_run_parallel = shared_pattern.is_none() && branch_indices.len() > 1;

            execution_groups.push(ExecutionGroup {
                group_id,
                branch_indices,
                shared_pattern,
                can_run_parallel,
            });
        }

        Self {
            execution_groups,
            is_union_all,
            optimization_result: result,
        }
    }

    /// 총 브랜치 수
    pub fn total_branches(&self) -> usize {
        self.execution_groups
            .iter()
            .map(|g| g.branch_indices.len())
            .sum()
    }

    /// 병렬 실행 가능한 그룹 수
    pub fn parallel_group_count(&self) -> usize {
        self.execution_groups
            .iter()
            .filter(|g| g.can_run_parallel)
            .count()
    }
}
