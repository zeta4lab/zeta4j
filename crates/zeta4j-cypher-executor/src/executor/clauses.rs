// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Cypher 절 실행 모듈
//!
//! WITH, UNWIND, CALL, RETURN 절 처리
//!
//! # v1.12.0 비동기 API 통일
//!
//! evaluate_expression() 비동기화에 따라 모든 관련 메서드를 async로 변경

use super::{ExecutionContext, QueryExecutor};
use zeta4j_cypher_core::ast::*;
use zeta4j_graph::core::Value;
use zeta4j_graph::{GraphError, Result};

impl QueryExecutor {
    /// UNWIND 절 실행: 리스트를 행으로 펼침
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_unwind(
        &self,
        clause: &UnwindClause,
        contexts: Vec<ExecutionContext>,
    ) -> Result<Vec<ExecutionContext>> {
        let mut result = Vec::new();

        for ctx in &contexts {
            // 표현식 평가 (리스트여야 함)
            let list_value = self.evaluate_expression(&clause.expr, ctx).await?;

            match list_value {
                Value::Array(items) => {
                    // 각 아이템에 대해 새 컨텍스트 생성
                    for item in items {
                        let mut new_ctx = ctx.clone();
                        new_ctx.variables.insert(clause.alias.clone(), item);
                        result.push(new_ctx);
                    }
                }
                Value::Null => {
                    // NULL은 빈 결과 (행 제거)
                }
                _ => {
                    // 리스트가 아닌 경우 단일 값으로 처리
                    let mut new_ctx = ctx.clone();
                    new_ctx.variables.insert(clause.alias.clone(), list_value);
                    result.push(new_ctx);
                }
            }
        }

        Ok(result)
    }

