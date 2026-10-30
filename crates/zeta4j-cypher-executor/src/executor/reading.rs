//! MATCH 절 실행 모듈
//!
//! MATCH, OPTIONAL MATCH 패턴 매칭 및 관계 탐색
//!
//! # v1.12.0 비동기 API 통일
//!
//! 모든 패턴 매칭 메서드가 async로 변경되었습니다.
//! WHERE 절의 표현식 평가가 비동기로 처리됩니다.

use super::{ExecutionContext, ExecutorIndexCondition, QueryExecutor, QueryHints};
use std::collections::{HashSet, VecDeque};
use zeta4j_cypher_core::ast::*;
use zeta4j_graph::core::{Node, NodeId, Path, PropertyContainer, Value};
use zeta4j_graph::error::{GraphError, Result};
use zeta4j_graph::transaction::TransactionId;

impl QueryExecutor {
    /// PropertyPattern의 프로퍼티가 노드와 일치하는지 검사
    /// Expression을 평가하여 Value로 변환 후 비교
    ///
    /// v1.12.0: async로 변경
    async fn matches_property_pattern(
        &self,
        props: &PropertyPattern,
        node: &Node,
        context: &ExecutionContext,
    ) -> Result<bool> {
        for (key, expr) in props.iter() {
            let expected_value = self.evaluate_expression(expr, context).await?;
            match node.get_property(key) {
                Some(actual_value) if *actual_value == expected_value => continue,
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
    /// MATCH/OPTIONAL MATCH 절 실행
    /// 콤마로 분리된 여러 패턴 지원: MATCH (a), (b), (c)-[:REL]->(d)
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_reading(
        &self,
        clause: &ReadingClause,
        context: &ExecutionContext,
    ) -> Result<Vec<ExecutionContext>> {
        self.execute_reading_with_hints(clause, context, &QueryHints::default())
            .await
    }

    /// MATCH/OPTIONAL MATCH 절 실행 (힌트 지원)
    /// LIMIT push-down 최적화: 조건이 충족되면 조기 종료
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_reading_with_hints(
        &self,
        clause: &ReadingClause,
        context: &ExecutionContext,
        hints: &QueryHints,
    ) -> Result<Vec<ExecutionContext>> {
        match clause {
            ReadingClause::Match {
                patterns,
                where_clause,
                optional,
            } => {
                // 여러 패턴을 순차적으로 처리 (Cross-Join)
                let mut current_contexts = vec![context.clone()];

                // LIMIT push-down: WHERE 절이 없을 때만 안전하게 적용
                // WHERE 절이 있으면 모든 후보를 스캔 후 필터링해야 함
                let effective_limit = hints.effective_limit();
                let can_push_down_limit = where_clause.is_none();
                let scan_limit = if can_push_down_limit {
                    effective_limit
                } else {
                    None
                };

                for pattern in patterns {
                    let mut next_contexts = Vec::new();

                    for ctx in &current_contexts {
                        // LIMIT push-down: WHERE 절이 없을 때만 조기 종료
                        if let Some(limit) = scan_limit
                            && next_contexts.len() >= limit
                        {
                            break;
                        }

                        let matches = self
                            .execute_single_pattern_with_limit(
                                pattern,
                                ctx,
                                scan_limit.map(|l| l.saturating_sub(next_contexts.len())),
                            )
                            .await?;
                        next_contexts.extend(matches);
                    }

                    current_contexts = next_contexts;

                    // 매칭 결과가 없으면 조기 종료 (단, OPTIONAL MATCH는 예외)
                    if current_contexts.is_empty() {
                        if *optional {
                            // OPTIONAL MATCH: null 바인딩 생성 후 계속
                            current_contexts =
                                vec![self.create_null_context_for_patterns(patterns, context)];
                        } else {
                            break;
                        }
                    }

                    // LIMIT push-down: WHERE 절이 없을 때만 조기 종료
                    if let Some(limit) = scan_limit
                        && current_contexts.len() >= limit
                    {
                        current_contexts.truncate(limit);
                        break;
                    }
                }

                // WHERE 절 평가 (모든 패턴 바인딩 후)
                if let Some(where_expr) = where_clause {
                    let mut filtered_contexts = Vec::new();
                    for ctx in current_contexts {
                        let result = self.evaluate_expression(where_expr, &ctx).await?;
                        if self.is_truthy(&result) {
                            filtered_contexts.push(ctx);

                            // WHERE 평가 후 LIMIT 체크
                            if let Some(limit) = effective_limit
                                && filtered_contexts.len() >= limit
                            {
                                break;
                            }
                        }
                    }
                    current_contexts = filtered_contexts;
                }

                // OPTIONAL MATCH: 매치 없으면 NULL로 바인딩
                if *optional && current_contexts.is_empty() {
                    return Ok(vec![
                        self.create_null_context_for_patterns(patterns, context),
                    ]);
                }

                Ok(current_contexts)
            }
            ReadingClause::LoadCsv {
                with_headers,
                url,
                variable,
                field_terminator,
            } => self.execute_load_csv(with_headers, url, variable, field_terminator, context),
        }
    }

    /// LOAD CSV 절 실행
    fn execute_load_csv(
        &self,
        with_headers: &bool,
        url: &str,
        variable: &str,
        field_terminator: &Option<char>,
        context: &ExecutionContext,
    ) -> Result<Vec<ExecutionContext>> {
        let delimiter = field_terminator.unwrap_or(',');

        // URL 파싱 및 파일 읽기
        let content = if url.starts_with("file://") {
            let path = url.strip_prefix("file://").unwrap_or(url);
            std::fs::read_to_string(path).map_err(|e| GraphError::Validation {
                message: format!("Failed to read CSV file '{}': {}", path, e),
            })?
        } else if url.starts_with("http://") || url.starts_with("https://") {
            return Err(GraphError::NotImplemented {
                feature: "HTTP URLs for LOAD CSV".to_string(),
            });
        } else {
            // file:// 없으면 로컬 파일로 가정
            std::fs::read_to_string(url).map_err(|e| GraphError::Validation {
                message: format!("Failed to read CSV file '{}': {}", url, e),
            })?
        };

        let mut results = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            return Ok(results);
        }

        if *with_headers {
            // WITH HEADERS: 첫 행을 헤더로 사용
            let headers: Vec<String> = self.parse_csv_line(lines[0], delimiter);

            for line in lines.iter().skip(1) {
                let values = self.parse_csv_line(line, delimiter);
                let mut row_map = indexmap::IndexMap::new();

                for (i, header) in headers.iter().enumerate() {
                    let value = values.get(i).cloned().unwrap_or_default();
                    row_map.insert(header.clone(), Value::String(value));
                }

                let mut new_ctx = context.clone();
                new_ctx
                    .variables
                    .insert(variable.to_string(), Value::Map(row_map));
                results.push(new_ctx);
            }
        } else {
            // 헤더 없음: 배열로 반환
            for line in &lines {
                let values = self.parse_csv_line(line, delimiter);
                let array: Vec<Value> = values.into_iter().map(Value::String).collect();

                let mut new_ctx = context.clone();
                new_ctx
                    .variables
                    .insert(variable.to_string(), Value::Array(array));
                results.push(new_ctx);
            }
        }

        Ok(results)
    }

    /// CSV 라인 파싱 (따옴표 지원)
    fn parse_csv_line(&self, line: &str, delimiter: char) -> Vec<String> {
        let mut result = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;

        for c in line.chars() {
            if c == '"' {
                in_quotes = !in_quotes;
            } else if c == delimiter && !in_quotes {
                result.push(current.trim().to_string());
                current = String::new();
            } else {
                current.push(c);
            }
        }
        result.push(current.trim().to_string());
        result
    }

    /// 단일 패턴 실행 (LIMIT push-down 지원)
    /// limit 파라미터가 Some이면 해당 개수에 도달 시 조기 종료
    ///
    /// v1.12.0: async로 변경
    #[async_recursion::async_recursion]
    async fn execute_single_pattern_with_limit(
        &self,
        pattern: &Pattern,
        context: &ExecutionContext,
        limit: Option<usize>,
    ) -> Result<Vec<ExecutionContext>> {
        // shortestPath/allShortestPaths 함수가 있으면 최단 경로 계산
        if let Some(path_func) = &pattern.path_function {
            return self
                .execute_shortest_path_pattern(pattern, path_func, context)
                .await;
        }

        // 패턴에 엣지가 있는지 확인 (elements 사용)
        if pattern.elements.len() > 1 {
            // 경로 패턴 (노드-엣지-노드) 처리 - WHERE 없이 실행
            return self
                .execute_path_match(pattern, &None, context, limit)
                .await;
        }

        // 단순 노드 패턴 처리
        if let Some(part) = pattern.parts.first() {
            let label = part.labels.first();

            // 라벨로 후보 노드 검색 (라벨 없으면 전체 스캔)
            let candidates = if let Some(label) = label {
                self.engine
                    .find_nodes_by_label_in_transaction(context.tx_id, label)
                    .await?
            } else {
                // 전체 노드 스캔 (성능 주의)
                self.engine
                    .all_node_ids_in_transaction(context.tx_id)
                    .await?
            };

            let _variable = part.variable.as_deref();
            // candidates는 이미 위에서 설정됨

            let mut matches = Vec::new();

            for node_id in candidates {
                // LIMIT push-down: 충분한 결과가 있으면 조기 종료
                if let Some(lim) = limit
                    && matches.len() >= lim
                {
                    break;
                }

                let node = self
                    .engine
                    .get_node_in_transaction(context.tx_id, node_id)
                    .await?;
                if let Some(node) = node {
                    // 모든 필수 레이블 검사
                    let has_all_labels = part
                        .labels
                        .iter()
                        .all(|required| node.labels().iter().any(|l| l == required));
                    if !has_all_labels {
                        continue;
                    }

                    // PropertyPattern 평가
                    let matched = if let Some(props) = &part.properties {
                        self.matches_property_pattern(props, &node, context).await?
                    } else {
                        true
                    };

                    if matched {
                        let mut new_ctx = context.clone();
                        if let Some(var) = &part.variable {
                            new_ctx.variables.insert(var.clone(), Value::Node(node_id));
                        }
                        matches.push(new_ctx);
                    }
                }
            }

            Ok(matches)
        } else {
            Ok(vec![context.clone()])
        }
    }

    /// shortestPath/allShortestPaths 패턴 실행
    ///
    /// v1.12.0: async로 변경
    async fn execute_shortest_path_pattern(
        &self,
        pattern: &Pattern,
        path_func: &PathFunction,
        context: &ExecutionContext,
    ) -> Result<Vec<ExecutionContext>> {
        use zeta4j_graph::algorithm::BfsShortestPath;

        // 패턴에서 시작/끝 노드 패턴 추출
        // 최소 2개 노드가 있어야 함 (시작 노드, 끝 노드)
        if pattern.parts.len() < 2 {
            return Err(GraphError::Validation {
                message: "shortestPath pattern requires start and end nodes".to_string(),
            });
        }

        let start_pattern = &pattern.parts[0];
        let end_pattern = pattern.parts.last().ok_or_else(|| GraphError::Validation {
            message: "Variable-length pattern requires at least 2 parts".to_string(),
        })?;

        // 시작 노드 찾기
        let start_nodes = self.find_matching_nodes(start_pattern, context).await?;
        // 끝 노드 찾기
        let end_nodes = self.find_matching_nodes(end_pattern, context).await?;

        // 엣지 패턴에서 관계 타입과 최대 깊이 추출
        let (rel_types, max_depth) = self.extract_edge_constraints(pattern);

        let mut results = Vec::new();
        let bfs = BfsShortestPath::new(&self.engine);

        for start_id in &start_nodes {
            for end_id in &end_nodes {
                if start_id == end_id {
                    continue; // 자기 자신으로의 경로 제외
                }

                // 최단 경로 찾기 (무방향 탐색)
                let path_result = bfs
                    .find(
                        *start_id,
                        *end_id,
                        max_depth,
                        rel_types.as_deref(),
                        false, // undirected
                    )
                    .await?;

                if let Some(sp) = path_result {
                    let mut new_ctx = context.clone();

                    // 시작/끝 노드 변수 바인딩
                    if let Some(var) = &start_pattern.variable {
                        new_ctx
                            .variables
                            .insert(var.clone(), Value::Node(*start_id));
                    }
                    if let Some(var) = &end_pattern.variable {
                        new_ctx.variables.insert(var.clone(), Value::Node(*end_id));
                    }

                    // 경로 변수 바인딩
                    if let Some(path_var) = &pattern.variable {
                        let path = Path {
                            nodes: sp.nodes,
                            edges: sp.edges,
                        };
                        new_ctx
                            .variables
                            .insert(path_var.clone(), Value::Path(path));
                    }

                    results.push(new_ctx);

                    // shortestPath는 첫 번째 결과만 반환
                    if matches!(path_func, PathFunction::ShortestPath) {
                        return Ok(results);
                    }
                }
            }
        }

        Ok(results)
    }

    /// 노드 패턴과 매칭되는 노드 ID 목록 찾기
    ///
    /// v1.12.0: async로 변경 (evaluate_expression 비동기화)
    async fn find_matching_nodes(
        &self,
        node_pattern: &NodePattern,
        context: &ExecutionContext,
    ) -> Result<Vec<NodeId>> {
        // 변수가 이미 바인딩되어 있으면 그 값 사용
        if let Some(var) = &node_pattern.variable
            && let Some(Value::Node(id)) = context.variables.get(var)
        {
            return Ok(vec![*id]);
        }

        // 라벨로 후보 노드 검색
        let candidates = if let Some(label) = node_pattern.labels.first() {
            self.engine
                .find_nodes_by_label_in_transaction(context.tx_id, label)
                .await?
        } else {
            self.engine
                .all_node_ids_in_transaction(context.tx_id)
                .await?
        };

        // 레이블 및 프로퍼티 필터링
        let mut matched = Vec::new();
        for node_id in candidates {
            if let Some(node) = self
                .engine
                .get_node_in_transaction(context.tx_id, node_id)
                .await?
            {
                // 모든 필수 레이블 검사
                let has_all_labels = node_pattern
                    .labels
                    .iter()
                    .all(|required| node.labels().iter().any(|l| l == required));
                if !has_all_labels {
                    continue;
                }

                // 프로퍼티 검사
                let matches = if let Some(props) = &node_pattern.properties {
                    self.matches_property_pattern(props, &node, context).await?
                } else {
                    true
                };

                if matches {
                    matched.push(node_id);
                }
            }
        }

        Ok(matched)
    }

    /// 패턴에서 엣지 제약 조건 추출 (관계 타입, 최대 깊이)
    fn extract_edge_constraints(&self, pattern: &Pattern) -> (Option<Vec<String>>, Option<usize>) {
        let mut rel_types: Option<Vec<String>> = None;
        let mut max_depth: Option<usize> = None;

        for element in &pattern.elements {
            if let PatternElement::Edge(edge) = element {
                // 관계 타입
                if !edge.rel_types.is_empty() {
                    rel_types = Some(edge.rel_types.clone());
                }

                // 가변 길이에서 최대 깊이 추출
                if let Some(var_len) = &edge.variable_length
                    && let Some(max) = var_len.max
                {
                    max_depth = Some(max as usize);
                }
            }
        }

        (rel_types, max_depth)
    }

    /// WHERE 절에서 인덱스 사용 가능한 조건 추출
    /// n.prop = value 형태의 등호 조건만 지원
    #[allow(dead_code)]
    pub(super) fn extract_index_condition(
        &self,
        expr: &Expression,
        variable: &str,
        label: &str,
    ) -> Option<ExecutorIndexCondition> {
        match expr {
            // 단순 등호 조건: n.prop = value
            Expression::Comparison {
                left,
                op: ComparisonOp::Equal,
                right,
            } => {
                // 좌측이 PropertyAccess이고 변수명이 일치하는 경우
                if let Expression::PropertyAccess(var, prop) = left.as_ref()
                    && var == variable
                {
                    // 우측이 리터럴인 경우에만 인덱스 사용 가능
                    if let Expression::Literal(value) = right.as_ref() {
                        // 해당 라벨+프로퍼티에 인덱스가 있는지 확인
                        if self.has_property_index_cached(label, prop) {
                            return Some(ExecutorIndexCondition {
                                variable: var.clone(),
                                property: prop.clone(),
                                value: value.clone(),
                            });
                        }
                    }
                }
                // 우측이 PropertyAccess인 경우 (value = n.prop)
                if let Expression::PropertyAccess(var, prop) = right.as_ref()
                    && var == variable
                    && let Expression::Literal(value) = left.as_ref()
                    && self.has_property_index_cached(label, prop)
                {
                    return Some(ExecutorIndexCondition {
                        variable: var.clone(),
                        property: prop.clone(),
                        value: value.clone(),
                    });
                }
                None
            }
            // AND 조건: 첫 번째 인덱스 가능 조건 추출
            Expression::Logical {
                left: Some(left),
                op: LogicalOp::And,
                right,
            } => {
                // 좌측에서 인덱스 조건 추출 시도
                if let Some(cond) = self.extract_index_condition(left, variable, label) {
                    return Some(cond);
                }
                // 우측에서 인덱스 조건 추출 시도
                self.extract_index_condition(right, variable, label)
            }
            _ => None,
        }
    }

    /// WHERE 절에서 범위 인덱스 조건 추출
    /// n.prop > value, n.prop < value, n.prop >= value, n.prop <= value 형태 지원
    #[allow(dead_code)]
    pub(super) fn extract_range_index_condition(
        &self,
        expr: &Expression,
        variable: &str,
        label: &str,
    ) -> Option<super::RangeIndexCondition> {
        match expr {
            // 비교 연산자: >, <, >=, <=
            Expression::Comparison { left, op, right } => {
                // n.prop > value
                if let Expression::PropertyAccess(var, prop) = left.as_ref()
                    && var == variable
                    && let Expression::Literal(value) = right.as_ref()
                    && self.has_property_index_cached(label, prop)
                {
                    return self.make_range_condition(var, prop, op, value, false);
                }
                // value < n.prop (역방향)
                if let Expression::PropertyAccess(var, prop) = right.as_ref()
                    && var == variable
                    && let Expression::Literal(value) = left.as_ref()
                    && self.has_property_index_cached(label, prop)
                {
                    return self.make_range_condition(var, prop, op, value, true);
                }
                None
            }
            // AND 조건: 동일 프로퍼티 범위 병합 시도
            Expression::Logical {
                left: Some(left_expr),
                op: LogicalOp::And,
                right: right_expr,
            } => {
                let left_cond = self.extract_range_index_condition(left_expr, variable, label);
                let right_cond = self.extract_range_index_condition(right_expr, variable, label);

                match (left_cond, right_cond) {
                    (Some(c1), Some(c2)) if c1.property == c2.property => {
                        // 동일 프로퍼티면 범위 병합
                        let has_c1_lower = c1.lower.is_some();
                        let has_c1_upper = c1.upper.is_some();
                        Some(super::RangeIndexCondition {
                            variable: c1.variable,
                            property: c1.property,
                            lower: c1.lower.or(c2.lower),
                            lower_inclusive: if has_c1_lower {
                                c1.lower_inclusive
                            } else {
                                c2.lower_inclusive
                            },
                            upper: c1.upper.or(c2.upper),
                            upper_inclusive: if has_c1_upper {
                                c1.upper_inclusive
                            } else {
                                c2.upper_inclusive
                            },
                        })
                    }
                    (Some(c), None) | (None, Some(c)) => Some(c),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// 비교 연산자에서 RangeIndexCondition 생성
    #[allow(dead_code)]
    fn make_range_condition(
        &self,
        var: &str,
        prop: &str,
        op: &ComparisonOp,
        value: &Value,
        inverted: bool,
    ) -> Option<super::RangeIndexCondition> {
        // inverted: true면 value op n.prop (역방향)
        let (lower, lower_inc, upper, upper_inc) = if inverted {
            // value < n.prop → n.prop > value (lower bound, exclusive)
            // value <= n.prop → n.prop >= value (lower bound, inclusive)
            // value > n.prop → n.prop < value (upper bound, exclusive)
            // value >= n.prop → n.prop <= value (upper bound, inclusive)
            match op {
                ComparisonOp::LessThan => (Some(value.clone()), false, None, false),
                ComparisonOp::LessEqual => (Some(value.clone()), true, None, false),
                ComparisonOp::GreaterThan => (None, false, Some(value.clone()), false),
                ComparisonOp::GreaterEqual => (None, false, Some(value.clone()), true),
                _ => return None,
            }
        } else {
            // n.prop > value → lower bound, exclusive
            // n.prop >= value → lower bound, inclusive
            // n.prop < value → upper bound, exclusive
            // n.prop <= value → upper bound, inclusive
            match op {
                ComparisonOp::GreaterThan => (Some(value.clone()), false, None, false),
                ComparisonOp::GreaterEqual => (Some(value.clone()), true, None, false),
                ComparisonOp::LessThan => (None, false, Some(value.clone()), false),
                ComparisonOp::LessEqual => (None, false, Some(value.clone()), true),
                _ => return None,
            }
        };

        Some(super::RangeIndexCondition {
            variable: var.to_string(),
            property: prop.to_string(),
            lower,
            lower_inclusive: lower_inc,
            upper,
            upper_inclusive: upper_inc,
        })
    }

    /// 범위 인덱스 스캔 실행
    ///
    /// 범위 조건에 따라 숫자/문자열 범위 쿼리 실행
    #[expect(dead_code)]
    pub(super) async fn execute_range_index_scan(
        &self,
        label: &str,
        condition: &super::RangeIndexCondition,
    ) -> Result<Vec<NodeId>> {
        // 값 타입에 따라 숫자 또는 문자열 범위 쿼리 선택
        let is_numeric = condition
            .lower
            .as_ref()
            .or(condition.upper.as_ref())
            .map(|v| matches!(v, Value::Integer(_) | Value::Float(_)))
            .unwrap_or(false);

        if is_numeric {
            // 숫자 범위 쿼리
            let min = condition.lower.as_ref().and_then(|v| v.as_float());
            let max = condition.upper.as_ref().and_then(|v| v.as_float());

            self.engine
                .find_nodes_by_numeric_range(
                    label,
                    &condition.property,
                    min,
                    max,
                    condition.lower_inclusive,
                    condition.upper_inclusive,
                )
                .await
        } else {
            // 문자열 범위 쿼리
            let min = condition.lower.as_ref().and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            });
            let max = condition.upper.as_ref().and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            });

            self.engine
                .find_nodes_by_string_range(
                    label,
                    &condition.property,
                    min,
                    max,
                    condition.lower_inclusive,
                    condition.upper_inclusive,
                )
                .await
        }
    }

    /// OPTIONAL MATCH에서 매치 없을 때 여러 패턴의 변수를 NULL로 바인딩
    /// 주의: 이미 바인딩된 변수는 덮어쓰지 않음 (OPTIONAL MATCH는 새 변수만 null)
    pub(super) fn create_null_context_for_patterns(
        &self,
        patterns: &[Pattern],
        context: &ExecutionContext,
    ) -> ExecutionContext {
        let mut new_ctx = context.clone();

        for pattern in patterns {
            // 패턴의 모든 변수를 NULL로 바인딩 (이미 바인딩된 것 제외)
            for part in &pattern.parts {
                if let Some(var) = &part.variable
                    && !new_ctx.variables.contains_key(var)
                {
                    new_ctx.variables.insert(var.clone(), Value::Null);
                }
            }

            // elements에서도 변수 추출
            for element in &pattern.elements {
                match element {
                    PatternElement::Node(node_pattern) => {
                        if let Some(var) = &node_pattern.variable
                            && !new_ctx.variables.contains_key(var)
                        {
                            new_ctx.variables.insert(var.clone(), Value::Null);
                        }
                    }
                    PatternElement::Edge(edge_pattern) => {
                        if let Some(var) = &edge_pattern.variable
                            && !new_ctx.variables.contains_key(var)
                        {
                            new_ctx.variables.insert(var.clone(), Value::Null);
                        }
                    }
                }
            }
        }

        new_ctx
    }

    /// 경로 패턴 매칭 (노드-엣지-노드 체인)
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_path_match(
        &self,
        pattern: &Pattern,
        where_clause: &Option<Expression>,
        context: &ExecutionContext,
        limit: Option<usize>,
    ) -> Result<Vec<ExecutionContext>> {
        // 패턴 요소 추출
        let elements = &pattern.elements;
        if elements.is_empty() {
            return Ok(vec![context.clone()]);
        }

        // 첫 번째 노드로 시작
        let first_node = match &elements[0] {
            PatternElement::Node(n) => n,
            _ => {
                return Err(GraphError::Validation {
                    message: "Pattern must start with a node".to_string(),
                });
            }
        };

        // 첫 번째 노드 후보 검색
        // EXISTS 서브쿼리에서 외부 컨텍스트의 바인딩된 변수 사용
        let candidates = if let Some(var) = &first_node.variable {
            if let Some(Value::Node(node_id)) = context.variables.get(var) {
                // 변수가 이미 바인딩되어 있으면 해당 노드만 후보로 사용
                vec![*node_id]
            } else if let Some(label) = first_node.labels.first() {
                self.engine
                    .find_nodes_by_label_in_transaction(context.tx_id, label)
                    .await?
            } else {
                // 변수는 있지만 레이블이 없으면 모든 노드 검색 (성능 주의)
                self.engine
                    .all_node_ids_in_transaction(context.tx_id)
                    .await?
            }
        } else if let Some(label) = first_node.labels.first() {
            self.engine
                .find_nodes_by_label_in_transaction(context.tx_id, label)
                .await?
        } else {
            // 변수도 없고 레이블도 없으면 모든 노드 검색 (성능 주의)
            self.engine
                .all_node_ids_in_transaction(context.tx_id)
                .await?
        };

        let mut matches = Vec::new();

        for start_node_id in candidates {
            // 시작 노드 필터링
            let start_node = match self
                .engine
                .get_node_in_transaction(context.tx_id, start_node_id)
                .await?
            {
                Some(n) => n,
                None => continue,
            };

            // 모든 필수 레이블 검사
            let has_all_labels = first_node
                .labels
                .iter()
                .all(|required| start_node.labels().iter().any(|l| l == required));
            if !has_all_labels {
                continue;
            }

            // PropertyPattern 평가 (Expression을 Value로 변환하여 비교)
            if let Some(props) = &first_node.properties
                && !self
                    .matches_property_pattern(props, &start_node, context)
                    .await?
            {
                continue;
            }

            // 경로 순회 - 모든 조합 지원
            let mut base_ctx = context.clone();
            if let Some(var) = &first_node.variable {
                base_ctx
                    .variables
                    .insert(var.clone(), Value::Node(start_node_id));
            }

            // 현재 경로의 모든 가능한 컨텍스트들 (각 단계에서 분기 가능)
            // (ExecutionContext, 현재 노드 ID, 경로의 노드 ID들, 경로의 엣지 ID들)
            let mut path_contexts: Vec<(ExecutionContext, u64, Vec<u64>, Vec<u64>)> =
                vec![(base_ctx, start_node_id, vec![start_node_id], vec![])];

            let mut i = 1;
            while i < elements.len() && !path_contexts.is_empty() {
                // 엣지 패턴
                let edge_pattern = match &elements[i] {
                    PatternElement::Edge(e) => e,
                    _ => break,
                };

                // 다음 노드 패턴
                let next_node_pattern = match elements.get(i + 1) {
                    Some(PatternElement::Node(n)) => n,
                    _ => break,
                };

                // 각 현재 컨텍스트에서 가능한 모든 엣지로 분기
                let mut next_path_contexts: Vec<(ExecutionContext, u64, Vec<u64>, Vec<u64>)> =
                    Vec::new();

                for (current_ctx, current_node_id, path_nodes, path_edges) in path_contexts {
                    // 가변 길이 패턴인 경우
                    if edge_pattern.variable_length.is_some() {
                        // BFS 기반 가변 길이 탐색
                        let paths = self
                            .execute_variable_length_expand(
                                current_node_id,
                                edge_pattern,
                                next_node_pattern,
                                &current_ctx,
                            )
                            .await?;

                        for (sub_path, target_node_id) in paths {
                            let mut new_ctx = current_ctx.clone();

                            // 경로 변수 바인딩 - 자기참조 패턴 동일성 검증 포함
                            if let Some(var) = &edge_pattern.variable {
                                if let Some(existing_value) = current_ctx.variables.get(var) {
                                    // Path 비교는 복잡하므로 일단 스킵 (같은 경로 변수 재사용은 드묾)
                                    match existing_value {
                                        Value::Path(_) => continue, // 이미 경로가 바인딩됨 - 스킵
                                        _ => continue,              // 타입 불일치 - 스킵
                                    }
                                } else {
                                    new_ctx
                                        .variables
                                        .insert(var.clone(), Value::Path(sub_path.clone()));
                                }
                            }
                            // 목표 노드 변수 바인딩 - 자기참조 패턴 동일성 검증 포함
                            if let Some(var) = &next_node_pattern.variable {
                                if let Some(existing_value) = current_ctx.variables.get(var) {
                                    match existing_value {
                                        Value::Node(existing_id)
                                            if *existing_id == target_node_id =>
                                        {
                                            // 같은 노드 - OK
                                        }
                                        _ => continue, // 다른 값 또는 타입 불일치 - 스킵
                                    }
                                } else {
                                    new_ctx
                                        .variables
                                        .insert(var.clone(), Value::Node(target_node_id));
                                }
                            }

                            // 전체 경로 정보 업데이트: 기존 경로 + 서브 경로
                            let mut new_path_nodes = path_nodes.clone();
                            let mut new_path_edges = path_edges.clone();
                            // 가변 길이 경로의 노드와 엣지 추가 (첫 노드는 이미 있으므로 스킵)
                            if !sub_path.nodes.is_empty() {
                                new_path_nodes.extend(sub_path.nodes.iter().skip(1));
                            }
                            new_path_edges.extend(sub_path.edges.iter());

                            next_path_contexts.push((
                                new_ctx,
                                target_node_id,
                                new_path_nodes,
                                new_path_edges,
                            ));
                        }
                    } else {
                        // 일반 단일 홉 엣지 검색
                        let edges = self
                            .find_matching_edges(
                                current_node_id,
                                edge_pattern,
                                next_node_pattern,
                                current_ctx.tx_id,
                            )
                            .await?;

                        // 각 매칭 엣지에 대해 새 컨텍스트 생성
                        for (edge_id, target_node_id) in edges {
                            // 타겟 노드 필터링 (라벨 및 속성 검사)
                            if !self
                                .matches_node_pattern(
                                    target_node_id,
                                    next_node_pattern,
                                    &current_ctx,
                                )
                                .await?
                            {
                                continue;
                            }

                            let mut new_ctx = current_ctx.clone();

                            // 변수 바인딩 - 자기참조 패턴 동일성 검증 포함
                            // (p)-[:REL]->(m)<-[:REL]-(p) 같은 패턴에서 p가 같은 노드인지 확인
                            if let Some(var) = &edge_pattern.variable {
                                if let Some(existing_value) = current_ctx.variables.get(var) {
                                    // 이미 바인딩된 변수: 동일성 확인
                                    match existing_value {
                                        Value::Edge(existing_id) if *existing_id == edge_id => {
                                            // 같은 엣지 - OK, 덮어쓰기 불필요
                                        }
                                        _ => continue, // 다른 값 또는 타입 불일치 - 스킵
                                    }
                                } else {
                                    // 새 바인딩
                                    new_ctx.variables.insert(var.clone(), Value::Edge(edge_id));
                                }
                            }
                            if let Some(var) = &next_node_pattern.variable {
                                if let Some(existing_value) = current_ctx.variables.get(var) {
                                    // 이미 바인딩된 변수: 동일성 확인
                                    match existing_value {
                                        Value::Node(existing_id)
                                            if *existing_id == target_node_id =>
                                        {
                                            // 같은 노드 - OK, 덮어쓰기 불필요
                                        }
                                        _ => continue, // 다른 값 또는 타입 불일치 - 스킵
                                    }
                                } else {
                                    // Isomorphism 규칙: 다른 변수가 이미 이 노드를 사용 중인지 확인
                                    // (tom)-[:REL]->(m)<-[:REL]-(coactor)에서 tom != coactor 보장
                                    let node_already_bound = new_ctx.variables.iter().any(|(k, v)| {
                                        k != var && matches!(v, Value::Node(id) if *id == target_node_id)
                                    });
                                    if node_already_bound {
                                        continue; // 다른 변수가 이미 이 노드 사용 중 - 스킵
                                    }
                                    // 새 바인딩
                                    new_ctx
                                        .variables
                                        .insert(var.clone(), Value::Node(target_node_id));
                                }
                            }

                            // 경로 정보 업데이트
                            let mut new_path_nodes = path_nodes.clone();
                            let mut new_path_edges = path_edges.clone();
                            new_path_nodes.push(target_node_id);
                            new_path_edges.push(edge_id);

                            next_path_contexts.push((
                                new_ctx,
                                target_node_id,
                                new_path_nodes,
                                new_path_edges,
                            ));
                        }
                    }
                }

                path_contexts = next_path_contexts;
                i += 2;
            }

            // WHERE 절 평가 후 결과에 추가
            for (mut path_ctx, _, path_nodes, path_edges) in path_contexts {
                // 패턴 경로 변수 바인딩 (p = (a)-[:REL]->(b) 형태)
                if let Some(path_var) = &pattern.variable {
                    let path = Path {
                        nodes: path_nodes,
                        edges: path_edges,
                    };
                    path_ctx
                        .variables
                        .insert(path_var.clone(), Value::Path(path));
                }
                if let Some(where_expr) = where_clause {
                    let result = self.evaluate_expression(where_expr, &path_ctx).await?;
                    if !self.is_truthy(&result) {
                        continue;
                    }
                }
                matches.push(path_ctx);

                // limit 도달 시 조기 종료
                if let Some(limit) = limit
                    && matches.len() >= limit
                {
                    break;
                }
            }

            // 외부 루프도 limit 도달 시 종료
            if let Some(limit) = limit
                && matches.len() >= limit
            {
                break;
            }
        }

        Ok(matches)
    }

    /// 조건에 맞는 엣지 검색
    pub(super) async fn find_matching_edges(
        &self,
        from_node: u64,
        edge_pattern: &EdgePattern,
        _target_pattern: &NodePattern,
        tx_id: TransactionId,
    ) -> Result<Vec<(u64, u64)>> {
        let mut results = Vec::new();

        let edges = self
            .engine
            .get_connected_edges_in_transaction(tx_id, from_node)
            .await?;

        for edge_id in edges {
            if let Some(edge) = self.engine.get_edge_in_transaction(tx_id, edge_id).await? {
                if let Some(rel_type) = edge_pattern.rel_types.first()
                    && edge.relation_type() != rel_type
                {
                    continue;
                }
                // 방향에 따른 필터링
                let matches = match edge_pattern.direction {
                    EdgeDirection::Outgoing => edge.from_node() == from_node,
                    EdgeDirection::Incoming => edge.to_node() == from_node,
                    EdgeDirection::Both => {
                        edge.from_node() == from_node || edge.to_node() == from_node
                    }
                };

                if matches {
                    let target = if edge.from_node() == from_node {
                        edge.to_node()
                    } else {
                        edge.from_node()
                    };
                    results.push((edge_id, target));
                }
            }
        }

        Ok(results)
    }

    /// 기본 최대 홉 수 (무한 루프 방지)
    pub(super) const DEFAULT_MAX_HOPS: u32 = 15;

    /// 가변 길이 관계 탐색 (BFS 기반)
    /// 시작 노드에서 min~max 홉 범위 내의 모든 경로를 찾습니다.
    ///
    /// v1.12.0: async로 변경 (matches_node_pattern 비동기화)
    pub(super) async fn execute_variable_length_expand(
        &self,
        start_node: u64,
        edge_pattern: &EdgePattern,
        target_pattern: &NodePattern,
        context: &ExecutionContext,
    ) -> Result<Vec<(Path, u64)>> {
        let range =
            edge_pattern
                .variable_length
                .as_ref()
                .ok_or_else(|| GraphError::Validation {
                    message: "Variable length range required".to_string(),
                })?;

        let min = range.min;
        let max = range.max.unwrap_or(Self::DEFAULT_MAX_HOPS);

        // BFS를 위한 큐: (현재 노드, 현재 경로, 경로 내 방문 노드 집합)
        let mut queue: VecDeque<(u64, Path, HashSet<u64>)> = VecDeque::new();
        let initial_path = Path::new(start_node);
        let mut initial_visited = HashSet::new();
        initial_visited.insert(start_node);
        queue.push_back((start_node, initial_path, initial_visited));

        let mut results: Vec<(Path, u64)> = Vec::new();

        // min이 0이면 시작 노드도 결과에 포함 (자기 자신)
        if min == 0 {
            let path = Path::new(start_node);
            results.push((path, start_node));
        }

        while let Some((current_node, current_path, path_visited)) = queue.pop_front() {
            let current_depth = current_path.len() as u32;

            // 최대 깊이에 도달하면 더 탐색하지 않음
            if current_depth >= max {
                continue;
            }

            // 현재 노드에서 나가는 엣지 찾기
            let edges = self
                .find_edges_for_variable_length(current_node, edge_pattern, context.tx_id)
                .await?;

            for (edge_id, target_node_id) in edges {
                let next_depth = current_depth + 1;

                // 사이클 방지: 현재 경로 내에서 이미 방문한 노드면 건너뜀
                // (다른 경로에서는 같은 노드 방문 허용)
                if path_visited.contains(&target_node_id) {
                    continue;
                }

                // 새 경로 생성
                let mut new_path = current_path.clone();
                new_path.extend(edge_id, target_node_id);

                // 새 방문 집합 생성 (현재 경로에 대해)
                let mut new_visited = path_visited.clone();
                new_visited.insert(target_node_id);

                // min 이상이면 목표 패턴 검사 후 결과에 추가
                // (중간 노드는 패턴 검사하지 않음 - 경로 탐색 허용)
                if next_depth >= min {
                    // 목표 노드 패턴 검사 (레이블과 속성)
                    if self
                        .matches_node_pattern(target_node_id, target_pattern, context)
                        .await?
                    {
                        results.push((new_path.clone(), target_node_id));
                    }
                }

                // max 미만이면 계속 탐색 (중간 노드 타입과 관계없이)
                if next_depth < max {
                    queue.push_back((target_node_id, new_path, new_visited));
                }
            }
        }

        // Neo4j 호환: 경로 길이 순으로 정렬, 같은 길이면 마지막 노드 ID 순
        results.sort_by(|(path_a, node_a), (path_b, node_b)| {
            path_a
                .len()
                .cmp(&path_b.len())
                .then_with(|| node_a.cmp(node_b))
        });

        Ok(results)
    }

    /// 가변 길이 탐색을 위한 엣지 검색 (관계 타입 없어도 검색 가능)
    pub(super) async fn find_edges_for_variable_length(
        &self,
        from_node: u64,
        edge_pattern: &EdgePattern,
        tx_id: TransactionId,
    ) -> Result<Vec<(u64, u64)>> {
        let mut results = Vec::new();

        let edges = self
            .engine
            .get_connected_edges_in_transaction(tx_id, from_node)
            .await?;

        for edge_id in edges {
            if let Some(edge) = self.engine.get_edge_in_transaction(tx_id, edge_id).await? {
                if let Some(rel_type) = edge_pattern.rel_types.first()
                    && edge.relation_type() != rel_type
                {
                    continue;
                }
                // 방향에 따른 필터링
                let (matches, target) = match edge_pattern.direction {
                    EdgeDirection::Outgoing => {
                        if edge.from_node() == from_node {
                            (true, edge.to_node())
                        } else {
                            (false, 0)
                        }
                    }
                    EdgeDirection::Incoming => {
                        if edge.to_node() == from_node {
                            (true, edge.from_node())
                        } else {
                            (false, 0)
                        }
                    }
                    EdgeDirection::Both => {
                        if edge.from_node() == from_node {
                            (true, edge.to_node())
                        } else if edge.to_node() == from_node {
                            (true, edge.from_node())
                        } else {
                            (false, 0)
                        }
                    }
                };

                if matches {
                    results.push((edge_id, target));
                }
            }
        }

        Ok(results)
    }

    /// 노드가 패턴에 맞는지 검사 (레이블과 속성)
    ///
    /// v1.12.0: async로 변경 (evaluate_expression 비동기화)
    pub(super) async fn matches_node_pattern(
        &self,
        node_id: u64,
        pattern: &NodePattern,
        context: &ExecutionContext,
    ) -> Result<bool> {
        // 레이블 필터가 없으면 모든 노드 매칭
        if pattern.labels.is_empty() && pattern.properties.is_none() {
            return Ok(true);
        }

        let node = match self
            .engine
            .get_node_in_transaction(context.tx_id, node_id)
            .await?
        {
            Some(n) => n,
            None => return Ok(false),
        };

        // 레이블 검사 - 모든 필수 레이블이 있어야 함
        for required_label in &pattern.labels {
            let has_label = node.labels().iter().any(|l| l == required_label);
            if !has_label {
                return Ok(false);
            }
        }

        // PropertyPattern 평가 (Expression을 Value로 변환하여 비교)
        if let Some(props) = &pattern.properties
            && !self.matches_property_pattern(props, &node, context).await?
        {
            return Ok(false);
        }

        Ok(true)
    }
}
