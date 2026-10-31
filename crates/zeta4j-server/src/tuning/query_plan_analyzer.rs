// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 쿼리 플랜 분석기
//!
//! 쿼리 실행 계획을 분석하여 인덱스 추천 및 최적화 제안을 제공합니다.

use super::index_advisor::{IndexRecommendation, QueryPattern, RecommendedIndexType};
use std::collections::HashMap;
use zeta4j_cypher_executor::ast::{PlanNode, PlanNodeType, QueryPlan};

/// 쿼리 플랜 분석 결과
#[derive(Debug, Clone)]
pub struct PlanAnalysis {
    /// 분석된 쿼리 (원본)
    pub query_text: Option<String>,
    /// 총 예상 비용
    pub total_cost: f64,
    /// 예상 행 수
    pub estimated_rows: u64,
    /// 탐지된 문제점들
    pub issues: Vec<PlanIssue>,
    /// 최적화 제안
    pub suggestions: Vec<OptimizationSuggestion>,
    /// 사용된 인덱스
    pub used_indexes: Vec<String>,
    /// 누락된 인덱스 (인덱스가 있었다면 성능 향상 가능)
    pub missing_indexes: Vec<IndexRecommendation>,
    /// 연산별 비용 분석
    pub operation_costs: Vec<OperationCost>,
}

impl PlanAnalysis {
    /// 새 분석 결과 생성
    pub fn new() -> Self {
        Self {
            query_text: None,
            total_cost: 0.0,
            estimated_rows: 0,
            issues: Vec::new(),
            suggestions: Vec::new(),
            used_indexes: Vec::new(),
            missing_indexes: Vec::new(),
            operation_costs: Vec::new(),
        }
    }

    /// 전체 점수 (0-100, 높을수록 좋음)
    pub fn health_score(&self) -> u32 {
        let mut score = 100i32;

        // 심각한 문제: -20점씩
        score -= self
            .issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Critical)
            .count() as i32
            * 20;
        // 경고: -10점씩
        score -= self
            .issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count() as i32
            * 10;
        // 힌트: -5점씩
        score -= self
            .issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Hint)
            .count() as i32
            * 5;

        score.max(0) as u32
    }

    /// 요약 문자열
    pub fn summary(&self) -> String {
        format!(
            "Cost: {:.2}, Rows: {}, Issues: {}, Score: {}",
            self.total_cost,
            self.estimated_rows,
            self.issues.len(),
            self.health_score()
        )
    }
}

impl Default for PlanAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

/// 플랜 문제점
#[derive(Debug, Clone)]
pub struct PlanIssue {
    /// 심각도
    pub severity: IssueSeverity,
    /// 문제 유형
    pub issue_type: IssueType,
    /// 설명
    pub description: String,
    /// 관련 노드 타입
    pub node_type: Option<PlanNodeType>,
    /// 예상 영향 (비용 증가율)
    pub impact: f64,
}

/// 문제 심각도
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueSeverity {
    /// 심각 - 즉시 조치 필요
    Critical,
    /// 경고 - 성능 저하 가능
    Warning,
    /// 힌트 - 개선 여지 있음
    Hint,
}

impl IssueSeverity {
    /// 이름 반환
    pub fn name(&self) -> &str {
        match self {
            IssueSeverity::Critical => "CRITICAL",
            IssueSeverity::Warning => "WARNING",
            IssueSeverity::Hint => "HINT",
        }
    }
}

/// 문제 유형
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueType {
    /// 전체 테이블 스캔
    FullTableScan,
    /// 인덱스 미사용
    MissingIndex,
    /// 카티션 곱
    CartesianProduct,
    /// 높은 필터 비율
    HighFilterRatio,
    /// 큰 정렬 연산
    LargeSort,
    /// 비효율적 확장
    InefficientExpand,
}

impl IssueType {
    /// 이름 반환
    pub fn name(&self) -> &str {
        match self {
            IssueType::FullTableScan => "Full Table Scan",
            IssueType::MissingIndex => "Missing Index",
            IssueType::CartesianProduct => "Cartesian Product",
            IssueType::HighFilterRatio => "High Filter Ratio",
            IssueType::LargeSort => "Large Sort",
            IssueType::InefficientExpand => "Inefficient Expand",
        }
    }
}

/// 최적화 제안
#[derive(Debug, Clone)]
pub struct OptimizationSuggestion {
    /// 제안 유형
    pub suggestion_type: SuggestionType,
    /// 설명
    pub description: String,
    /// 예상 개선율 (0.0 ~ 1.0, 1.0 = 100% 개선)
    pub estimated_improvement: f64,
    /// 실행할 명령 (Cypher 등)
    pub action: Option<String>,
}

