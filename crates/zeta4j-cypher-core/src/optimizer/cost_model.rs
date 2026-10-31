// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 쿼리 비용 모델
//!
//! 실행 계획의 비용을 추정하는 비용 모델을 정의합니다.
//! PostgreSQL의 비용 모델을 참고하여 설계되었습니다.

use super::statistics::StatisticsManager;
use crate::ast::{PlanNode, PlanNodeType};

/// 비용 모델 상수
#[derive(Debug, Clone)]
pub struct CostModel {
    // ========== I/O 비용 상수 ==========
    /// 순차 페이지 읽기 비용 (기준값)
    pub seq_page_cost: f64,

    /// 랜덤 페이지 읽기 비용 (순차 대비 약 4배)
    pub random_page_cost: f64,

    // ========== CPU 비용 상수 ==========
    /// 행(튜플) 처리 비용
    pub cpu_tuple_cost: f64,

    /// 인덱스 항목 처리 비용
    pub cpu_index_tuple_cost: f64,

    /// 연산자 평가 비용 (비교, 함수 호출 등)
    pub cpu_operator_cost: f64,

    // ========== 메모리 비용 상수 ==========
    /// 정렬 시 메모리 사용 임계값 (바이트)
    pub sort_mem_threshold: u64,

    /// 해시 테이블 빌드 비용 계수
    pub hash_build_cost: f64,
}

impl Default for CostModel {
    fn default() -> Self {
        Self {
            // PostgreSQL 기본값 참고
            seq_page_cost: 1.0,
            random_page_cost: 4.0,
            cpu_tuple_cost: 0.01,
            cpu_index_tuple_cost: 0.005,
            cpu_operator_cost: 0.0025,
            sort_mem_threshold: 256 * 1024, // 256KB
            hash_build_cost: 0.02,
        }
    }
}

impl CostModel {
    /// 새 비용 모델 생성 (기본값)
    pub fn new() -> Self {
        Self::default()
    }

    // ========== 스캔 비용 ==========

    /// 전체 노드 스캔 비용
    pub fn all_nodes_scan_cost(&self, total_nodes: u64) -> f64 {
        // 순차 스캔: 모든 노드 순회
        let pages = self.estimate_pages(total_nodes);
        (pages as f64 * self.seq_page_cost) + (total_nodes as f64 * self.cpu_tuple_cost)
    }

    /// 레이블 스캔 비용
    pub fn label_scan_cost(&self, label_nodes: u64, total_nodes: u64) -> f64 {
        // 레이블 인덱스 사용 시 더 효율적
        // 기본적으로 순차 스캔이지만 레이블 인덱스로 필터링
        let selectivity = if total_nodes > 0 {
            label_nodes as f64 / total_nodes as f64
        } else {
            1.0
        };

        let pages = self.estimate_pages(total_nodes);
        let scan_cost = pages as f64 * self.seq_page_cost * selectivity;
        let cpu_cost = label_nodes as f64 * self.cpu_tuple_cost;

        scan_cost + cpu_cost
    }

    /// ID 기반 노드 조회 비용 (가장 빠름)
    pub fn node_by_id_cost(&self) -> f64 {
        // 직접 접근: 상수 비용
        self.random_page_cost + self.cpu_tuple_cost
    }

    /// 인덱스 스캔 비용
    pub fn index_scan_cost(&self, matched_rows: u64, selectivity: f64) -> f64 {
        // 인덱스 탐색 비용 + 힙 접근 비용
        let index_pages = (matched_rows as f64 * selectivity).ceil() as u64;
        let index_cost = (index_pages.max(1) as f64).log2() * self.random_page_cost;
        let heap_cost = matched_rows as f64 * self.random_page_cost * 0.1; // 힙 접근 (인덱스 순서)
        let cpu_cost = matched_rows as f64 * self.cpu_index_tuple_cost;

        index_cost + heap_cost + cpu_cost
    }

    // ========== 필터 비용 ==========

    /// 필터 연산 비용
    pub fn filter_cost(&self, input_rows: u64, conditions: usize) -> f64 {
        // 각 행에 대해 조건 평가
        input_rows as f64 * conditions as f64 * self.cpu_operator_cost
    }

    // ========== 확장(조인) 비용 ==========

    /// 관계 확장(Expand) 비용
    pub fn expand_cost(&self, input_rows: u64, avg_degree: f64) -> f64 {
        // 각 입력 행에서 관계 탐색
        let output_rows = (input_rows as f64 * avg_degree) as u64;
        let traversal_cost = input_rows as f64 * self.random_page_cost * 0.5;
        let cpu_cost = output_rows as f64 * self.cpu_tuple_cost;

        traversal_cost + cpu_cost
    }