    /// CALL 서브쿼리 실행
    /// CALL { subquery } - 각 입력 컨텍스트에서 서브쿼리 실행 후 결과 병합
    ///
    /// v1.12.0: async로 변경
    #[async_recursion::async_recursion]
    pub(super) async fn execute_call_subquery(
        &self,
        call: &CallSubquery,
        contexts: Vec<ExecutionContext>,
    ) -> Result<Vec<ExecutionContext>> {
        let mut result = Vec::new();

        for ctx in contexts {
            // 서브쿼리 실행을 위한 초기 컨텍스트: 외부 변수 포함
            let mut subquery_contexts = vec![ctx.clone()];

            // 서브쿼리의 각 statement 실행
            for (stmt_idx, stmt) in call.subquery.statements.iter().enumerate() {
                match stmt {
                    Statement::Reading(clause) => {
                        let mut next_contexts = Vec::new();
                        for sub_ctx in subquery_contexts {
                            let matches = self.execute_reading(clause, &sub_ctx).await?;
                            next_contexts.extend(matches);
                        }
                        subquery_contexts = next_contexts;
                    }
                    Statement::Updating(clause) => {
                        for sub_ctx in &mut subquery_contexts {
                            self.execute_updating(clause, sub_ctx).await?;
                        }
                    }
                    Statement::With(clause) => {
                        subquery_contexts = self.execute_with(clause, subquery_contexts).await?;
                    }
                    Statement::Unwind(clause) => {
                        subquery_contexts = self.execute_unwind(clause, subquery_contexts).await?;
                    }
                    Statement::Call(nested_call) => {
                        // 중첩 CALL 서브쿼리 지원
                        subquery_contexts = self
                            .execute_call_subquery(nested_call, subquery_contexts)
                            .await?;
                    }
                    Statement::Return(return_clause) => {
                        // 서브쿼리 RETURN: 집계 함수 여부에 따라 처리 방식 결정
                        let has_agg = self.has_aggregate_in_return(return_clause);
                        if has_agg {
                            // 집계 함수가 있는 경우: 그룹화 후 집계 실행
                            // 중요: 빈 컨텍스트에서도 집계 결과가 반환되어야 함 (collect → [], count → 0 등)
                            let (aggregated_rows, _) = self
                                .execute_aggregated_return(return_clause, &subquery_contexts)
                                .await?;

                            // 집계 결과가 비어있으면 기본 집계 결과 생성 (Neo4j 호환)
                            let rows_to_process = if aggregated_rows.is_empty() {
                                // 빈 컨텍스트에서 집계 결과 생성
                                let (default_row, _) =
                                    self.execute_aggregated_return(return_clause, &[]).await?;
                                default_row
                            } else {
                                aggregated_rows
                            };

                            // 집계 결과를 외부 컨텍스트에 병합 (보통 1행)
                            for row in rows_to_process {
                                let mut merged_ctx = ctx.clone();

                                // RETURN 항목의 alias로 바인딩
                                for (i, item) in return_clause.items.iter().enumerate() {
                                    if let ReturnItem::Expression { expr, alias } = item {
                                        let var_name =
                                            alias.clone().unwrap_or_else(|| match expr {
                                                Expression::Variable(name) => name.clone(),
                                                Expression::PropertyAccess(var, prop) => {
                                                    format!("{}.{}", var, prop)
                                                }
                                                Expression::Aggregate(agg) => {
                                                    format!("{:?}", agg.function)
                                                }
                                                _ => format!("col{}", i),
                                            });
                                        if i < row.len() {
                                            merged_ctx.variables.insert(var_name, row[i].clone());
                                        }
                                    }
                                }
                                result.push(merged_ctx);
                            }
                        } else {
                            // 집계 함수가 없는 경우: ORDER BY/SKIP/LIMIT 적용 후 처리
                            let mut sorted_contexts = subquery_contexts.clone();

                            // ORDER BY 적용
                            if let Some(order_items) = &return_clause.order_by {
                                // ORDER BY 정렬을 위해 값들을 미리 평가
                                let mut indexed_contexts: Vec<(usize, Vec<Value>)> = Vec::new();
                                for (idx, sub_ctx) in sorted_contexts.iter().enumerate() {
                                    let mut values = Vec::new();
                                    for item in order_items {
                                        let val = self
                                            .evaluate_expression(&item.expr, sub_ctx)
                                            .await
                                            .unwrap_or(Value::Null);
                                        values.push(val);
                                    }
                                    indexed_contexts.push((idx, values));
                                }

                                // 평가된 값으로 정렬
                                indexed_contexts.sort_by(|(_, vals_a), (_, vals_b)| {
                                    for (i, item) in order_items.iter().enumerate() {
                                        let val_a = vals_a.get(i).cloned().unwrap_or(Value::Null);
                                        let val_b = vals_b.get(i).cloned().unwrap_or(Value::Null);

                                        let ordering = self.compare_values_total(&val_a, &val_b);
                                        let ordering = if item.ascending {
                                            ordering
                                        } else {
                                            ordering.reverse()
                                        };

                                        if ordering != std::cmp::Ordering::Equal {
                                            return ordering;
                                        }
                                    }
                                    std::cmp::Ordering::Equal
                                });

                                // 정렬된 순서로 컨텍스트 재배열
                                let original_contexts = sorted_contexts.clone();
                                sorted_contexts = indexed_contexts
                                    .iter()
                                    .map(|(idx, _)| original_contexts[*idx].clone())
                                    .collect();
                            }

                            // SKIP 적용
                            if let Some(skip) = return_clause.skip {
                                sorted_contexts =
                                    sorted_contexts.into_iter().skip(skip as usize).collect();
                            }

                            // LIMIT 적용
                            if let Some(limit) = return_clause.limit {
                                sorted_contexts =
                                    sorted_contexts.into_iter().take(limit as usize).collect();
                            }

                            for sub_ctx in &sorted_contexts {
                                let mut merged_ctx = ctx.clone();

                                // RETURN 항목에서 alias가 있는 것들을 외부 컨텍스트에 바인딩
                                for item in &return_clause.items {
                                    match item {
                                        ReturnItem::Star => {
                                            // RETURN * - 서브쿼리의 모든 변수를 외부로 전달
                                            for (var_name, value) in &sub_ctx.variables {
                                                merged_ctx
                                                    .variables
                                                    .insert(var_name.clone(), value.clone());
                                            }
                                        }
                                        ReturnItem::Expression { expr, alias } => {
                                            let value =
                                                self.evaluate_expression(expr, sub_ctx).await?;
                                            // alias가 있으면 해당 이름으로, 없으면 변수명 또는 표현식으로
                                            let var_name =
                                                alias.clone().unwrap_or_else(|| match expr {
                                                    Expression::Variable(name) => name.clone(),
                                                    Expression::PropertyAccess(var, prop) => {
                                                        format!("{}.{}", var, prop)
                                                    }
                                                    _ => format!("{:?}", expr),
                                                });
                                            merged_ctx.variables.insert(var_name, value);
                                        }
                                    }
                                }
                                result.push(merged_ctx);
                            }
                        }
                        // RETURN 처리 후 다음 statement 무시
                        break;
                    }
                    Statement::Index(_) => {
                        // Index 명령은 서브쿼리 내에서 지원하지 않음
                        return Err(GraphError::Parse {
                            message: "Index commands are not supported within CALL subqueries"
                                .to_string(),
                        });
                    }
                    Statement::Schema(_) => {
                        // Schema 명령은 서브쿼리 내에서 지원하지 않음
                        return Err(GraphError::Parse {
                            message: "Schema commands are not supported within CALL subqueries"
                                .to_string(),
                        });
                    }
                    Statement::Constraint(_) => {
                        // Constraint 명령은 서브쿼리 내에서 지원하지 않음
                        return Err(GraphError::Parse {
                            message: "Constraint commands are not supported within CALL subqueries"
                                .to_string(),
                        });
                    }
                    Statement::Admin(_) => {
                        // Admin 명령은 서브쿼리 내에서 지원하지 않음
                        return Err(GraphError::Parse {
                            message: "Admin commands are not supported within CALL subqueries"
                                .to_string(),
                        });
                    }
                    Statement::Procedure(proc_call) => {
                        // 중첩된 프로시저 호출
                        subquery_contexts =
                            self.execute_procedure(proc_call, subquery_contexts).await?;
                    }
                    Statement::Finish => {
                        // FINISH: 결과 없이 종료 (서브쿼리 내에서는 빈 결과 반환)
                        break;
                    }
                    Statement::Let(let_clause) => {
                        // LET: 변수 바인딩
                        for sub_ctx in &mut subquery_contexts {
                            let value = self
                                .evaluate_expression(&let_clause.expression, sub_ctx)
                                .await?;
                            sub_ctx.variables.insert(let_clause.variable.clone(), value);
                        }
                    }
                    Statement::Use(_) => {
                        // USE: 서브쿼리 내에서 데이터베이스 전환 불가
                        return Err(GraphError::Parse {
                            message: "USE is not supported within CALL subqueries".to_string(),
                        });
                    }
                }

                // 빈 컨텍스트 처리:
                // - RETURN이 있으면 집계 함수 처리 가능 (collect→[], count→0)
                // - RETURN이 없으면 더 이상 처리할 것이 없으므로 break
                if subquery_contexts.is_empty() {
                    // 남은 statements 중 RETURN이 있는지 확인
                    let remaining = &call.subquery.statements[stmt_idx + 1..];
                    let has_remaining_return =
                        remaining.iter().any(|s| matches!(s, Statement::Return(_)));
                    if !has_remaining_return {
                        break;
                    }
                }
            }

            // 서브쿼리에 RETURN이 없는 경우: 원래 컨텍스트 유지
            if !call
                .subquery
                .statements
                .iter()
                .any(|s| matches!(s, Statement::Return(_)))
            {
                // RETURN 없이 종료된 경우 (예: CALL { CREATE ... })
                // 각 서브쿼리 컨텍스트를 외부에 병합
                for sub_ctx in subquery_contexts {
                    let mut merged_ctx = ctx.clone();
                    // 서브쿼리에서 새로 바인딩된 변수만 병합
                    for (var_name, value) in sub_ctx.variables {
                        if !ctx.variables.contains_key(&var_name) {
                            merged_ctx.variables.insert(var_name, value);
                        }
                    }
                    result.push(merged_ctx);
                }
            }
        }

        Ok(result)
    }