/// 제안 유형
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionType {
    /// 인덱스 생성
    CreateIndex,
    /// 쿼리 재작성
    RewriteQuery,
    /// 통계 갱신
    UpdateStatistics,
    /// 제약조건 추가
    AddConstraint,
}

impl SuggestionType {
    /// 이름 반환
    pub fn name(&self) -> &str {
        match self {
            SuggestionType::CreateIndex => "Create Index",
            SuggestionType::RewriteQuery => "Rewrite Query",
            SuggestionType::UpdateStatistics => "Update Statistics",
            SuggestionType::AddConstraint => "Add Constraint",
        }
    }
}

/// 연산별 비용 분석
#[derive(Debug, Clone)]
pub struct OperationCost {
    /// 연산 유형
    pub operation: PlanNodeType,
    /// 설명
    pub description: String,
    /// 비용
    pub cost: f64,
    /// 예상 행 수
    pub rows: u64,
    /// 전체 대비 비율
    pub cost_percentage: f64,
}

/// 쿼리 플랜 분석기 설정
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// 전체 스캔 경고 임계값 (행 수)
    pub full_scan_warning_threshold: u64,
    /// 높은 필터 비율 경고 임계값 (0.0 ~ 1.0)
    pub high_filter_ratio_threshold: f64,
    /// 큰 정렬 경고 임계값 (행 수)
    pub large_sort_threshold: u64,
    /// 카티션 곱 경고 활성화
    pub warn_cartesian_product: bool,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            full_scan_warning_threshold: 10000,
            high_filter_ratio_threshold: 0.9,
            large_sort_threshold: 100000,
            warn_cartesian_product: true,
        }
    }
}

/// 쿼리 플랜 분석기
pub struct QueryPlanAnalyzer {
    /// 설정
    config: AnalyzerConfig,
    /// 기존 인덱스 목록 (라벨:프로퍼티)
    existing_indexes: HashMap<String, Vec<String>>,
}