    /// Optional 확장 비용 (LEFT OUTER JOIN 유사)
    pub fn optional_expand_cost(&self, input_rows: u64, avg_degree: f64) -> f64 {
        // 일반 확장 + NULL 처리 오버헤드
        self.expand_cost(input_rows, avg_degree) * 1.1
    }

    // ========== 정렬/집계 비용 ==========

    /// 정렬 비용
    pub fn sort_cost(&self, rows: u64, sort_columns: usize) -> f64 {
        if rows <= 1 {
            return 0.0;
        }

        // O(n log n) 정렬
        let log_n = (rows as f64).log2();
        let comparison_cost = rows as f64 * log_n * sort_columns as f64 * self.cpu_operator_cost;

        // 메모리 초과 시 디스크 사용 (외부 정렬)
        let row_size = 64; // 평균 행 크기 추정
        let memory_needed = rows * row_size;
        let disk_cost = if memory_needed > self.sort_mem_threshold {
            let runs = (memory_needed / self.sort_mem_threshold) as f64;
            runs * self.seq_page_cost * 2.0 // 읽기 + 쓰기
        } else {
            0.0
        };

        comparison_cost + disk_cost
    }

    /// 집계 비용 (GROUP BY, COUNT, SUM 등)
    pub fn aggregation_cost(&self, input_rows: u64, group_columns: usize) -> f64 {
        // 해시 집계 또는 정렬 집계
        let hash_cost = input_rows as f64 * self.hash_build_cost;
        let cpu_cost = input_rows as f64 * group_columns as f64 * self.cpu_operator_cost;

        hash_cost + cpu_cost
    }

    /// DISTINCT 비용
    pub fn distinct_cost(&self, input_rows: u64, distinct_columns: usize) -> f64 {
        // 해시 기반 중복 제거
        self.aggregation_cost(input_rows, distinct_columns)
    }

    // ========== 기타 연산 비용 ==========

    /// SKIP 비용 (거의 무시)
    pub fn skip_cost(&self, _skip_count: u64) -> f64 {
        self.cpu_operator_cost
    }

    /// LIMIT 비용 (거의 무시)
    pub fn limit_cost(&self, _limit_count: u64) -> f64 {
        self.cpu_operator_cost
    }

    /// UNION 비용
    pub fn union_cost(&self, left_rows: u64, right_rows: u64) -> f64 {
        // 두 결과 합치기
        (left_rows + right_rows) as f64 * self.cpu_tuple_cost
    }

    /// CREATE 비용 (쓰기)
    pub fn create_cost(&self, nodes: u64, edges: u64) -> f64 {
        let node_cost = nodes as f64 * self.random_page_cost;
        let edge_cost = edges as f64 * self.random_page_cost;
        node_cost + edge_cost
    }

    /// DELETE 비용
    pub fn delete_cost(&self, items: u64) -> f64 {
        items as f64 * self.random_page_cost * 0.5
    }

    /// SET 비용 (프로퍼티 업데이트)
    pub fn set_cost(&self, items: u64) -> f64 {
        items as f64 * self.random_page_cost * 0.3
    }

    // ========== 유틸리티 ==========

    /// 행 수에서 페이지 수 추정
    fn estimate_pages(&self, rows: u64) -> u64 {
        // 페이지당 약 100개 행 가정
        (rows / 100).max(1)
    }

    /// description에서 레이블 추출 (예: "Person" 또는 ":Person")
    fn extract_label_from_description(&self, description: &str) -> Option<String> {
        // ":Person" 또는 "Person" 형태에서 레이블 추출
        let trimmed = description.trim();
        if let Some(stripped) = trimmed.strip_prefix(':') {
            Some(stripped.to_string())
        } else if !trimmed.is_empty() && trimmed.chars().next().is_some_and(|c| c.is_alphabetic()) {
            // 첫 단어를 레이블로 사용
            Some(
                trimmed
                    .split_whitespace()
                    .next()
                    .unwrap_or(trimmed)
                    .to_string(),
            )
        } else {
            None
        }
    }

    /// 인덱스 스캔 description에서 레이블과 프로퍼티 추출
    /// 형식: "n:Person USING INDEX ON :Person(age) WHERE ..."
    fn extract_index_info_from_description(&self, description: &str) -> Option<(String, String)> {
        // "ON :Label(property)" 패턴에서 레이블과 프로퍼티 추출
        if let Some(on_pos) = description.find("ON :") {
            let after_on = &description[on_pos + 4..];
            if let Some(paren_pos) = after_on.find('(') {
                let label = after_on[..paren_pos].to_string();
                if let Some(end_paren) = after_on.find(')') {
                    let property = after_on[paren_pos + 1..end_paren].to_string();
                    return Some((label, property));
                }
            }
        }
        None
    }

