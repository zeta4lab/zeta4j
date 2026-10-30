//! 표현식 평가 모듈
//!
//! Cypher 표현식의 평가를 담당합니다.
//! - 리터럴, 변수, 속성 접근
//! - 비교 및 논리 연산
//! - CASE 표현식
//! - EXISTS 서브쿼리
//!
//! # v1.12.0 비동기 API 통일
//!
//! 모든 표현식 평가 메서드가 async로 변경되었습니다.

use super::temporal::{
    get_date_property, get_datetime_property, get_duration_property, get_local_datetime_property,
    get_time_property,
};
use super::{ExecutionContext, QueryExecutor};
use indexmap::IndexMap;
use zeta4j_common::{ZetaError, codes};
use zeta4j_cypher_core::ast::*;
use zeta4j_graph::core::{PropertyContainer, Value};
use zeta4j_graph::{GraphError, Result};

impl QueryExecutor {
    /// 표현식 평가
    ///
    #[async_recursion::async_recursion]
    pub(super) async fn evaluate_expression(
        &self,
        expr: &Expression,
        context: &ExecutionContext,
    ) -> Result<Value> {
        match expr {
            Expression::Literal(val) => Ok(val.clone()),
            Expression::Parameter(name) => context.parameters.get(name).cloned().ok_or_else(|| {
                GraphError::Zeta(ZetaError::QueryExecution {
                    code: codes::cypher::PARAMETER_NOT_FOUND,
                    message: format!("Parameter not found: ${}", name),
                })
            }),
            Expression::Variable(name) => context.variables.get(name).cloned().ok_or_else(|| {
                GraphError::Zeta(ZetaError::QueryExecution {
                    code: codes::cypher::VARIABLE_NOT_FOUND,
                    message: format!("Variable not found: {}", name),
                })
            }),
            Expression::PropertyAccess(var, prop) => {
                self.evaluate_property_access(var, prop, context).await
            }
            Expression::Comparison { left, op, right } => {
                self.evaluate_comparison(left, op, right, context).await
            }
            Expression::Logical { left, op, right } => {
                self.evaluate_logical(left, op, right, context).await
            }
            Expression::IsNull { expr, negated } => {
                let val = self.evaluate_expression(expr, context).await?;
                let is_null = matches!(val, Value::Null);
                Ok(Value::Boolean(if *negated { !is_null } else { is_null }))
            }
            Expression::List(exprs) => {
                let mut values = Vec::with_capacity(exprs.len());
                for e in exprs {
                    values.push(self.evaluate_expression(e, context).await?);
                }
                Ok(Value::Array(values))
            }
            Expression::Map(pairs) => {
                let mut map = IndexMap::new();
                for (key, value_expr) in pairs {
                    let value = self.evaluate_expression(value_expr, context).await?;
                    map.insert(key.clone(), value);
                }
                Ok(Value::Map(map))
            }
            Expression::Aggregate(_) => {
                // 집계 함수는 단일 컨텍스트에서 평가할 수 없음
                // execute_grouped_return에서 처리됨
                Err(GraphError::Validation {
                    message: "Aggregate functions cannot be evaluated in this context".to_string(),
                })
            }
            Expression::FunctionCall(call) => self.evaluate_scalar_function(call, context).await,
            Expression::Case(case_expr) => self.evaluate_case_expression(case_expr, context).await,
            Expression::Exists(exists_expr) => {
                self.evaluate_exists_expression(exists_expr, context).await
            }
            Expression::Arithmetic { left, op, right } => {
                self.evaluate_arithmetic(left, op, right, context).await
            }
            Expression::ListComprehension(comp) => {
                self.evaluate_list_comprehension(comp, context).await
            }
            Expression::Subscript { expr, index } => {
                self.evaluate_subscript(expr, index, context).await
            }
            Expression::SubscriptRange { expr, start, end } => {
                self.evaluate_subscript_range(expr, start.as_deref(), end.as_deref(), context)
                    .await
            }
            Expression::Vector(vec_expr) => self.evaluate_vector(vec_expr, context).await,
        }
    }