    /// RETURN 절 항목 평가하여 결과 행 생성
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_return_items(
        &self,
        clause: &ReturnClause,
        context: &ExecutionContext,
    ) -> Result<Vec<Value>> {
        let mut result_row = Vec::new();

        for item in &clause.items {
            match item {
                ReturnItem::Expression { expr, .. } => {
                    let val = self.evaluate_expression(expr, context).await?;
                    result_row.push(val);
                }
                ReturnItem::Star => {
                    // 모든 변수를 결과에 포함
                    for (_, value) in &context.variables {
                        result_row.push(value.clone());
                    }
                }
            }
        }

        Ok(result_row)
    }

    /// WITH 절 실행: 컨텍스트 변환 및 필터링
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_with(
        &self,
        clause: &WithClause,
        contexts: Vec<ExecutionContext>,
    ) -> Result<Vec<ExecutionContext>> {
        let mut new_contexts: Vec<ExecutionContext>;

        // 1. 집계 함수 포함 여부 확인
        if self.has_aggregate_in_with(clause) {
            // 집계가 있는 경우: 그룹화 및 집계 처리
            new_contexts = self.execute_with_aggregation(clause, &contexts).await?;
        } else {
            // 집계가 없는 경우: 기존 로직
            new_contexts = Vec::new();
            for ctx in &contexts {
                let mut new_ctx = ExecutionContext::new(ctx.tx_id);

                // WITH 항목들을 평가하여 새 변수 바인딩 생성
                for item in &clause.items {
                    match item {
                        ReturnItem::Expression { expr, alias } => {
                            let val = self.evaluate_expression(expr, ctx).await?;
                            // alias가 있으면 그것을 사용, 없으면 표현식에서 변수명 추출
                            let var_name = if let Some(name) = alias {
                                name.clone()
                            } else {
                                self.extract_variable_name(expr)
                            };
                            new_ctx.variables.insert(var_name, val);
                        }
                        ReturnItem::Star => {
                            // * 는 모든 변수를 그대로 전달
                            new_ctx.variables = ctx.variables.clone();
                        }
                    }
                }

                new_contexts.push(new_ctx);
            }
        }

        // 2. DISTINCT 적용
        if clause.distinct {
            new_contexts = self.apply_distinct_contexts(new_contexts);
        }

        // 3. ORDER BY 적용
        if let Some(order_items) = &clause.order_by {
            // ORDER BY 정렬을 위해 값들을 미리 평가
            let mut indexed_contexts: Vec<(usize, Vec<Value>)> = Vec::new();
            for (idx, ctx) in new_contexts.iter().enumerate() {
                let mut values = Vec::new();
                for item in order_items {
                    let val = self
                        .evaluate_expression(&item.expr, ctx)
                        .await
                        .unwrap_or(Value::Null);
                    values.push(val);
                }
                indexed_contexts.push((idx, values));
            }

            // 평가된 값으로 정렬
            indexed_contexts.sort_by(|(idx_a, vals_a), (idx_b, vals_b)| {
                for (i, item) in order_items.iter().enumerate() {
                    let val_a = vals_a.get(i).cloned().unwrap_or(Value::Null);
                    let val_b = vals_b.get(i).cloned().unwrap_or(Value::Null);

                    // 전체 순서 비교 사용 (total ordering)
                    let ordering = self.compare_values_total(&val_a, &val_b);

                    let ordering = if item.ascending {
                        ordering
                    } else {
                        ordering.reverse()
                    };

                    if ordering != std::cmp::Ordering::Equal {
                        return ordering;
                    }
                }
                // 동일 값일 때 노드/엣지 ID로 tie-breaking (Neo4j 호환)
                self.compare_context_ids(&new_contexts[*idx_a], &new_contexts[*idx_b])
            });

            // 정렬된 순서로 컨텍스트 재배열
            let original_contexts = new_contexts.clone();
            new_contexts = indexed_contexts
                .iter()
                .map(|(idx, _)| original_contexts[*idx].clone())
                .collect();
        }

        // 4. SKIP 적용
        if let Some(skip) = clause.skip {
            new_contexts = new_contexts.into_iter().skip(skip as usize).collect();
        }

        // 5. LIMIT 적용
        if let Some(limit) = clause.limit {
            new_contexts = new_contexts.into_iter().take(limit as usize).collect();
        }

        // 6. WHERE 필터링
        if let Some(where_expr) = &clause.where_clause {
            let mut filtered = Vec::new();
            for ctx in new_contexts {
                match self.evaluate_expression(where_expr, &ctx).await {
                    Ok(val) if self.is_truthy(&val) => filtered.push(ctx),
                    _ => {}
                }
            }
            new_contexts = filtered;
        }

        Ok(new_contexts)
    }

