//! 공간 함수 구현 모듈
//!
//! 공간 관련 함수들의 구현:
//! - point(): 2D/3D 포인트 생성
//! - distance(): 유클리드 거리 계산
//! - withinBBox(): 경계 사각형 내 포함 여부

use crate::ast::ScalarFunction;
use indexmap::IndexMap;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

/// 공간 함수 평가
pub fn evaluate(func: &ScalarFunction, args: &[Value]) -> Result<Value> {
    match func {
        ScalarFunction::Point => eval_point(args),
        ScalarFunction::Distance => eval_distance(args),
        ScalarFunction::WithinBBox => eval_within_bbox(args),
        _ => Err(GraphError::Validation {
            message: format!("{:?} is not a spatial function", func),
        }),
    }
}

/// point({x: ..., y: ...}) 또는 point(x, y)
fn eval_point(args: &[Value]) -> Result<Value> {
    match args.len() {
        1 => {
            // point({x: ..., y: ...}) 형식
            match &args[0] {
                Value::Map(map) => {
                    let x = extract_number(map.get("x"), "point() requires x as number")?;
                    let y = extract_number(map.get("y"), "point() requires y as number")?;

                    let mut result = IndexMap::new();
                    result.insert("x".to_string(), Value::Float(x));
                    result.insert("y".to_string(), Value::Float(y));
                    Ok(Value::Map(result))
                }
                _ => Err(GraphError::Validation {
                    message: "point() with 1 argument requires a map {x: ..., y: ...}".to_string(),
                }),
            }
        }
        2 => {
            // point(x, y) 형식
            let x = value_to_f64(&args[0], "point(x, y) requires x as number")?;
            let y = value_to_f64(&args[1], "point(x, y) requires y as number")?;

            let mut result = IndexMap::new();
            result.insert("x".to_string(), Value::Float(x));
            result.insert("y".to_string(), Value::Float(y));
            Ok(Value::Map(result))
        }
        _ => Err(GraphError::Validation {
            message: "point() requires 1 or 2 arguments".to_string(),
        }),
    }
}

/// distance(point1, point2) - 유클리드 거리
fn eval_distance(args: &[Value]) -> Result<Value> {
    if args.len() != 2 {
        return Err(GraphError::Validation {
            message: "distance() requires exactly 2 arguments".to_string(),
        });
    }

    let (x1, y1) = extract_point(&args[0], "distance()")?;
    let (x2, y2) = extract_point(&args[1], "distance()")?;

    let distance = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    Ok(Value::Float(distance))
}

/// withinBBox(point, minPoint, maxPoint) - 점이 경계 사각형 내에 있는지
fn eval_within_bbox(args: &[Value]) -> Result<Value> {
    if args.len() != 3 {
        return Err(GraphError::Validation {
            message: "withinBBox() requires exactly 3 arguments".to_string(),
        });
    }

    let (px, py) = extract_point(&args[0], "withinBBox() point")?;
    let (min_x, min_y) = extract_point(&args[1], "withinBBox() min point")?;
    let (max_x, max_y) = extract_point(&args[2], "withinBBox() max point")?;

    let within = px >= min_x && px <= max_x && py >= min_y && py <= max_y;
    Ok(Value::Boolean(within))
}

// === 헬퍼 함수 ===

/// Map에서 x, y 좌표 추출
fn extract_point(value: &Value, context: &str) -> Result<(f64, f64)> {
    match value {
        Value::Map(map) => {
            let x = extract_number(map.get("x"), &format!("{} requires x as number", context))?;
            let y = extract_number(map.get("y"), &format!("{} requires y as number", context))?;
            Ok((x, y))
        }
        _ => Err(GraphError::Validation {
            message: format!("{} requires point argument {{x: ..., y: ...}}", context),
        }),
    }
}

/// Option<&Value>에서 숫자 추출
fn extract_number(value: Option<&Value>, error_msg: &str) -> Result<f64> {
    match value {
        Some(Value::Float(f)) => Ok(*f),
        Some(Value::Integer(i)) => Ok(*i as f64),
        _ => Err(GraphError::Validation {
            message: error_msg.to_string(),
        }),
    }
}

/// Value를 f64로 변환
fn value_to_f64(value: &Value, error_msg: &str) -> Result<f64> {
    match value {
        Value::Float(f) => Ok(*f),
        Value::Integer(i) => Ok(*i as f64),
        _ => Err(GraphError::Validation {
            message: error_msg.to_string(),
        }),
    }
}