    /// 배열/리스트 인덱스 접근 평가
    /// expr[index] - 배열의 특정 인덱스 요소 반환
    async fn evaluate_subscript(
        &self,
        expr: &Expression,
        index: &Expression,
        context: &ExecutionContext,
    ) -> Result<Value> {
        let array_val = self.evaluate_expression(expr, context).await?;
        let index_val = self.evaluate_expression(index, context).await?;

        // NULL 전파
        if matches!(array_val, Value::Null) || matches!(index_val, Value::Null) {
            return Ok(Value::Null);
        }

        match (&array_val, &index_val) {
            (Value::Array(arr), Value::Integer(idx)) => {
                let idx = *idx;
                // 음수 인덱스 지원 (Python 스타일)
                let actual_idx = if idx < 0 {
                    let len = arr.len() as i64;
                    if idx.abs() > len {
                        return Ok(Value::Null); // 범위 초과
                    }
                    (len + idx) as usize
                } else {
                    idx as usize
                };

                Ok(arr.get(actual_idx).cloned().unwrap_or(Value::Null))
            }
            (Value::String(s), Value::Integer(idx)) => {
                // 문자열 인덱스 접근
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
                // 맵 키 접근
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

    /// 배열/리스트 범위 슬라이싱 평가
    /// expr[start..end] - 배열의 부분 배열 반환
    async fn evaluate_subscript_range(
        &self,
        expr: &Expression,
        start: Option<&Expression>,
        end: Option<&Expression>,
        context: &ExecutionContext,
    ) -> Result<Value> {
        let array_val = self.evaluate_expression(expr, context).await?;

        // NULL 전파
        if matches!(array_val, Value::Null) {
            return Ok(Value::Null);
        }

        // 시작/끝 인덱스 평가
        let start_idx = if let Some(start_expr) = start {
            match self.evaluate_expression(start_expr, context).await? {
                Value::Integer(i) => Some(i),
                Value::Null => return Ok(Value::Null),
                other => {
                    return Err(GraphError::Validation {
                        message: format!("Range start must be integer, got {}", other.type_name()),
                    });
                }
            }
        } else {
            None
        };

        let end_idx = if let Some(end_expr) = end {
            match self.evaluate_expression(end_expr, context).await? {
                Value::Integer(i) => Some(i),
                Value::Null => return Ok(Value::Null),
                other => {
                    return Err(GraphError::Validation {
                        message: format!("Range end must be integer, got {}", other.type_name()),
                    });
                }
            }
        } else {
            None
        };

        match array_val {
            Value::Array(arr) => {
                let len = arr.len() as i64;

                // 시작 인덱스 계산 (음수 지원)
                let actual_start = match start_idx {
                    Some(idx) if idx < 0 => ((len + idx).max(0)) as usize,
                    Some(idx) => (idx as usize).min(arr.len()),
                    None => 0,
                };

                // 끝 인덱스 계산 (음수 지원, exclusive)
                let actual_end = match end_idx {
                    Some(idx) if idx < 0 => ((len + idx).max(0)) as usize,
                    Some(idx) => (idx as usize).min(arr.len()),
                    None => arr.len(),
                };

                // 범위 검증
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

    /// 리스트 컴프리헨션 평가
    /// [x IN list WHERE cond | expr]
    async fn evaluate_list_comprehension(
        &self,
        comp: &ListComprehensionExpr,
        context: &ExecutionContext,
    ) -> Result<Value> {
        // 리스트 표현식 평가
        let list_val = self.evaluate_expression(&comp.list_expr, context).await?;

        let items = match list_val {
            Value::Array(arr) => arr,
            Value::Null => return Ok(Value::Null),
            _ => {
                return Err(GraphError::Validation {
                    message: format!("List comprehension requires a list, got {:?}", list_val),
                });
            }
        };

        let mut result = Vec::new();

        for item in items {
            // 반복 변수를 컨텍스트에 바인딩
            let mut inner_context = context.clone();
            inner_context
                .variables
                .insert(comp.variable.clone(), item.clone());

            // 필터 조건 평가 (있는 경우)
            if let Some(filter) = &comp.filter {
                let filter_val = self.evaluate_expression(filter, &inner_context).await?;
                if !self.is_truthy(&filter_val) {
                    continue; // 조건 미충족 시 스킵
                }
            }

            // 변환 표현식 평가 (있는 경우)
            let result_val = if let Some(map_expr) = &comp.map_expr {
                self.evaluate_expression(map_expr, &inner_context).await?
            } else {
                item // 변환 없으면 원본 값
            };

            result.push(result_val);
        }

        Ok(Value::Array(result))
    }

    /// 산술 연산 평가
    async fn evaluate_arithmetic(
        &self,
        left: &Expression,
        op: &ArithmeticOp,
        right: &Expression,
        context: &ExecutionContext,
    ) -> Result<Value> {
        let left_val = self.evaluate_expression(left, context).await?;
        let right_val = self.evaluate_expression(right, context).await?;

        // NULL 전파
        if matches!(left_val, Value::Null) || matches!(right_val, Value::Null) {
            return Ok(Value::Null);
        }

        match (left_val, right_val) {
            (Value::Integer(l), Value::Integer(r)) => {
                // Power returns Float
                if matches!(op, ArithmeticOp::Power) {
                    return Ok(Value::Float((l as f64).powf(r as f64)));
                }
                let result = match op {
                    ArithmeticOp::Add => {
                        l.checked_add(r).ok_or_else(|| GraphError::Validation {
                            message: "Integer overflow in addition".to_string(),
                        })?
                    }
                    ArithmeticOp::Subtract => {
                        l.checked_sub(r).ok_or_else(|| GraphError::Validation {
                            message: "Integer overflow in subtraction".to_string(),
                        })?
                    }
                    ArithmeticOp::Multiply => {
                        l.checked_mul(r).ok_or_else(|| GraphError::Validation {
                            message: "Integer overflow in multiplication".to_string(),
                        })?
                    }
                    ArithmeticOp::Divide => {
                        if r == 0 {
                            return Err(GraphError::Validation {
                                message: "Division by zero".to_string(),
                            });
                        }
                        l / r
                    }
                    ArithmeticOp::Modulo => {
                        if r == 0 {
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
            (Value::Integer(l), Value::Float(r)) => {
                let l = l as f64;
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
            (Value::Float(l), Value::Integer(r)) => {
                let r = r as f64;
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

    /// 속성 접근 평가 (var.prop)
    /// 특수 속성: _id (내부 ID), _labels (노드 라벨)
    async fn evaluate_property_access(
        &self,
        var: &str,
        prop: &str,
        context: &ExecutionContext,
    ) -> Result<Value> {
        match context.variables.get(var) {
            Some(Value::Node(id)) => {
                // 특수 속성 처리
                if prop == "_id" {
                    return Ok(Value::Integer(*id as i64));
                }
                let node = self
                    .engine
                    .get_node_in_transaction(context.tx_id, *id)
                    .await?;
                if let Some(node) = node {
                    if prop == "_labels" {
                        let labels: Vec<Value> = node
                            .labels()
                            .iter()
                            .map(|l| Value::String(l.clone()))
                            .collect();
                        return Ok(Value::Array(labels));
                    }
                    Ok(node.get_property(prop).cloned().unwrap_or(Value::Null))
                } else {
                    Ok(Value::Null)
                }
            }
            Some(Value::Edge(id)) => {
                // 특수 속성 처리
                if prop == "_id" {
                    return Ok(Value::Integer(*id as i64));
                }
                let edge = self
                    .engine
                    .get_edge_in_transaction(context.tx_id, *id)
                    .await?;
                if let Some(edge) = edge {
                    if prop == "_type" {
                        return Ok(Value::String(edge.relation_type().to_string()));
                    }
                    Ok(edge.get_property(prop).cloned().unwrap_or(Value::Null))
                } else {
                    Ok(Value::Null)
                }
            }
            // === 시간 타입 속성 접근 ===
            Some(Value::Date(d)) => get_date_property(d, prop),
            Some(Value::Time(t)) => get_time_property(t, prop),
            Some(Value::DateTime(dt)) => get_datetime_property(dt, prop),
            Some(Value::LocalDateTime(ldt)) => get_local_datetime_property(ldt, prop),
            Some(Value::Duration(dur)) => get_duration_property(dur, prop),
            Some(Value::Map(map)) => {
                // 맵에서 속성 접근
                Ok(map.get(prop).cloned().unwrap_or(Value::Null))
            }
            _ => Ok(Value::Null),
        }
    }

    /// 비교 연산 평가
    async fn evaluate_comparison(
        &self,
        left: &Expression,
        op: &ComparisonOp,
        right: &Expression,
        context: &ExecutionContext,
    ) -> Result<Value> {
        let left_val = self.evaluate_expression(left, context).await?;
        let right_val = self.evaluate_expression(right, context).await?;

        let result = match op {
            ComparisonOp::Equal => self.values_equal(&left_val, &right_val),
            ComparisonOp::NotEqual => !self.values_equal(&left_val, &right_val),
            ComparisonOp::LessThan => {
                self.compare_values(&left_val, &right_val) == Some(std::cmp::Ordering::Less)
            }
            ComparisonOp::LessEqual => {
                matches!(
                    self.compare_values(&left_val, &right_val),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                )
            }
            ComparisonOp::GreaterThan => {
                self.compare_values(&left_val, &right_val) == Some(std::cmp::Ordering::Greater)
            }
            ComparisonOp::GreaterEqual => {
                matches!(
                    self.compare_values(&left_val, &right_val),
                    Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                )
            }
            ComparisonOp::Contains => self.string_contains(&left_val, &right_val),
            ComparisonOp::StartsWith => self.string_starts_with(&left_val, &right_val),
            ComparisonOp::EndsWith => self.string_ends_with(&left_val, &right_val),
            ComparisonOp::In => self.value_in_list(&left_val, &right_val),
            ComparisonOp::IsNull => matches!(left_val, Value::Null),
            ComparisonOp::IsNotNull => !matches!(left_val, Value::Null),
        };

        Ok(Value::Boolean(result))
    }

    /// 논리 연산 평가
    async fn evaluate_logical(
        &self,
        left: &Option<Box<Expression>>,
        op: &LogicalOp,
        right: &Expression,
        context: &ExecutionContext,
    ) -> Result<Value> {
        match op {
            LogicalOp::Not => {
                let right_val = self.evaluate_expression(right, context).await?;
                Ok(Value::Boolean(!self.is_truthy(&right_val)))
            }
            LogicalOp::And => {
                if let Some(left_expr) = left {
                    let left_val = self.evaluate_expression(left_expr, context).await?;
                    if !self.is_truthy(&left_val) {
                        return Ok(Value::Boolean(false)); // 단락 평가
                    }
                }
                let right_val = self.evaluate_expression(right, context).await?;
                Ok(Value::Boolean(self.is_truthy(&right_val)))
            }
            LogicalOp::Or => {
                if let Some(left_expr) = left {
                    let left_val = self.evaluate_expression(left_expr, context).await?;
                    if self.is_truthy(&left_val) {
                        return Ok(Value::Boolean(true)); // 단락 평가
                    }
                }
                let right_val = self.evaluate_expression(right, context).await?;
                Ok(Value::Boolean(self.is_truthy(&right_val)))
            }
            LogicalOp::Xor => {
                let left_truthy = if let Some(left_expr) = left {
                    self.is_truthy(&self.evaluate_expression(left_expr, context).await?)
                } else {
                    false
                };
                let right_val = self.evaluate_expression(right, context).await?;
                let right_truthy = self.is_truthy(&right_val);
                Ok(Value::Boolean(left_truthy ^ right_truthy))
            }
        }
    }

    /// EXISTS 서브쿼리 실행
    /// EXISTS { pattern [WHERE condition] } - 매치가 있으면 true, 없으면 false
    pub(super) async fn evaluate_exists_expression(
        &self,
        exists_expr: &ExistsExpression,
        context: &ExecutionContext,
    ) -> Result<Value> {
        // 임시 ReadingClause::Match 생성하여 기존 매칭 로직 재활용
        let match_clause = ReadingClause::Match {
            patterns: vec![exists_expr.pattern.clone()],
            where_clause: exists_expr.where_clause.as_ref().map(|e| (**e).clone()),
            optional: false,
        };

        // 현재 컨텍스트에서 패턴 매칭 실행
        let matches = self.execute_reading(&match_clause, context).await?;

        // 매치가 하나라도 있으면 true
        Ok(Value::Boolean(!matches.is_empty()))
    }

    /// CASE 표현식 실행
    /// Simple CASE: CASE expr WHEN value THEN result ... ELSE default END
    /// Generic CASE: CASE WHEN condition THEN result ... ELSE default END
    pub(super) async fn evaluate_case_expression(
        &self,
        case_expr: &CaseExpression,
        context: &ExecutionContext,
    ) -> Result<Value> {
        // Simple CASE: operand가 있는 경우
        if let Some(operand) = &case_expr.operand {
            let operand_value = self.evaluate_expression(operand, context).await?;

            // 각 WHEN 절 검사
            for when_clause in &case_expr.when_clauses {
                let when_value = self
                    .evaluate_expression(&when_clause.condition, context)
                    .await?;
                if self.values_equal(&operand_value, &when_value) {
                    return self.evaluate_expression(&when_clause.result, context).await;
                }
            }
        } else {
            // Generic CASE: operand가 없는 경우 (조건식 평가)
            for when_clause in &case_expr.when_clauses {
                let condition_value = self
                    .evaluate_expression(&when_clause.condition, context)
                    .await?;
                if self.is_truthy(&condition_value) {
                    return self.evaluate_expression(&when_clause.result, context).await;
                }
            }
        }

        // 매치되는 WHEN이 없으면 ELSE 결과 반환, ELSE도 없으면 NULL
        if let Some(else_result) = &case_expr.else_result {
            self.evaluate_expression(else_result, context).await
        } else {
            Ok(Value::Null)
        }
    }

    /// Phase 3: 벡터 표현식 평가
    /// vector([0.1, 0.2, 0.3]) 또는 vector($param)
    async fn evaluate_vector(
        &self,
        vec_expr: &VectorExpression,
        context: &ExecutionContext,
    ) -> Result<Value> {
        use zeta4j_graph::core::VectorValue;

        match vec_expr {
            VectorExpression::Literal(elements) => {
                // 각 요소 평가 후 f32로 변환
                let mut floats = Vec::with_capacity(elements.len());
                for e in elements {
                    let value = self.evaluate_expression(e, context).await?;
                    floats.push(self.value_to_f32(&value)?);
                }

                if floats.is_empty() {
                    return Err(GraphError::Validation {
                        message: "Empty vector is not allowed".to_string(),
                    });
                }

                let vector = VectorValue::new(floats).map_err(|e| GraphError::Validation {
                    message: format!("Vector error: {}", e),
                })?;

                Ok(Value::Vector(vector))
            }

            VectorExpression::Parameter(name) => {
                // 파라미터에서 벡터 가져오기
                let param_value = context.parameters.get(name).ok_or_else(|| {
                    GraphError::Zeta(ZetaError::QueryExecution {
                        code: codes::cypher::PARAMETER_NOT_FOUND,
                        message: format!("Parameter not found: ${}", name),
                    })
                })?;

                self.param_to_vector(param_value)
            }
        }
    }

    /// Value를 f32로 변환
    fn value_to_f32(&self, value: &Value) -> Result<f32> {
        match value {
            Value::Float(f) => Ok(*f as f32),
            Value::Integer(i) => Ok(*i as f32),
            _ => Err(GraphError::Validation {
                message: format!(
                    "Expected numeric value for vector element, got {}",
                    value.type_name()
                ),
            }),
        }
    }

    /// 파라미터 값을 VectorValue로 변환
    fn param_to_vector(&self, value: &Value) -> Result<Value> {
        use zeta4j_graph::core::VectorValue;

        match value {
            Value::Vector(_) => Ok(value.clone()),

            Value::Array(arr) => {
                let floats: std::result::Result<Vec<f32>, _> =
                    arr.iter().map(|v| self.value_to_f32(v)).collect();

                let vector = VectorValue::new(floats?).map_err(|e| GraphError::Validation {
                    message: format!("Vector error: {}", e),
                })?;

                Ok(Value::Vector(vector))
            }

            _ => Err(GraphError::Validation {
                message: format!(
                    "Expected vector or array for vector parameter, got {}",
                    value.type_name()
                ),
            }),
        }
    }
}