    /// 범위 조건 description에서 범위 값 추출
    /// 형식: "... WHERE n.age >= 20 AND n.age <= 50"
    fn extract_range_bounds_from_description(
        &self,
        description: &str,
    ) -> (Option<f64>, Option<f64>, bool, bool) {
        let mut lower: Option<f64> = None;
        let mut upper: Option<f64> = None;
        let mut lower_inclusive = false;
        let mut upper_inclusive = false;

        if let Some(where_pos) = description.find("WHERE ") {
            let where_clause = &description[where_pos + 6..];

            // ">=" 또는 ">" 패턴 찾기
            if let Some(gte_pos) = where_clause.find(" >= ") {
                let value_start = gte_pos + 4;
                if let Some(value) = self.parse_numeric_value(&where_clause[value_start..]) {
                    lower = Some(value);
                    lower_inclusive = true;
                }
            } else if let Some(gt_pos) = where_clause.find(" > ") {
                let value_start = gt_pos + 3;
                if let Some(value) = self.parse_numeric_value(&where_clause[value_start..]) {
                    lower = Some(value);
                    lower_inclusive = false;
                }
            }

            // "<=" 또는 "<" 패턴 찾기
            if let Some(lte_pos) = where_clause.find(" <= ") {
                let value_start = lte_pos + 4;
                if let Some(value) = self.parse_numeric_value(&where_clause[value_start..]) {
                    upper = Some(value);
                    upper_inclusive = true;
                }
            } else if let Some(lt_pos) = where_clause.find(" < ") {
                let value_start = lt_pos + 3;
                if let Some(value) = self.parse_numeric_value(&where_clause[value_start..]) {
                    upper = Some(value);
                    upper_inclusive = false;
                }
            }
        }

        (lower, upper, lower_inclusive, upper_inclusive)
    }

    /// 문자열에서 숫자 값 파싱
    fn parse_numeric_value(&self, s: &str) -> Option<f64> {
        // 숫자 부분만 추출 (공백이나 AND 전까지)
        let end = s
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .unwrap_or(s.len());
        s[..end].trim().parse::<f64>().ok()
    }

    /// 전체 계획 비용 계산
    pub fn total_plan_cost(&self, plan: &PlanNode, stats: &StatisticsManager) -> f64 {
        self.calculate_node_cost(plan, stats)
    }