    /// DISTINCT, ORDER BY, SKIP, LIMIT 적용
    /// 근본적 수정: rows와 contexts를 쌍으로 묶어 함께 처리
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn apply_order_skip_limit(
        &self,
        rows: Vec<Vec<Value>>,
        clause: &ReturnClause,
        contexts: &[ExecutionContext],
    ) -> Result<Vec<Vec<Value>>> {
        // rows와 contexts를 쌍으로 묶음
        let mut paired: Vec<(Vec<Value>, ExecutionContext)> =
            rows.into_iter().zip(contexts.iter().cloned()).collect();

        // 1. ORDER BY 먼저 적용 (DISTINCT 전에 정렬해야 결정론적 결과)
        if let Some(order_items) = &clause.order_by {
            // ORDER BY 정렬을 위해 값들을 미리 평가
            let mut indexed_pairs: Vec<(usize, Vec<Value>)> = Vec::new();
            for (idx, (_, ctx)) in paired.iter().enumerate() {
                let mut values = Vec::new();
                for item in order_items {
                    let val = self
                        .evaluate_expression(&item.expr, ctx)
                        .await
                        .unwrap_or(Value::Null);
                    values.push(val);
                }
                indexed_pairs.push((idx, values));
            }

            // 평가된 값으로 정렬
            indexed_pairs.sort_by(|(idx_a, vals_a), (idx_b, vals_b)| {
                for (i, item) in order_items.iter().enumerate() {
                    let val_a = vals_a.get(i).cloned().unwrap_or(Value::Null);
                    let val_b = vals_b.get(i).cloned().unwrap_or(Value::Null);

                    let ordering = self.compare_values_total(&val_a, &val_b);
                    let ordering = if item.ascending {
                        ordering
                    } else {
                        ordering.reverse()
                    };

                    if ordering != std::cmp::Ordering::Equal {
                        return ordering;
                    }
                }
                // 동일 값일 때 tie-breaking: 1) 노드/엣지 ID, 2) row 값
                let id_cmp = self.compare_context_ids(&paired[*idx_a].1, &paired[*idx_b].1);
                if id_cmp != std::cmp::Ordering::Equal {
                    return id_cmp;
                }
                // row 값으로 tie-breaking (집계 결과용)
                self.compare_rows(&paired[*idx_a].0, &paired[*idx_b].0)
            });

            // 정렬된 순서로 재배열
            let original_paired = paired.clone();
            paired = indexed_pairs
                .iter()
                .map(|(idx, _)| original_paired[*idx].clone())
                .collect();
        }

        // 2. DISTINCT 적용 (정렬된 상태에서 첫 번째 것 유지)
        if clause.distinct {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            paired.retain(|(row, _)| {
                let key = self.row_to_key(row);
                seen.insert(key)
            });
        }

        // 3. SKIP 적용
        if let Some(skip) = clause.skip {
            paired = paired.into_iter().skip(skip as usize).collect();
        }

        // 4. LIMIT 적용
        if let Some(limit) = clause.limit {
            paired = paired.into_iter().take(limit as usize).collect();
        }

        // rows만 추출하여 반환
        Ok(paired.into_iter().map(|(row, _)| row).collect())
    }

