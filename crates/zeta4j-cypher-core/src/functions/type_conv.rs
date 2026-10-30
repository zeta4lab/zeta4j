//! 타입 변환 함수 구현
//!
//! 순수 타입 변환 함수만 포함합니다.
//! 그래프 엔진 접근이 필요한 함수(type, labels, properties, id)는
//! executor에서 직접 처리합니다.
//!
//! - toInteger(value): 정수로 변환
//! - toFloat(value): 실수로 변환
//! - toBoolean(value): 불리언으로 변환
//! - coalesce(v1, v2, ...): 첫 번째 non-null 값 반환
//! - randomUUID(): 랜덤 UUID 생성
//! - timestamp(): 현재 타임스탬프 (밀리초)

use crate::ast::ScalarFunction;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

use super::validate_arg_count;

/// 타입 변환 함수 평가
pub fn evaluate(func: &ScalarFunction, args: &[Value]) -> Result<Value> {
    match func {
        ScalarFunction::ToInteger => eval_to_integer(args),
        ScalarFunction::ToFloat => eval_to_float(args),
        ScalarFunction::ToBoolean => eval_to_boolean(args),
        ScalarFunction::Coalesce => eval_coalesce(args),
        ScalarFunction::RandomUUID => eval_random_uuid(args),
        ScalarFunction::Timestamp => eval_timestamp(args),
        _ => Err(GraphError::Validation {
            message: format!("Unexpected type conversion function: {:?}", func),
        }),
    }
}

/// toInteger(value) - 정수로 변환
fn eval_to_integer(args: &[Value]) -> Result<Value> {
    validate_arg_count("toInteger", args, 1)?;
    match &args[0] {
        Value::Integer(i) => Ok(Value::Integer(*i)),
        Value::Float(f) => Ok(Value::Integer(*f as i64)),
        Value::String(s) => Ok(s
            .trim()
            .parse::<i64>()
            .map(Value::Integer)
            .unwrap_or(Value::Null)),
        Value::Boolean(b) => Ok(Value::Integer(if *b { 1 } else { 0 })),
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!(
                "toInteger() cannot convert {} to Integer",
                args[0].type_name()
            ),
        }),
    }
}

/// toFloat(value) - 실수로 변환
fn eval_to_float(args: &[Value]) -> Result<Value> {
    validate_arg_count("toFloat", args, 1)?;
    match &args[0] {
        Value::Integer(i) => Ok(Value::Float(*i as f64)),
        Value::Float(f) => Ok(Value::Float(*f)),
        Value::String(s) => Ok(s
            .trim()
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or(Value::Null)),
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!("toFloat() cannot convert {} to Float", args[0].type_name()),
        }),
    }
}

/// toBoolean(value) - 불리언으로 변환
fn eval_to_boolean(args: &[Value]) -> Result<Value> {
    validate_arg_count("toBoolean", args, 1)?;
    match &args[0] {
        Value::Boolean(b) => Ok(Value::Boolean(*b)),
        Value::String(s) => Ok(match s.to_lowercase().as_str() {
            "true" => Value::Boolean(true),
            "false" => Value::Boolean(false),
            _ => Value::Null,
        }),
        Value::Integer(i) => Ok(Value::Boolean(*i != 0)),
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!(
                "toBoolean() cannot convert {} to Boolean",
                args[0].type_name()
            ),
        }),
    }
}

/// coalesce(v1, v2, ...) - 첫 번째 non-null 값 반환
fn eval_coalesce(args: &[Value]) -> Result<Value> {
    if args.is_empty() {
        return Err(GraphError::Validation {
            message: "coalesce requires at least 1 argument".to_string(),
        });
    }
    for arg in args {
        if !matches!(arg, Value::Null) {
            return Ok(arg.clone());
        }
    }
    Ok(Value::Null)
}

/// randomUUID() - 랜덤 UUID 생성
fn eval_random_uuid(args: &[Value]) -> Result<Value> {
    validate_arg_count("randomUUID", args, 0)?;
    use uuid::Uuid;
    Ok(Value::String(Uuid::now_v7().to_string()))
}

/// timestamp() - 현재 타임스탬프 (밀리초)
fn eval_timestamp(args: &[Value]) -> Result<Value> {
    validate_arg_count("timestamp", args, 0)?;
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(Value::Integer(duration.as_millis() as i64))
}