    /// 개별 노드 비용 계산 (재귀)
    fn calculate_node_cost(&self, node: &PlanNode, stats: &StatisticsManager) -> f64 {
        // 자식 노드 비용 합계
        let children_cost: f64 = node
            .children
            .iter()
            .map(|c| self.calculate_node_cost(c, stats))
            .sum();

        // 현재 노드 비용
        let self_cost = match &node.node_type {
            PlanNodeType::AllNodesScan => self.all_nodes_scan_cost(stats.total_node_count()),
            PlanNodeType::NodeByLabelScan => {
                // description에서 레이블 추출 시도, 없으면 기본값 사용
                let label_count = self
                    .extract_label_from_description(&node.description)
                    .map(|l| {
                        let count = stats.label_count(&l);
                        // 레이블 통계가 0이면 전체의 10%로 추정
                        if count == 0 {
                            stats.total_node_count() / 10
                        } else {
                            count
                        }
                    })
                    .unwrap_or(stats.total_node_count() / 10)
                    .max(1); // 최소 1개
                self.label_scan_cost(label_count, stats.total_node_count())
            }
            PlanNodeType::NodeByIdSeek => self.node_by_id_cost(),
            PlanNodeType::NodeByIndexScan => {
                // 인덱스 스캔: 등호 조건
                // description에서 레이블과 프로퍼티 추출
                let selectivity = if let Some((label, property)) =
                    self.extract_index_info_from_description(&node.description)
                {
                    // 등호 조건의 selectivity = 1 / distinct_count
                    stats.equality_selectivity(&label, &property)
                } else {
                    0.01 // 기본 1% 선택도
                };
                self.index_scan_cost(node.estimated_rows.max(1), selectivity)
            }
            PlanNodeType::NodeByIndexRangeScan => {
                // 범위 인덱스 스캔: description에서 범위 정보 추출
                let selectivity = if let Some((label, property)) =
                    self.extract_index_info_from_description(&node.description)
                {
                    let (lower, upper, lower_inclusive, upper_inclusive) =
                        self.extract_range_bounds_from_description(&node.description);

                    // StatisticsManager에서 실제 selectivity 계산
                    stats.estimate_range_selectivity(
                        &label,
                        &property,
                        lower,
                        upper,
                        lower_inclusive,
                        upper_inclusive,
                    )
                } else {
                    0.1 // 기본 10% 선택도
                };
                self.index_scan_cost(node.estimated_rows.max(1), selectivity)
            }
            PlanNodeType::IndexIntersection => {
                // 인덱스 교집합: 교집합 연산 비용만 (자식 비용은 이미 children_cost에 포함)
                // 교집합 연산은 해시 기반으로 효율적
                (node.estimated_rows as f64).ln().max(1.0) + 1.0
            }
            PlanNodeType::Filter => self.filter_cost(node.estimated_rows, 1),
            PlanNodeType::Expand => {
                let avg_degree = stats.expand_estimate(None, true);
                let input_rows = node.children.first().map(|c| c.estimated_rows).unwrap_or(1);
                self.expand_cost(input_rows, avg_degree)
            }
            PlanNodeType::OptionalExpand => {
                let avg_degree = stats.expand_estimate(None, true);
                let input_rows = node.children.first().map(|c| c.estimated_rows).unwrap_or(1);
                self.optional_expand_cost(input_rows, avg_degree)
            }
            PlanNodeType::Sort => self.sort_cost(node.estimated_rows, 1),
            PlanNodeType::Aggregation => self.aggregation_cost(node.estimated_rows, 1),
            PlanNodeType::Distinct => self.distinct_cost(node.estimated_rows, 1),
            PlanNodeType::Skip => {
                self.skip_cost(10) // 기본값
            }
            PlanNodeType::Limit => {
                self.limit_cost(10) // 기본값
            }
            PlanNodeType::Union => {
                let left = node.children.first().map(|c| c.estimated_rows).unwrap_or(0);
                let right = node.children.get(1).map(|c| c.estimated_rows).unwrap_or(0);
                self.union_cost(left, right)
            }
            PlanNodeType::Create => {
                self.create_cost(1, 0) // 추정
            }
            PlanNodeType::Delete => self.delete_cost(node.estimated_rows),
            PlanNodeType::SetProperty => self.set_cost(node.estimated_rows),
            PlanNodeType::SetNodeProperties => {
                // 전체 속성 교체: 기존 속성 삭제 + 새 속성 설정
                self.set_cost(node.estimated_rows) * 1.5
            }
            PlanNodeType::SetNodePropertiesMerge => {
                // 속성 병합: 기존 속성 유지 + 새 속성 추가
                self.set_cost(node.estimated_rows) * 1.2
            }
            PlanNodeType::SetLabels => {
                // 레이블 추가: 인덱스 업데이트 포함
                self.set_cost(node.estimated_rows)
            }
            PlanNodeType::RemoveProperty => {
                // 속성 제거: SET과 유사한 비용
                self.set_cost(node.estimated_rows)
            }
            PlanNodeType::RemoveLabels => {
                // 레이블 제거: 인덱스 업데이트 포함
                self.set_cost(node.estimated_rows)
            }
            PlanNodeType::Merge => {
                // MERGE = 읽기 + 조건부 쓰기
                self.node_by_id_cost() + self.create_cost(1, 0) * 0.5
            }
            PlanNodeType::Unwind => node.estimated_rows as f64 * self.cpu_tuple_cost,
            PlanNodeType::Call => {
                // 프로시저 호출: 추정하기 어려움
                node.estimated_rows as f64 * self.cpu_tuple_cost * 10.0
            }
            PlanNodeType::Projection => {
                // RETURN/WITH 프로젝션
                node.estimated_rows as f64 * self.cpu_tuple_cost
            }
            PlanNodeType::ProduceResults => {
                // 최종 결과 생성 비용
                node.estimated_rows as f64 * self.cpu_tuple_cost
            }
            PlanNodeType::UnionAll => {
                let left = node.children.first().map(|c| c.estimated_rows).unwrap_or(0);
                let right = node.children.get(1).map(|c| c.estimated_rows).unwrap_or(0);
                self.union_cost(left, right)
            }
            PlanNodeType::CartesianProduct => {
                // CartesianProduct: cross join 비용 = 양쪽 곱
                let left = node.children.first().map(|c| c.estimated_rows).unwrap_or(1);
                let right = node.children.get(1).map(|c| c.estimated_rows).unwrap_or(1);
                (left * right) as f64 * self.cpu_tuple_cost
            }
            PlanNodeType::Admin => {
                // 관리 명령: 일반적으로 빠른 메타데이터 연산
                self.cpu_tuple_cost
            }
            PlanNodeType::Foreach => {
                // FOREACH: 리스트 순회 + 내부 업데이트 비용
                node.estimated_rows as f64 * self.cpu_tuple_cost * 5.0
            }
            PlanNodeType::LoadCsv => {
                // LOAD CSV: 파일 읽기 + 파싱 비용
                self.seq_page_cost * 10.0 + node.estimated_rows as f64 * self.cpu_tuple_cost * 2.0
            }
            PlanNodeType::EmptyResult => {
                // FINISH: 빈 결과, 비용 0
                0.0
            }
        };

        children_cost + self_cost
    }
}
