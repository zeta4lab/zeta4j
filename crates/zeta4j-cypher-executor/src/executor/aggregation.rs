//! 집계 함수 실행 모듈
//!
//! RETURN 절에서 집계 함수(COUNT, SUM, AVG, MIN, MAX, COLLECT)를 처리합니다.
//! 그룹화(GROUP BY 의미론)도 이 모듈에서 처리합니다.
//!
//! # v1.12.0 비동기 API 통일
//!
//! evaluate_expression() 비동기화에 따라 모든 집계 관련 메서드를 async로 변경

use super::{ExecutionContext, QueryExecutor};
use zeta4j_cypher_core::ast::{
    AggregateFunction, AggregateFunctionCall, ArithmeticOp, Expression, ReturnClause, ReturnItem,
    WithClause,
};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    /// 표현식에 집계 함수가 포함되어 있는지 확인
    pub(super) fn contains_aggregate(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Aggregate(_) => true,
            Expression::Comparison { left, right, .. } => {
                self.contains_aggregate(left) || self.contains_aggregate(right)
            }
            Expression::Logical { left, right, .. } => {
                left.as_ref().is_some_and(|l| self.contains_aggregate(l))
                    || self.contains_aggregate(right)
            }
            Expression::IsNull { expr, .. } => self.contains_aggregate(expr),
            Expression::List(exprs) => exprs.iter().any(|e| self.contains_aggregate(e)),
            Expression::Map(pairs) => pairs.iter().any(|(_, e)| self.contains_aggregate(e)),
            // 인덱스 접근 및 슬라이싱도 내부 표현식 검사 (Issue #70)
            Expression::Subscript { expr, index } => {
                self.contains_aggregate(expr) || self.contains_aggregate(index)
            }
            Expression::SubscriptRange { expr, start, end } => {
                self.contains_aggregate(expr)
                    || start.as_ref().is_some_and(|e| self.contains_aggregate(e))
                    || end.as_ref().is_some_and(|e| self.contains_aggregate(e))
            }
            Expression::Arithmetic { left, right, .. } => {
                self.contains_aggregate(left) || self.contains_aggregate(right)
            }
            Expression::FunctionCall(call) => {
                call.arguments.iter().any(|e| self.contains_aggregate(e))
            }
            Expression::Case(case_expr) => {
                case_expr
                    .operand
                    .as_ref()
                    .is_some_and(|e| self.contains_aggregate(e))
                    || case_expr.when_clauses.iter().any(|w| {
                        self.contains_aggregate(&w.condition) || self.contains_aggregate(&w.result)
                    })
                    || case_expr
                        .else_result
                        .as_ref()
                        .is_some_and(|e| self.contains_aggregate(e))
            }
            Expression::ListComprehension(comp) => {
                self.contains_aggregate(&comp.list_expr)
                    || comp
                        .filter
                        .as_ref()
                        .is_some_and(|e| self.contains_aggregate(e))
                    || comp
                        .map_expr
                        .as_ref()
                        .is_some_and(|e| self.contains_aggregate(e))
            }
            _ => false,
        }
    }

    /// 집계가 포함된 RETURN 절 실행 (그룹화 처리)
    /// 반환: (결과 행들, 각 행에 대응하는 컨텍스트들)
    /// 컨텍스트에는 alias 값이 포함되어 ORDER BY에서 사용 가능
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_aggregated_return(
        &self,
        clause: &ReturnClause,
        contexts: &[ExecutionContext],
    ) -> Result<(Vec<Vec<Value>>, Vec<ExecutionContext>)> {
        // 1. RETURN 항목을 집계 vs 비집계로 분류
        let (group_key_indices, _aggregate_indices) = self.classify_return_items(clause);

        // 2. 그룹 키가 없으면 전체를 하나의 그룹으로 처리
        if group_key_indices.is_empty() {
            // 모든 컨텍스트를 하나의 그룹으로 처리
            let (row, ctx) = self
                .compute_aggregated_row_with_context(clause, contexts)
                .await?;
            return Ok((vec![row], vec![ctx]));
        }

        // 3. 그룹 키로 컨텍스트를 그룹화
        let groups = self
            .group_contexts_by_keys(clause, &group_key_indices, contexts)
            .await?;

        // 4. 각 그룹에 대해 결과 행과 컨텍스트 생성
        let mut results = Vec::new();
        let mut result_contexts = Vec::new();
        for (_, group_contexts) in groups {
            let (row, ctx) = self
                .compute_aggregated_row_with_context(clause, &group_contexts)
                .await?;
            results.push(row);
            result_contexts.push(ctx);
        }

        Ok((results, result_contexts))
    }

    /// RETURN 항목을 집계/비집계로 분류
    /// 반환: (비집계 인덱스들, 집계 인덱스들)
    fn classify_return_items(&self, clause: &ReturnClause) -> (Vec<usize>, Vec<usize>) {
        let mut group_keys = Vec::new();
        let mut aggregates = Vec::new();

        for (idx, item) in clause.items.iter().enumerate() {
            match item {
                ReturnItem::Expression { expr, .. } => {
                    if self.contains_aggregate(expr) {
                        aggregates.push(idx);
                    } else {
                        group_keys.push(idx);
                    }
                }
                ReturnItem::Star => {
                    group_keys.push(idx);
                }
            }
        }

        (group_keys, aggregates)
    }

    /// 그룹 키로 컨텍스트 그룹화
    ///
    /// v1.12.0: async로 변경
    async fn group_contexts_by_keys(
        &self,
        clause: &ReturnClause,
        key_indices: &[usize],
        contexts: &[ExecutionContext],
    ) -> Result<Vec<(String, Vec<ExecutionContext>)>> {
        let mut groups: std::collections::HashMap<String, Vec<ExecutionContext>> =
            std::collections::HashMap::new();

        for ctx in contexts {
            // 그룹 키 생성
            let mut key_parts = Vec::new();
            for &idx in key_indices {
                if let Some(item) = clause.items.get(idx) {
                    match item {
                        ReturnItem::Expression { expr, .. } => {
                            let val = self.evaluate_expression(expr, ctx).await?;
                            key_parts.push(format!("{:?}", val));
                        }
                        ReturnItem::Star => {
                            key_parts.push(format!("{:?}", ctx.variables));
                        }
                    }
                }
            }
            let key = key_parts.join("|");

            groups.entry(key).or_default().push(ctx.clone());
        }

        // 순서 보존을 위해 Vec으로 변환
        Ok(groups.into_iter().collect())
    }

    /// 그룹에 대해 집계된 결과 행과 컨텍스트 계산
    /// 컨텍스트에 alias 변수를 추가하여 ORDER BY에서 참조 가능
    ///
    /// v1.12.0: async로 변경
    async fn compute_aggregated_row_with_context(
        &self,
        clause: &ReturnClause,
        contexts: &[ExecutionContext],
    ) -> Result<(Vec<Value>, ExecutionContext)> {
        let mut row = Vec::new();

        // 첫 번째 컨텍스트를 기반으로 새 컨텍스트 생성 (그룹 키 값 유지)
        // 컨텍스트가 비어있어도 집계 결과는 계산해야 함 (Cypher 표준)
        // count → 0, sum → 0, avg/max/min → null, collect → []
        let mut new_ctx = match contexts.first() {
            Some(ctx) => ctx.clone(),
            None => ExecutionContext::new(0),
        };

        for item in &clause.items {
            match item {
                ReturnItem::Expression { expr, alias } => {
                    let val = self.evaluate_with_aggregation(expr, contexts).await?;
                    row.push(val.clone());

                    // alias가 있으면 컨텍스트에 추가 (ORDER BY에서 사용)
                    if let Some(alias_name) = alias {
                        new_ctx.variables.insert(alias_name.clone(), val);
                    }
                }
                ReturnItem::Star => {
                    // * 는 집계와 함께 사용하기 어려움 - 첫 번째 컨텍스트 사용
                    if let Some(ctx) = contexts.first() {
                        for (_, value) in &ctx.variables {
                            row.push(value.clone());
                        }
                    }
                }
            }
        }

        Ok((row, new_ctx))
    }

    /// 집계 함수를 고려한 표현식 평가
    ///
    /// v1.12.0: async로 변경
    #[async_recursion::async_recursion]
    async fn evaluate_with_aggregation(
        &self,
        expr: &Expression,
        contexts: &[ExecutionContext],
    ) -> Result<Value> {
        match expr {
            Expression::Aggregate(agg_call) => {
                self.execute_aggregate_function(agg_call, contexts).await
            }
            // 인덱스 접근: collect(...)[0] 형태 지원 (Issue #70)
            Expression::Subscript {
                expr: inner_expr,
                index,
            } => {
                // 내부 표현식을 집계 모드로 평가
                let array_val = self.evaluate_with_aggregation(inner_expr, contexts).await?;
                // 인덱스는 상수이므로 첫 번째 컨텍스트로 평가
                let index_val = if let Some(ctx) = contexts.first() {
                    self.evaluate_expression(index, ctx).await?
                } else {
                    return Ok(Value::Null);
                };
                self.apply_subscript(&array_val, &index_val)
            }
            // 범위 슬라이싱: collect(...)[0..10] 형태 지원 (Issue #70)
            Expression::SubscriptRange {
                expr: inner_expr,
                start,
                end,
            } => {
                // 내부 표현식을 집계 모드로 평가
                let array_val = self.evaluate_with_aggregation(inner_expr, contexts).await?;
                // 시작/끝 인덱스는 상수이므로 첫 번째 컨텍스트로 평가
                let ctx = contexts.first();
                let start_val = if let Some(start_expr) = start {
                    if let Some(c) = ctx {
                        Some(self.evaluate_expression(start_expr, c).await?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let end_val = if let Some(end_expr) = end {
                    if let Some(c) = ctx {
                        Some(self.evaluate_expression(end_expr, c).await?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.apply_subscript_range(&array_val, start_val.as_ref(), end_val.as_ref())
            }
            // 산술 연산: avg(x) * 100, sum(x) / count(x) 형태 지원
            Expression::Arithmetic { left, op, right } => {
                let left_val = self.evaluate_with_aggregation(left, contexts).await?;
                let right_val = self.evaluate_with_aggregation(right, contexts).await?;
                self.apply_arithmetic_op(&left_val, op, &right_val)
            }
            // 스칼라 함수: round(avg(x)), abs(sum(x)) 형태 지원
            Expression::FunctionCall(call) => {
                // 인수들을 집계 모드로 평가
                let mut evaluated_args = Vec::new();
                for arg in &call.arguments {
                    let val = self.evaluate_with_aggregation(arg, contexts).await?;
                    evaluated_args.push(val);
                }
                // 스칼라 함수 실행
                self.evaluate_scalar_function_values(
                    &call.function,
                    &evaluated_args,
                    contexts.first().map(|context| context.tx_id),
                )
                .await
            }
            // 비집계 표현식은 첫 번째 컨텍스트 사용 (그룹 내 모두 같은 값)
            _ => {
                if let Some(ctx) = contexts.first() {
                    self.evaluate_expression(expr, ctx).await
                } else {
                    Ok(Value::Null)
                }
            }
        }
    }

    /// 배열 인덱스 접근 적용 (집계 결과에 대한 subscript)
    fn apply_subscript(&self, array_val: &Value, index_val: &Value) -> Result<Value> {
        // NULL 전파
        if matches!(array_val, Value::Null) || matches!(index_val, Value::Null) {
            return Ok(Value::Null);
        }

        match (array_val, index_val) {
            (Value::Array(arr), Value::Integer(idx)) => {
                let idx = *idx;
                // 음수 인덱스 지원 (Python 스타일)
                let actual_idx = if idx < 0 {
                    let len = arr.len() as i64;
                    if idx.abs() > len {
                        return Ok(Value::Null);
                    }
                    (len + idx) as usize
                } else {
                    idx as usize
                };
                Ok(arr.get(actual_idx).cloned().unwrap_or(Value::Null))
            }
            (Value::String(s), Value::Integer(idx)) => {
                let idx = *idx;
                let chars: Vec<char> = s.chars().collect();
                let actual_idx = if idx < 0 {
                    let len = chars.len() as i64;
                    if idx.abs() > len {
                        return Ok(Value::Null);
                    }
                    (len + idx) as usize
                } else {
                    idx as usize
                };
                Ok(chars
                    .get(actual_idx)
                    .map(|c| Value::String(c.to_string()))
                    .unwrap_or(Value::Null))
            }
            (Value::Map(map), Value::String(key)) => {
                Ok(map.get(key).cloned().unwrap_or(Value::Null))
            }
            _ => Err(GraphError::Validation {
                message: format!(
                    "Cannot subscript {} with {}",
                    array_val.type_name(),
                    index_val.type_name()
                ),
            }),
        }
    }

    /// 배열 범위 슬라이싱 적용 (집계 결과에 대한 range slice)
    fn apply_subscript_range(
        &self,
        array_val: &Value,
        start: Option<&Value>,
        end: Option<&Value>,
    ) -> Result<Value> {
        // NULL 전파
        if matches!(array_val, Value::Null) {
            return Ok(Value::Null);
        }

        // 시작/끝 인덱스 추출
        let start_idx = match start {
            Some(Value::Integer(i)) => Some(*i),
            Some(Value::Null) => return Ok(Value::Null),
            Some(other) => {
                return Err(GraphError::Validation {
                    message: format!("Range start must be integer, got {}", other.type_name()),
                });
            }
            None => None,
        };

        let end_idx = match end {
            Some(Value::Integer(i)) => Some(*i),
            Some(Value::Null) => return Ok(Value::Null),
            Some(other) => {
                return Err(GraphError::Validation {
                    message: format!("Range end must be integer, got {}", other.type_name()),
                });
            }
            None => None,
        };

        match array_val {
            Value::Array(arr) => {
                let len = arr.len() as i64;

                let actual_start = match start_idx {
                    Some(idx) if idx < 0 => ((len + idx).max(0)) as usize,
                    Some(idx) => (idx as usize).min(arr.len()),
                    None => 0,
                };

                let actual_end = match end_idx {
                    Some(idx) if idx < 0 => ((len + idx).max(0)) as usize,
                    Some(idx) => (idx as usize).min(arr.len()),
                    None => arr.len(),
                };

                if actual_start >= actual_end {
                    return Ok(Value::Array(vec![]));
                }

                Ok(Value::Array(arr[actual_start..actual_end].to_vec()))
            }
            Value::String(s) => {
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;

                let actual_start = match start_idx {
                    Some(idx) if idx < 0 => ((len + idx).max(0)) as usize,
                    Some(idx) => (idx as usize).min(chars.len()),
                    None => 0,
                };

                let actual_end = match end_idx {
                    Some(idx) if idx < 0 => ((len + idx).max(0)) as usize,
                    Some(idx) => (idx as usize).min(chars.len()),
                    None => chars.len(),
                };

                if actual_start >= actual_end {
                    return Ok(Value::String(String::new()));
                }

                Ok(Value::String(
                    chars[actual_start..actual_end].iter().collect(),
                ))
            }
            _ => Err(GraphError::Validation {
                message: format!("Cannot slice {}", array_val.type_name()),
            }),
        }
    }

    /// 산술 연산 적용 (집계 결과에 대한 연산)
    fn apply_arithmetic_op(
        &self,
        left_val: &Value,
        op: &ArithmeticOp,
        right_val: &Value,
    ) -> Result<Value> {
        // NULL 전파
        if matches!(left_val, Value::Null) || matches!(right_val, Value::Null) {
            return Ok(Value::Null);
        }

        match (left_val, right_val) {
            (Value::Integer(l), Value::Integer(r)) => {
                // Power returns Float
                if matches!(op, ArithmeticOp::Power) {
                    return Ok(Value::Float((*l as f64).powf(*r as f64)));
                }
                let result = match op {
                    ArithmeticOp::Add => {
                        l.checked_add(*r).ok_or_else(|| GraphError::Validation {
                            message: "Integer overflow in addition".to_string(),
                        })?
                    }
                    ArithmeticOp::Subtract => {
                        l.checked_sub(*r).ok_or_else(|| GraphError::Validation {
                            message: "Integer overflow in subtraction".to_string(),
                        })?
                    }
                    ArithmeticOp::Multiply => {
                        l.checked_mul(*r).ok_or_else(|| GraphError::Validation {
                            message: "Integer overflow in multiplication".to_string(),
                        })?
                    }
                    ArithmeticOp::Divide => {
                        if *r == 0 {
                            return Err(GraphError::Validation {
                                message: "Division by zero".to_string(),
                            });
                        }
                        l / r
                    }
                    ArithmeticOp::Modulo => {
                        if *r == 0 {
                            return Err(GraphError::Validation {
                                message: "Modulo by zero".to_string(),
                            });
                        }
                        l % r
                    }
                    ArithmeticOp::Power => unreachable!(),
                };
                Ok(Value::Integer(result))
            }
            (Value::Float(l), Value::Float(r)) => {
                let result = match op {
                    ArithmeticOp::Add => l + r,
                    ArithmeticOp::Subtract => l - r,
                    ArithmeticOp::Multiply => l * r,
                    ArithmeticOp::Divide => {
                        if *r == 0.0 {
                            return Err(GraphError::Validation {
                                message: "Division by zero".to_string(),
                            });
                        }
                        l / r
                    }
                    ArithmeticOp::Modulo => {
                        if *r == 0.0 {
                            return Err(GraphError::Validation {
                                message: "Modulo by zero".to_string(),
                            });
                        }
                        l % r
                    }
                    ArithmeticOp::Power => l.powf(*r),
                };
                Ok(Value::Float(result))
            }
            (Value::Integer(l), Value::Float(r)) => {
                let l = *l as f64;
                let result = match op {
                    ArithmeticOp::Add => l + r,
                    ArithmeticOp::Subtract => l - r,
                    ArithmeticOp::Multiply => l * r,
                    ArithmeticOp::Divide => {
                        if *r == 0.0 {
                            return Err(GraphError::Validation {
                                message: "Division by zero".to_string(),
                            });
                        }
                        l / r
                    }
                    ArithmeticOp::Modulo => {
                        if *r == 0.0 {
                            return Err(GraphError::Validation {
                                message: "Modulo by zero".to_string(),
                            });
                        }
                        l % r
                    }
                    ArithmeticOp::Power => l.powf(*r),
                };
                Ok(Value::Float(result))
            }
            (Value::Float(l), Value::Integer(r)) => {
                let r = *r as f64;
                let result = match op {
                    ArithmeticOp::Add => l + r,
                    ArithmeticOp::Subtract => l - r,
                    ArithmeticOp::Multiply => l * r,
                    ArithmeticOp::Divide => {
                        if r == 0.0 {
                            return Err(GraphError::Validation {
                                message: "Division by zero".to_string(),
                            });
                        }
                        l / r
                    }
                    ArithmeticOp::Modulo => {
                        if r == 0.0 {
                            return Err(GraphError::Validation {
                                message: "Modulo by zero".to_string(),
                            });
                        }
                        l % r
                    }
                    ArithmeticOp::Power => l.powf(r),
                };
                Ok(Value::Float(result))
            }
            (Value::String(l), Value::String(r)) if matches!(op, ArithmeticOp::Add) => {
                // 문자열 연결 (+ 연산자만)
                Ok(Value::String(format!("{}{}", l, r)))
            }
            _ => Err(GraphError::Validation {
                message: "Cannot perform arithmetic operation on non-numeric values".to_string(),
            }),
        }
    }

    /// 집계 함수 실행
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_aggregate_function(
        &self,
        agg_call: &AggregateFunctionCall,
        contexts: &[ExecutionContext],
    ) -> Result<Value> {
        match &agg_call.function {
            AggregateFunction::CountAll => {
                // COUNT(*) - 전체 행 수
                Ok(Value::Integer(contexts.len() as i64))
            }
            AggregateFunction::Count => {
                // COUNT(expr) - NULL 제외 수
                let arg = agg_call
                    .argument
                    .as_ref()
                    .ok_or_else(|| GraphError::Validation {
                        message: "COUNT requires an argument".to_string(),
                    })?;

                let mut values = Vec::new();
                for ctx in contexts {
                    if let Ok(val) = self.evaluate_expression(arg, ctx).await
                        && !matches!(val, Value::Null)
                    {
                        values.push(val);
                    }
                }

                if agg_call.distinct {
                    // COUNT(DISTINCT expr)
                    let unique: std::collections::HashSet<String> =
                        values.iter().map(|v| format!("{:?}", v)).collect();
                    Ok(Value::Integer(unique.len() as i64))
                } else {
                    Ok(Value::Integer(values.len() as i64))
                }
            }
            AggregateFunction::Sum => {
                // SUM(expr)
                let arg = agg_call
                    .argument
                    .as_ref()
                    .ok_or_else(|| GraphError::Validation {
                        message: "SUM requires an argument".to_string(),
                    })?;

                let mut sum: f64 = 0.0;

                for ctx in contexts {
                    if let Ok(val) = self.evaluate_expression(arg, ctx).await {
                        match val {
                            Value::Integer(i) => sum += i as f64,
                            Value::Float(f) => sum += f,
                            _ => {}
                        }
                    }
                }

                // Neo4j 표준: 빈 집합에서 sum은 0 반환
                // 정수 결과인지 확인
                if sum.fract() == 0.0 && sum >= i64::MIN as f64 && sum <= i64::MAX as f64 {
                    Ok(Value::Integer(sum as i64))
                } else {
                    Ok(Value::Float(sum))
                }
            }
            AggregateFunction::Avg => {
                // AVG(expr)
                let arg = agg_call
                    .argument
                    .as_ref()
                    .ok_or_else(|| GraphError::Validation {
                        message: "AVG requires an argument".to_string(),
                    })?;

                let mut sum: f64 = 0.0;
                let mut count: usize = 0;

                for ctx in contexts {
                    if let Ok(val) = self.evaluate_expression(arg, ctx).await {
                        match val {
                            Value::Integer(i) => {
                                sum += i as f64;
                                count += 1;
                            }
                            Value::Float(f) => {
                                sum += f;
                                count += 1;
                            }
                            _ => {}
                        }
                    }
                }

                if count > 0 {
                    let avg = sum / count as f64;
                    // 정수에 매우 가까우면 반올림 (부동소수점 오차 보정)
                    let rounded = avg.round();
                    if (avg - rounded).abs() < 1e-10 {
                        Ok(Value::Float(rounded))
                    } else {
                        Ok(Value::Float(avg))
                    }
                } else {
                    Ok(Value::Null)
                }
            }
            AggregateFunction::Min => {
                // MIN(expr)
                let arg = agg_call
                    .argument
                    .as_ref()
                    .ok_or_else(|| GraphError::Validation {
                        message: "MIN requires an argument".to_string(),
                    })?;

                let mut min_val: Option<Value> = None;

                for ctx in contexts {
                    if let Ok(val) = self.evaluate_expression(arg, ctx).await {
                        if matches!(val, Value::Null) {
                            continue;
                        }
                        match &min_val {
                            None => min_val = Some(val),
                            Some(current) => {
                                if let Some(std::cmp::Ordering::Less) =
                                    self.compare_values(&val, current)
                                {
                                    min_val = Some(val);
                                }
                            }
                        }
                    }
                }

                Ok(min_val.unwrap_or(Value::Null))
            }
            AggregateFunction::Max => {
                // MAX(expr)
                let arg = agg_call
                    .argument
                    .as_ref()
                    .ok_or_else(|| GraphError::Validation {
                        message: "MAX requires an argument".to_string(),
                    })?;

                let mut max_val: Option<Value> = None;

                for ctx in contexts {
                    if let Ok(val) = self.evaluate_expression(arg, ctx).await {
                        if matches!(val, Value::Null) {
                            continue;
                        }
                        match &max_val {
                            None => max_val = Some(val),
                            Some(current) => {
                                if let Some(std::cmp::Ordering::Greater) =
                                    self.compare_values(&val, current)
                                {
                                    max_val = Some(val);
                                }
                            }
                        }
                    }
                }

                Ok(max_val.unwrap_or(Value::Null))
            }
            AggregateFunction::Collect => {
                // COLLECT(expr) - NULL 값은 제외, 순서 유지 (Cypher 표준)
                let arg = agg_call
                    .argument
                    .as_ref()
                    .ok_or_else(|| GraphError::Validation {
                        message: "COLLECT requires an argument".to_string(),
                    })?;

                let mut values = Vec::new();
                for ctx in contexts {
                    if let Ok(val) = self.evaluate_expression(arg, ctx).await
                        && !matches!(val, Value::Null)
                    {
                        values.push(val);
                    }
                }

                // Neo4j는 collect()에서 순서를 유지함 (정렬하지 않음)
                Ok(Value::Array(values))
            }
        }
    }

    /// WITH 절에서 집계가 포함된 경우 처리
    /// 그룹화 및 집계 결과를 새 컨텍스트로 반환
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_with_aggregation(
        &self,
        clause: &WithClause,
        contexts: &[ExecutionContext],
    ) -> Result<Vec<ExecutionContext>> {
        // 1. WITH 항목을 집계 vs 비집계로 분류
        let (group_key_indices, aggregate_indices) = self.classify_with_items(clause);

        // 2. 그룹 키가 없으면 전체를 하나의 그룹으로 처리
        if group_key_indices.is_empty() {
            let ctx = self
                .compute_with_aggregated_context(clause, contexts, &aggregate_indices)
                .await?;
            return Ok(vec![ctx]);
        }

        // 3. 그룹 키로 컨텍스트를 그룹화
        let groups = self
            .group_contexts_by_with_keys(clause, &group_key_indices, contexts)
            .await?;

        // 4. 각 그룹에 대해 새 컨텍스트 생성
        let mut result_contexts = Vec::new();
        for (_, group_contexts) in groups {
            let ctx = self
                .compute_with_aggregated_context(clause, &group_contexts, &aggregate_indices)
                .await?;
            result_contexts.push(ctx);
        }

        Ok(result_contexts)
    }

    /// WITH 항목을 집계/비집계로 분류
    fn classify_with_items(&self, clause: &WithClause) -> (Vec<usize>, Vec<usize>) {
        let mut group_keys = Vec::new();
        let mut aggregates = Vec::new();

        for (idx, item) in clause.items.iter().enumerate() {
            match item {
                ReturnItem::Expression { expr, .. } => {
                    if self.contains_aggregate(expr) {
                        aggregates.push(idx);
                    } else {
                        group_keys.push(idx);
                    }
                }
                ReturnItem::Star => {
                    group_keys.push(idx);
                }
            }
        }

        (group_keys, aggregates)
    }

    /// WITH 그룹 키로 컨텍스트 그룹화
    ///
    /// v1.12.0: async로 변경
    async fn group_contexts_by_with_keys(
        &self,
        clause: &WithClause,
        key_indices: &[usize],
        contexts: &[ExecutionContext],
    ) -> Result<Vec<(String, Vec<ExecutionContext>)>> {
        let mut groups: std::collections::HashMap<String, Vec<ExecutionContext>> =
            std::collections::HashMap::new();

        for ctx in contexts {
            // 그룹 키 생성
            let mut key_parts = Vec::new();
            for &idx in key_indices {
                if let Some(item) = clause.items.get(idx) {
                    match item {
                        ReturnItem::Expression { expr, .. } => {
                            let val = self.evaluate_expression(expr, ctx).await?;
                            key_parts.push(format!("{:?}", val));
                        }
                        ReturnItem::Star => {
                            key_parts.push(format!("{:?}", ctx.variables));
                        }
                    }
                }
            }
            let key = key_parts.join("|");

            groups.entry(key).or_default().push(ctx.clone());
        }

        // 순서 보존을 위해 Vec으로 변환
        Ok(groups.into_iter().collect())
    }

    /// WITH 그룹에 대해 집계된 컨텍스트 생성
    ///
    /// v1.12.0: async로 변경
    async fn compute_with_aggregated_context(
        &self,
        clause: &WithClause,
        contexts: &[ExecutionContext],
        _aggregate_indices: &[usize],
    ) -> Result<ExecutionContext> {
        // 첫 번째 컨텍스트를 기반으로 새 컨텍스트 생성
        let tx_id = contexts.first().map(|c| c.tx_id).unwrap_or(0);
        let mut new_ctx = ExecutionContext::new(tx_id);

        for item in &clause.items {
            match item {
                ReturnItem::Expression { expr, alias } => {
                    // 집계 함수가 포함된 표현식은 그룹 전체에 대해 평가
                    let val = if self.contains_aggregate(expr) {
                        self.evaluate_with_aggregation_for_contexts(expr, contexts)
                            .await?
                    } else {
                        // 비집계 표현식은 첫 번째 컨텍스트 사용 (그룹 내 모두 같은 값)
                        if let Some(ctx) = contexts.first() {
                            self.evaluate_expression(expr, ctx).await?
                        } else {
                            Value::Null
                        }
                    };

                    // alias가 있으면 그것을 사용, 없으면 표현식에서 변수명 추출
                    let var_name = if let Some(name) = alias {
                        name.clone()
                    } else {
                        self.extract_variable_name(expr)
                    };
                    new_ctx.variables.insert(var_name, val);
                }
                ReturnItem::Star => {
                    // * 는 첫 번째 컨텍스트의 모든 변수를 그대로 전달
                    if let Some(ctx) = contexts.first() {
                        for (key, value) in &ctx.variables {
                            new_ctx.variables.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
        }

        Ok(new_ctx)
    }

    /// 컨텍스트 그룹에 대해 집계를 포함한 표현식 평가
    ///
    /// v1.12.0: async로 변경
    #[async_recursion::async_recursion]
    async fn evaluate_with_aggregation_for_contexts(
        &self,
        expr: &Expression,
        contexts: &[ExecutionContext],
    ) -> Result<Value> {
        match expr {
            Expression::Aggregate(agg_call) => {
                self.execute_aggregate_function(agg_call, contexts).await
            }
            // 인덱스 접근: collect(...)[0] 형태 지원 (Issue #70)
            Expression::Subscript {
                expr: inner_expr,
                index,
            } => {
                let array_val = self
                    .evaluate_with_aggregation_for_contexts(inner_expr, contexts)
                    .await?;
                let index_val = if let Some(ctx) = contexts.first() {
                    self.evaluate_expression(index, ctx).await?
                } else {
                    return Ok(Value::Null);
                };
                self.apply_subscript(&array_val, &index_val)
            }
            // 범위 슬라이싱: collect(...)[0..10] 형태 지원 (Issue #70)
            Expression::SubscriptRange {
                expr: inner_expr,
                start,
                end,
            } => {
                let array_val = self
                    .evaluate_with_aggregation_for_contexts(inner_expr, contexts)
                    .await?;
                let ctx = contexts.first();
                let start_val = if let Some(start_expr) = start {
                    if let Some(c) = ctx {
                        Some(self.evaluate_expression(start_expr, c).await?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let end_val = if let Some(end_expr) = end {
                    if let Some(c) = ctx {
                        Some(self.evaluate_expression(end_expr, c).await?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.apply_subscript_range(&array_val, start_val.as_ref(), end_val.as_ref())
            }
            // 비집계 표현식은 첫 번째 컨텍스트 사용
            _ => {
                if let Some(ctx) = contexts.first() {
                    self.evaluate_expression(expr, ctx).await
                } else {
                    Ok(Value::Null)
                }
            }
        }
    }
}