    /// 표현식에서 변수명 추출 (alias 없는 경우)
    pub(super) fn extract_variable_name(&self, expr: &Expression) -> String {
        self.infer_column_name(expr)
    }

    /// 표현식에서 컬럼 이름 추론
    /// openCypher 호환: 표현식 타입에 따라 적절한 컬럼 이름 생성
    pub(super) fn infer_column_name(&self, expr: &Expression) -> String {
        match expr {
            // 변수: 변수명 그대로
            Expression::Variable(name) => name.clone(),

            // 프로퍼티 접근: variable.property
            Expression::PropertyAccess(var, prop) => format!("{}.{}", var, prop),

            // 리터럴: 값 자체를 문자열로
            Expression::Literal(val) => self.value_to_column_name(val),

            // 파라미터: $name
            Expression::Parameter(name) => format!("${}", name),

            // 스칼라 함수 호출: function_name(...)
            Expression::FunctionCall(call) => {
                let func_name = format!("{:?}", call.function).to_lowercase();
                if call.arguments.is_empty() {
                    format!("{}()", func_name)
                } else {
                    let arg_name = self.infer_column_name(&call.arguments[0]);
                    format!("{}({})", func_name, arg_name)
                }
            }

            // 집계 함수: count(...), sum(...) 등
            Expression::Aggregate(agg) => {
                let func_name = format!("{:?}", agg.function).to_lowercase();
                if let Some(ref arg) = agg.argument {
                    let arg_name = self.infer_column_name(arg);
                    if agg.distinct {
                        format!("{}(DISTINCT {})", func_name, arg_name)
                    } else {
                        format!("{}({})", func_name, arg_name)
                    }
                } else {
                    // COUNT(*) 의 경우
                    if agg.distinct {
                        format!("{}(DISTINCT *)", func_name)
                    } else {
                        format!("{}(*)", func_name)
                    }
                }
            }

            // 비교 연산: 표현식
            Expression::Comparison { .. } => "expression".to_string(),

            // 논리 연산: 표현식
            Expression::Logical { .. } => "expression".to_string(),

            // 리스트/맵 관련
            Expression::List(_) => "list".to_string(),
            Expression::Map(_) => "map".to_string(),

            // CASE 표현식
            Expression::Case(_) => "case".to_string(),

            // EXISTS 서브쿼리
            Expression::Exists(_) => "exists".to_string(),

            // IS NULL / IS NOT NULL
            Expression::IsNull { expr, negated } => {
                let inner = self.infer_column_name(expr);
                if *negated {
                    format!("{} IS NOT NULL", inner)
                } else {
                    format!("{} IS NULL", inner)
                }
            }

            // 산술 연산
            Expression::Arithmetic { left, op, right } => {
                let l = self.infer_column_name(left);
                let r = self.infer_column_name(right);
                let op_str = match op {
                    ArithmeticOp::Add => "+",
                    ArithmeticOp::Subtract => "-",
                    ArithmeticOp::Multiply => "*",
                    ArithmeticOp::Divide => "/",
                    ArithmeticOp::Modulo => "%",
                    ArithmeticOp::Power => "^",
                };
                format!("{} {} {}", l, op_str, r)
            }

            // 리스트 컴프리헨션
            Expression::ListComprehension(comp) => {
                format!("[{} IN ...]", comp.variable)
            }

            // 인덱스 접근
            Expression::Subscript { expr, index } => {
                let base = self.infer_column_name(expr);
                let idx = self.infer_column_name(index);
                format!("{}[{}]", base, idx)
            }

            // 범위 슬라이싱
            Expression::SubscriptRange { expr, start, end } => {
                let base = self.infer_column_name(expr);
                let start_str = start
                    .as_ref()
                    .map(|e| self.infer_column_name(e))
                    .unwrap_or_default();
                let end_str = end
                    .as_ref()
                    .map(|e| self.infer_column_name(e))
                    .unwrap_or_default();
                format!("{}[{}..{}]", base, start_str, end_str)
            }

            // Phase 3: 벡터 리터럴
            Expression::Vector(vec_expr) => match vec_expr {
                VectorExpression::Literal(elements) => format!("vector([{}])", elements.len()),
                VectorExpression::Parameter(name) => format!("vector(${})", name),
            },
        }
    }

