//! 리스트 함수 구현
//!
//! - head(list): 리스트의 첫 번째 요소
//! - tail(list): 첫 번째 요소를 제외한 나머지
//! - last(list): 리스트의 마지막 요소
//! - size(list|string): 길이
//! - range(start, end, step?): 정수 범위 생성
//! - keys(map): 맵의 키 리스트

use crate::ast::ScalarFunction;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

use super::{validate_arg_count, validate_arg_count_range};

/// 리스트 함수 평가
pub fn evaluate(func: &ScalarFunction, args: &[Value]) -> Result<Value> {
    match func {
        ScalarFunction::Head => eval_head(args),
        ScalarFunction::Tail => eval_tail(args),
        ScalarFunction::Last => eval_last(args),
        ScalarFunction::Size => eval_size(args),
        ScalarFunction::Range => eval_range(args),
        ScalarFunction::Keys => eval_keys(args),
        _ => Err(GraphError::Validation {
            message: format!("Unexpected list function: {:?}", func),
        }),
    }
}

/// head(list) - 첫 번째 요소 반환
fn eval_head(args: &[Value]) -> Result<Value> {
    validate_arg_count("head", args, 1)?;
    match &args[0] {
        Value::Array(arr) => Ok(arr.first().cloned().unwrap_or(Value::Null)),
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!("head() expected List, got {}", args[0].type_name()),
        }),
    }
}

/// tail(list) - 첫 번째 요소 제외한 나머지
fn eval_tail(args: &[Value]) -> Result<Value> {
    validate_arg_count("tail", args, 1)?;
    match &args[0] {
        Value::Array(arr) => {
            if arr.is_empty() {
                Ok(Value::Array(vec![]))
            } else {
                Ok(Value::Array(arr[1..].to_vec()))
            }
        }
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!("tail() expected List, got {}", args[0].type_name()),
        }),
    }
}

/// last(list) - 마지막 요소 반환
fn eval_last(args: &[Value]) -> Result<Value> {
    validate_arg_count("last", args, 1)?;
    match &args[0] {
        Value::Array(arr) => Ok(arr.last().cloned().unwrap_or(Value::Null)),
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!("last() expected List, got {}", args[0].type_name()),
        }),
    }
}

/// size(list|string) - 길이 반환
fn eval_size(args: &[Value]) -> Result<Value> {
    validate_arg_count("size", args, 1)?;
    match &args[0] {
        Value::Array(arr) => Ok(Value::Integer(arr.len() as i64)),
        Value::String(s) => Ok(Value::Integer(s.chars().count() as i64)),
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!(
                "size() expected List or String, got {}",
                args[0].type_name()
            ),
        }),
    }
}

/// range(start, end, step?) - 정수 범위 생성
fn eval_range(args: &[Value]) -> Result<Value> {
    validate_arg_count_range("range", args, 2, 3)?;

    match (&args[0], &args[1]) {
        (Value::Integer(start), Value::Integer(end)) => {
            let step = if args.len() == 3 {
                match &args[2] {
                    Value::Integer(s) => *s,
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!(
                                "range() step expected Integer, got {}",
                                args[2].type_name()
                            ),
                        });
                    }
                }
            } else {
                1
            };

            if step == 0 {
                return Err(GraphError::Validation {
                    message: "range() step cannot be zero".to_string(),
                });
            }

            let mut result = Vec::new();
            let mut i = *start;

            if step > 0 {
                while i <= *end {
                    result.push(Value::Integer(i));
                    i += step;
                }
            } else {
                while i >= *end {
                    result.push(Value::Integer(i));
                    i += step;
                }
            }

            Ok(Value::Array(result))
        }
        (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!(
                "range() expected (Integer, Integer), got ({}, {})",
                args[0].type_name(),
                args[1].type_name()
            ),
        }),
    }
}

/// keys(map) - 맵의 키 리스트 반환 (삽입 순서 유지)
fn eval_keys(args: &[Value]) -> Result<Value> {
    validate_arg_count("keys", args, 1)?;
    match &args[0] {
        Value::Map(map) => {
            // null 값 프로퍼티 제외, 삽입 순서 유지
            let keys: Vec<Value> = map
                .iter()
                .filter(|(_, v)| !matches!(v, Value::Null))
                .map(|(k, _)| Value::String(k.clone()))
                .collect();
            Ok(Value::Array(keys))
        }
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!("keys() expected Map, got {}", args[0].type_name()),
        }),
    }
}