impl QueryPlanAnalyzer {
    /// 새 분석기 생성
    pub fn new(config: AnalyzerConfig) -> Self {
        Self {
            config,
            existing_indexes: HashMap::new(),
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(AnalyzerConfig::default())
    }

    /// 기존 인덱스 등록
    pub fn register_index(&mut self, label: &str, property: &str) {
        self.existing_indexes
            .entry(label.to_string())
            .or_default()
            .push(property.to_string());
    }

    /// 인덱스 존재 여부 확인
    pub fn has_index(&self, label: &str, property: &str) -> bool {
        self.existing_indexes
            .get(label)
            .map(|props| props.contains(&property.to_string()))
            .unwrap_or(false)
    }

    /// 쿼리 플랜 분석
    pub fn analyze(&self, plan: &QueryPlan) -> PlanAnalysis {
        let mut analysis = PlanAnalysis::new();

        // 루트 노드부터 분석 시작
        self.analyze_node(&plan.root, &mut analysis, 0.0);

        // 총 비용 및 행 수 계산
        analysis.total_cost = self.calculate_total_cost(&plan.root);
        analysis.estimated_rows = plan.root.estimated_rows;

        // 연산별 비용 비율 계산
        if analysis.total_cost > 0.0 {
            for op_cost in &mut analysis.operation_costs {
                op_cost.cost_percentage = op_cost.cost / analysis.total_cost * 100.0;
            }
        }

        // 비용이 높은 순으로 정렬
        analysis
            .operation_costs
            .sort_by(|a, b| b.cost.total_cmp(&a.cost));

        // 문제점 기반 제안 생성
        self.generate_suggestions(&mut analysis);

        analysis
    }

    /// 노드 분석 (재귀)
    fn analyze_node(&self, node: &PlanNode, analysis: &mut PlanAnalysis, parent_rows: f64) {
        // 현재 노드 연산 비용 기록
        let cost = self.estimate_node_cost(node);
        analysis.operation_costs.push(OperationCost {
            operation: node.node_type.clone(),
            description: node.description.clone(),
            cost,
            rows: node.estimated_rows,
            cost_percentage: 0.0, // 나중에 계산
        });

        // 노드 타입별 분석
        match &node.node_type {
            PlanNodeType::AllNodesScan => {
                if node.estimated_rows > self.config.full_scan_warning_threshold {
                    analysis.issues.push(PlanIssue {
                        severity: IssueSeverity::Warning,
                        issue_type: IssueType::FullTableScan,
                        description: format!(
                            "Full node scan ({} rows): adding label filter recommended",
                            node.estimated_rows
                        ),
                        node_type: Some(node.node_type.clone()),
                        impact: cost,
                    });
                }
            }
            PlanNodeType::NodeByLabelScan => {
                // 레이블 스캔에서 인덱스 추천 기회 탐지
                if node.estimated_rows > self.config.full_scan_warning_threshold
                    && let Some((label, _prop)) = self.extract_label_property(&node.description)
                {
                    // WHERE 절이 있는데 인덱스가 없는 경우 추천
                    analysis.issues.push(PlanIssue {
                        severity: IssueSeverity::Hint,
                        issue_type: IssueType::MissingIndex,
                        description: format!(
                            "Label scan '{}' expects {} rows: consider property index",
                            label, node.estimated_rows
                        ),
                        node_type: Some(node.node_type.clone()),
                        impact: cost * 0.5,
                    });
                }
            }
            PlanNodeType::NodeByIndexScan => {
                // 인덱스 사용 기록
                if let Some(idx_name) = self.extract_index_name(&node.description) {
                    analysis.used_indexes.push(idx_name);
                }
            }
            PlanNodeType::Filter => {
                // 필터 비율 분석
                if parent_rows > 0.0 && node.estimated_rows > 0 {
                    let filter_ratio = 1.0 - (node.estimated_rows as f64 / parent_rows);
                    if filter_ratio > self.config.high_filter_ratio_threshold {
                        analysis.issues.push(PlanIssue {
                            severity: IssueSeverity::Hint,
                            issue_type: IssueType::HighFilterRatio,
                            description: format!(
                                "High filter ratio ({:.1}%): pre-filtering with index recommended",
                                filter_ratio * 100.0
                            ),
                            node_type: Some(node.node_type.clone()),
                            impact: cost,
                        });
                    }
                }
            }
            PlanNodeType::CartesianProduct => {
                if self.config.warn_cartesian_product {
                    analysis.issues.push(PlanIssue {
                        severity: IssueSeverity::Critical,
                        issue_type: IssueType::CartesianProduct,
                        description: format!(
                            "Cartesian product detected: {} rows generated. Join condition needed",
                            node.estimated_rows
                        ),
                        node_type: Some(node.node_type.clone()),
                        impact: cost * 2.0,
                    });
                }
            }
            PlanNodeType::Sort => {
                if node.estimated_rows > self.config.large_sort_threshold {
                    analysis.issues.push(PlanIssue {
                        severity: IssueSeverity::Warning,
                        issue_type: IssueType::LargeSort,
                        description: format!(
                            "Large sort ({} rows): consider index on ORDER BY property",
                            node.estimated_rows
                        ),
                        node_type: Some(node.node_type.clone()),
                        impact: cost,
                    });
                }
            }
            PlanNodeType::Expand | PlanNodeType::OptionalExpand
                // 확장 연산에서 높은 cardinality 체크
                if node.estimated_rows > parent_rows as u64 * 100 => {
                    analysis.issues.push(PlanIssue {
                        severity: IssueSeverity::Hint,
                        issue_type: IssueType::InefficientExpand,
                        description:
                            "Expand operation increased rows by 100x+: consider relationship type filter or LIMIT"
                                .to_string(),
                        node_type: Some(node.node_type.clone()),
                        impact: cost * 0.3,
                    });
                }
            _ => {}
        }

        // 자식 노드 분석
        for child in &node.children {
            self.analyze_node(child, analysis, node.estimated_rows as f64);
        }
    }

    /// 노드 비용 추정
    fn estimate_node_cost(&self, node: &PlanNode) -> f64 {
        let rows = node.estimated_rows.max(1) as f64;

        match &node.node_type {
            PlanNodeType::AllNodesScan => rows * 1.0,
            PlanNodeType::NodeByLabelScan => rows * 0.5,
            PlanNodeType::NodeByIdSeek => 1.0,
            PlanNodeType::NodeByIndexScan => rows.log2().max(1.0) * 1.5,
            PlanNodeType::Filter => rows * 0.1,
            PlanNodeType::Expand | PlanNodeType::OptionalExpand => rows * 0.8,
            PlanNodeType::Sort => rows * rows.log2().max(1.0) * 0.5,
            PlanNodeType::Aggregation => rows * 0.3,
            PlanNodeType::Distinct => rows * 0.2,
            PlanNodeType::CartesianProduct => rows * 1.5,
            PlanNodeType::Union | PlanNodeType::UnionAll => rows * 0.1,
            _ => rows * 0.05,
        }
    }

    /// 총 비용 계산
    fn calculate_total_cost(&self, node: &PlanNode) -> f64 {
        let mut cost = self.estimate_node_cost(node);
        for child in &node.children {
            cost += self.calculate_total_cost(child);
        }
        cost
    }

    /// 제안 생성
    fn generate_suggestions(&self, analysis: &mut PlanAnalysis) {
        // 문제점 기반 제안 생성
        for issue in &analysis.issues {
            match issue.issue_type {
                IssueType::FullTableScan => {
                    analysis.suggestions.push(OptimizationSuggestion {
                        suggestion_type: SuggestionType::CreateIndex,
                        description: "Specify a label or create a property index".to_string(),
                        estimated_improvement: 0.7,
                        action: None,
                    });
                }
                IssueType::MissingIndex => {
                    if let Some((label, _prop)) =
                        self.extract_label_from_description(&issue.description)
                    {
                        analysis.suggestions.push(OptimizationSuggestion {
                            suggestion_type: SuggestionType::CreateIndex,
                            description: format!(
                                "Index recommended for frequently queried property (label: {})",
                                label
                            ),
                            estimated_improvement: 0.5,
                            action: Some(format!("CREATE INDEX ON :{}(<property>)", label)),
                        });
                    }
                }
                IssueType::CartesianProduct => {
                    analysis.suggestions.push(OptimizationSuggestion {
                        suggestion_type: SuggestionType::RewriteQuery,
                        description:
                            "Add a relationship to the MATCH pattern to avoid Cartesian product"
                                .to_string(),
                        estimated_improvement: 0.9,
                        action: None,
                    });
                }
                IssueType::HighFilterRatio => {
                    analysis.suggestions.push(OptimizationSuggestion {
                        suggestion_type: SuggestionType::CreateIndex,
                        description: "Create an index on the property in the WHERE clause"
                            .to_string(),
                        estimated_improvement: 0.6,
                        action: None,
                    });
                }
                IssueType::LargeSort => {
                    analysis.suggestions.push(OptimizationSuggestion {
                        suggestion_type: SuggestionType::CreateIndex,
                        description:
                            "Creating an index on the ORDER BY property will speed up sorting"
                                .to_string(),
                        estimated_improvement: 0.4,
                        action: None,
                    });
                }
                IssueType::InefficientExpand => {
                    analysis.suggestions.push(OptimizationSuggestion {
                        suggestion_type: SuggestionType::RewriteQuery,
                        description: "Specify relationship type or add LIMIT".to_string(),
                        estimated_improvement: 0.3,
                        action: None,
                    });
                }
            }
        }

        // 중복 제거
        analysis.suggestions.dedup_by(|a, b| {
            a.suggestion_type == b.suggestion_type && a.description == b.description
        });
    }

    /// 설명에서 라벨 추출
    fn extract_label_from_description(
        &self,
        description: &str,
    ) -> Option<(String, Option<String>)> {
        // "라벨 스캔 'Person'..." 패턴
        if let Some(start) = description.find('\'')
            && let Some(end) = description[start + 1..].find('\'')
        {
            let label = &description[start + 1..start + 1 + end];
            return Some((label.to_string(), None));
        }
        None
    }

    /// 설명에서 라벨과 프로퍼티 추출
    fn extract_label_property(&self, description: &str) -> Option<(String, Option<String>)> {
        // "n:Person" 또는 ":Person" 패턴
        if let Some(pos) = description.find(':') {
            let after = &description[pos + 1..];
            let label = after
                .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
                .next()
                .unwrap_or("");
            if !label.is_empty() {
                // WHERE n.prop = ... 패턴에서 프로퍼티 추출
                if let Some(where_pos) = description.find("WHERE") {
                    let where_part = &description[where_pos..];
                    if let Some(dot_pos) = where_part.find('.') {
                        let after_dot = &where_part[dot_pos + 1..];
                        let prop = after_dot
                            .split(|c: char| !c.is_alphanumeric() && c != '_')
                            .next()
                            .unwrap_or("");
                        if !prop.is_empty() {
                            return Some((label.to_string(), Some(prop.to_string())));
                        }
                    }
                }
                return Some((label.to_string(), None));
            }
        }
        None
    }

    /// 인덱스 이름 추출
    fn extract_index_name(&self, description: &str) -> Option<String> {
        // "USING INDEX ON :Label(prop)" 패턴
        if let Some(idx_start) = description.find("USING INDEX") {
            let idx_part = &description[idx_start..];
            if let Some(label_start) = idx_part.find(':')
                && let Some(prop_start) = idx_part.find('(')
                && let Some(prop_end) = idx_part.find(')')
            {
                let label = &idx_part[label_start + 1..prop_start];
                let prop = &idx_part[prop_start + 1..prop_end];
                return Some(format!("{}_{}", label, prop));
            }
        }
        None
    }

    /// QueryPattern에서 인덱스 추천 생성
    pub fn recommend_indexes_from_patterns(
        &self,
        patterns: &[QueryPattern],
    ) -> Vec<IndexRecommendation> {
        let mut recommendations = Vec::new();

        for pattern in patterns {
            // 라벨이 없으면 스킵
            let label = match &pattern.label {
                Some(l) => l.clone(),
                None => continue,
            };

            // 프로퍼티가 없으면 스킵
            if pattern.properties.is_empty() {
                continue;
            }

            // 기존 인덱스가 있으면 스킵
            let has_index = pattern
                .properties
                .iter()
                .any(|prop| self.has_index(&label, prop));

            if has_index {
                continue;
            }

            // 빈도와 스캔 행 수를 기반으로 우선순위 계산
            let priority = (pattern.frequency as f64) * pattern.avg_scan_rows.log2().max(1.0);

            // 인덱스 타입 결정
            let (index_type, reason) = if pattern.properties.len() > 1 {
                (
                    RecommendedIndexType::Composite,
                    format!(
                        "{} properties queried together (frequency: {} times)",
                        pattern.properties.len(),
                        pattern.frequency
                    ),
                )
            } else {
                let prop = &pattern.properties[0];
                if prop.contains("text") || prop.contains("description") || prop.contains("content")
                {
                    (
                        RecommendedIndexType::Fulltext,
                        format!(
                            "Text property '{}' (frequency: {} times)",
                            prop, pattern.frequency
                        ),
                    )
                } else if prop.contains("location")
                    || prop.contains("geo")
                    || prop.contains("point")
                {
                    (
                        RecommendedIndexType::Spatial,
                        format!(
                            "Spatial property '{}' (frequency: {} times)",
                            prop, pattern.frequency
                        ),
                    )
                } else {
                    (
                        RecommendedIndexType::BTree,
                        format!(
                            "Property '{}' queried (frequency: {} times, avg {:.0} rows scanned)",
                            prop, pattern.frequency, pattern.avg_scan_rows
                        ),
                    )
                }
            };

            // 예상 성능 향상 계산
            let improvement = if pattern.avg_scan_rows > 10000.0 {
                20.0
            } else if pattern.avg_scan_rows > 1000.0 {
                10.0
            } else if pattern.avg_scan_rows > 100.0 {
                5.0
            } else {
                2.0
            };

            recommendations.push(IndexRecommendation {
                label,
                properties: pattern.properties.clone(),
                index_type,
                estimated_improvement: improvement,
                reason,
                priority_score: priority,
            });
        }

        // 우선순위 순으로 정렬
        recommendations.sort_by(|a, b| b.priority_score.total_cmp(&a.priority_score));

        recommendations
    }

    /// 플랜에서 쿼리 패턴 추출
    pub fn extract_patterns_from_plan(&self, plan: &QueryPlan) -> Vec<QueryPattern> {
        let mut patterns = Vec::new();
        self.extract_patterns_from_node(&plan.root, &mut patterns);
        patterns
    }

    /// 노드에서 패턴 추출 (재귀)
    fn extract_patterns_from_node(&self, node: &PlanNode, patterns: &mut Vec<QueryPattern>) {
        match &node.node_type {
            PlanNodeType::NodeByLabelScan | PlanNodeType::NodeByIndexScan => {
                if let Some((label, prop)) = self.extract_label_property(&node.description) {
                    let properties = prop.map(|p| vec![p]).unwrap_or_default();
                    patterns.push(QueryPattern {
                        label: Some(label),
                        properties,
                        frequency: 1,
                        avg_scan_rows: node.estimated_rows as f64,
                    });
                }
            }
            PlanNodeType::Filter => {
                // 필터에서 프로퍼티 추출 시도
                if let Some((label, Some(p))) = self.extract_label_property(&node.description) {
                    patterns.push(QueryPattern {
                        label: Some(label),
                        properties: vec![p],
                        frequency: 1,
                        avg_scan_rows: node.estimated_rows as f64,
                    });
                }
            }
            _ => {}
        }

        for child in &node.children {
            self.extract_patterns_from_node(child, patterns);
        }
    }
}

impl Default for QueryPlanAnalyzer {
    fn default() -> Self {
        Self::with_defaults()
    }
}