    /// Value를 컬럼 이름 문자열로 변환
    fn value_to_column_name(&self, val: &Value) -> String {
        match val {
            Value::Null => "NULL".to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Integer(i) => i.to_string(),
            Value::Float(f) => format!("{}", f),
            Value::String(s) => format!("\"{}\"", s),
            Value::Array(_) => "list".to_string(),
            Value::Map(_) => "map".to_string(),
            _ => "value".to_string(),
        }
    }

    /// RETURN 절에서 컬럼 이름 목록 추출
    pub(super) fn extract_column_names(
        &self,
        clause: &ReturnClause,
        context: &ExecutionContext,
    ) -> Vec<String> {
        let mut columns = Vec::new();

        for item in &clause.items {
            match item {
                ReturnItem::Expression { expr, alias } => {
                    // alias가 있으면 alias 사용, 없으면 표현식에서 추론
                    let col_name = if let Some(name) = alias {
                        name.clone()
                    } else {
                        self.infer_column_name(expr)
                    };
                    columns.push(col_name);
                }
                ReturnItem::Star => {
                    // RETURN * 의 경우 모든 변수명을 컬럼으로
                    for var_name in context.variables.keys() {
                        columns.push(var_name.clone());
                    }
                }
            }
        }

        columns
    }

    /// 컨텍스트의 노드/엣지 ID를 비교하여 tie-breaking (Neo4j 호환)
    /// ORDER BY에서 동일 값일 때 결정론적 순서 보장
    fn compare_context_ids(
        &self,
        ctx_a: &ExecutionContext,
        ctx_b: &ExecutionContext,
    ) -> std::cmp::Ordering {
        // 컨텍스트의 변수들을 알파벳 순으로 정렬하여 일관된 비교
        let mut vars_a: Vec<_> = ctx_a.variables.iter().collect();
        let mut vars_b: Vec<_> = ctx_b.variables.iter().collect();
        vars_a.sort_by_key(|(k, _)| *k);
        vars_b.sort_by_key(|(k, _)| *k);

        // 1단계: 노드/엣지 ID로 비교
        for ((_, val_a), (_, val_b)) in vars_a.iter().zip(vars_b.iter()) {
            let id_a = self.extract_entity_id(val_a);
            let id_b = self.extract_entity_id(val_b);

            match (id_a, id_b) {
                (Some(a), Some(b)) => {
                    let cmp = a.cmp(&b);
                    if cmp != std::cmp::Ordering::Equal {
                        return cmp;
                    }
                }
                (Some(_), None) => return std::cmp::Ordering::Less,
                (None, Some(_)) => return std::cmp::Ordering::Greater,
                (None, None) => continue,
            }
        }

        // 2단계: 노드/엣지 ID가 없으면 문자열 값으로 비교 (집계 결과용)
        for ((_, val_a), (_, val_b)) in vars_a.iter().zip(vars_b.iter()) {
            if let (Value::String(s_a), Value::String(s_b)) = (val_a, val_b) {
                let cmp = s_a.cmp(s_b);
                if cmp != std::cmp::Ordering::Equal {
                    return cmp;
                }
            }
        }

        // 3단계: 정수 값으로 비교
        for ((_, val_a), (_, val_b)) in vars_a.iter().zip(vars_b.iter()) {
            if let (Value::Integer(i_a), Value::Integer(i_b)) = (val_a, val_b) {
                let cmp = i_a.cmp(i_b);
                if cmp != std::cmp::Ordering::Equal {
                    return cmp;
                }
            }
        }

        std::cmp::Ordering::Equal
    }

