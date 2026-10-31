// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 벡터 유사도 스칼라 함수 구현
//!
//! HNSW 인덱스의 거리 함수를 Cypher에서 사용할 수 있도록 래핑합니다.
//!
//! # 지원 함수
//!
//! - `vector.similarity.cosine(a, b)` - 코사인 유사도 [0, 1] (Neo4j 호환: (cos+1)/2)
//! - `vector.similarity.euclidean(a, b)` - 유클리드 유사도 [0, 1] (Neo4j 호환: 1/(1+dist²))
//! - `vector.similarity.dotProduct(a, b)` - 내적 (-∞, ∞)
//! - `vector.distance(a, b)` - 코사인 거리 [0, 2]
//! - `vector.dimensions(v)` - 차원 수
//! - `vector.normalize(v)` - L2 정규화
//!
//! # 예제
//!
//! ```cypher
//! MATCH (a:Doc {id: 1}), (b:Doc {id: 2})
//! RETURN vector.similarity.cosine(a.embedding, b.embedding) AS similarity
//! ```

use crate::ast::ScalarFunction;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

use super::validate_arg_count;

// 벡터 연산 함수 (zeta4j-vector에 의존하지 않는 로컬 구현)

/// 코사인 유사도 계산 (Neo4j 호환)
/// 원래 코사인 유사도 [-1, 1]을 [0, 1]로 정규화: (cos + 1) / 2
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.5 // (0 + 1) / 2
    } else {
        let raw_cosine = dot / (norm_a * norm_b);
        (raw_cosine + 1.0) / 2.0 // [-1, 1] → [0, 1]
    }
}

/// 유클리드 유사도 계산 (Neo4j 호환)
/// 거리를 유사도로 변환: 1 / (1 + distance²)
fn euclidean_similarity(a: &[f32], b: &[f32]) -> f32 {
    let distance_squared: f32 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum();
    1.0 / (1.0 + distance_squared)
}

/// 내적 계산
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// 벡터 함수 실행
pub fn evaluate(func: &ScalarFunction, args: &[Value]) -> Result<Value> {
    match func {
        ScalarFunction::VectorCosineSimilarity => {
            validate_arg_count("vector.similarity.cosine", args, 2)?;
            let (a, b) = extract_two_vectors_owned(args)?;
            validate_dimensions(&a, &b)?;

            let sim = cosine_similarity(&a, &b);
            Ok(Value::Float(sim as f64))
        }

        ScalarFunction::VectorEuclideanDistance => {
            validate_arg_count("vector.similarity.euclidean", args, 2)?;
            let (a, b) = extract_two_vectors_owned(args)?;
            validate_dimensions(&a, &b)?;

            let sim = euclidean_similarity(&a, &b);
            Ok(Value::Float(sim as f64))
        }

        ScalarFunction::VectorDotProduct => {
            validate_arg_count("vector.similarity.dotProduct", args, 2)?;
            let (a, b) = extract_two_vectors_owned(args)?;
            validate_dimensions(&a, &b)?;

            let dot = dot_product(&a, &b);
            Ok(Value::Float(dot as f64))
        }

        ScalarFunction::VectorDistance => {
            validate_arg_count("vector.distance", args, 2)?;
            let (a, b) = extract_two_vectors_owned(args)?;
            validate_dimensions(&a, &b)?;

            // 코사인 거리 = 1 - 코사인 유사도
            let sim = cosine_similarity(&a, &b);
            let distance = 1.0 - sim;
            Ok(Value::Float(distance as f64))
        }

        ScalarFunction::VectorDimensions => {
            validate_arg_count("vector.dimensions", args, 1)?;

            match &args[0] {
                Value::Vector(v) => Ok(Value::Integer(v.dimensions() as i64)),
                Value::Array(arr) => Ok(Value::Integer(arr.len() as i64)),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "vector.dimensions() expected Vector or Array, got {}",
                        args[0].type_name()
                    ),
                }),
            }
        }

        ScalarFunction::VectorNormalize => {
            validate_arg_count("vector.normalize", args, 1)?;

            match &args[0] {
                Value::Vector(v) => {
                    let normalized = v.normalized();
                    Ok(Value::Vector(normalized))
                }
                Value::Array(_) => {
                    // 배열을 벡터로 변환하고 정규화
                    let vec = value_to_vec(&args[0], "Argument")?;
                    let vv = zeta4j_graph::core::VectorValue::new(vec).map_err(|e| {
                        GraphError::Validation {
                            message: e.to_string(),
                        }
                    })?;
                    Ok(Value::Vector(vv.normalized()))
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "vector.normalize() expected Vector or Array, got {}",
                        args[0].type_name()
                    ),
                }),
            }
        }

        _ => Err(GraphError::Validation {
            message: format!("Unknown vector function: {:?}", func),
        }),
    }
}

/// 값을 벡터로 변환 (Array도 지원)
fn value_to_vec(value: &Value, arg_name: &str) -> Result<Vec<f32>> {
    match value {
        Value::Vector(v) => Ok(v.as_slice().to_vec()),
        Value::Array(arr) => {
            // 배열을 f32 벡터로 변환
            let mut result = Vec::with_capacity(arr.len());
            for (i, v) in arr.iter().enumerate() {
                match v {
                    Value::Float(f) => result.push(*f as f32),
                    Value::Integer(n) => result.push(*n as f32),
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!(
                                "{} array element {} expected number, got {}",
                                arg_name,
                                i,
                                v.type_name()
                            ),
                        });
                    }
                }
            }
            if result.is_empty() {
                return Err(GraphError::Validation {
                    message: format!("{} array is empty", arg_name),
                });
            }
            Ok(result)
        }
        Value::Null => Err(GraphError::Validation {
            message: format!("{} is null", arg_name),
        }),
        _ => Err(GraphError::Validation {
            message: format!(
                "{} expected Vector or Array, got {}",
                arg_name,
                value.type_name()
            ),
        }),
    }
}

/// 두 벡터 추출 헬퍼 (owned vectors 반환)
fn extract_two_vectors_owned(args: &[Value]) -> Result<(Vec<f32>, Vec<f32>)> {
    let a = value_to_vec(&args[0], "First argument")?;
    let b = value_to_vec(&args[1], "Second argument")?;
    Ok((a, b))
}

/// 차원 일치 검증
fn validate_dimensions(a: &[f32], b: &[f32]) -> Result<()> {
    if a.len() != b.len() {
        return Err(GraphError::Validation {
            message: format!("Dimension mismatch: {} vs {}", a.len(), b.len()),
        });
    }
    Ok(())
}