    /// Value에서 노드/엣지 ID 추출
    fn extract_entity_id(&self, value: &Value) -> Option<u64> {
        match value {
            Value::Node(id) => Some(*id),
            Value::Edge(id) => Some(*id),
            Value::Integer(id) if *id >= 0 => Some(*id as u64),
            _ => None,
        }
    }

    /// row 값으로 tie-breaking (집계 결과용)
    /// 문자열 → 정수 → 실수 순으로 비교
    fn compare_rows(&self, row_a: &[Value], row_b: &[Value]) -> std::cmp::Ordering {
        for (val_a, val_b) in row_a.iter().zip(row_b.iter()) {
            let cmp = self.compare_values_total(val_a, val_b);
            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
        }
        std::cmp::Ordering::Equal
    }

    /// 프로시저 호출 실행
    ///
    /// CALL db.index.vector.queryNodes('idx', 10, $vec) YIELD node, score
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_procedure(
        &self,
        proc_call: &ProcedureCall,
        contexts: Vec<ExecutionContext>,
    ) -> Result<Vec<ExecutionContext>> {
        // 프로시저 레지스트리에서 프로시저 조회 (executor에 저장된 registry 사용)
        let proc = self
            .procedure_registry
            .get(&proc_call.name)
            .ok_or_else(|| GraphError::Parse {
                message: format!("Unknown procedure: {}", proc_call.name),
            })?;

        let mut result = Vec::new();

        // 각 컨텍스트에 대해 프로시저 실행
        for ctx in contexts {
            // 인수 평가
            let mut args = Vec::new();
            for arg in &proc_call.arguments {
                let val = self.evaluate_expression(arg, &ctx).await?;
                args.push(val);
            }

            // 프로시저 실행
            let proc_result =
                proc.execute(args, None, &self.engine)
                    .map_err(|e| GraphError::Validation {
                        message: format!("Procedure execution failed: {}", e),
                    })?;

            // 결과를 컨텍스트로 변환
            for row in proc_result.rows {
                let mut new_ctx = ctx.clone();

                // YIELD 절이 있으면 해당 변수에 바인딩
                if let Some(yields) = &proc_call.yields {
                    for (i, yield_item) in yields.iter().enumerate() {
                        if i < row.len() {
                            let var_name = yield_item
                                .alias
                                .as_ref()
                                .unwrap_or(&yield_item.variable)
                                .clone();
                            new_ctx.variables.insert(var_name, row[i].clone());
                        }
                    }
                } else {
                    // YIELD 없으면 컬럼 이름으로 바인딩
                    for (i, col_name) in proc_result.columns.iter().enumerate() {
                        if i < row.len() {
                            new_ctx.variables.insert(col_name.clone(), row[i].clone());
                        }
                    }
                }

                result.push(new_ctx);
            }
        }

        Ok(result)
    }
}
