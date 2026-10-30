//! 문자열 스칼라 함수 구현
//!
//! toUpper, toLower, trim, replace, substring 등

use super::{validate_arg_count, validate_arg_count_range};
use crate::ast::ScalarFunction;
use crate::{check_args, propagate_null};
use zeta4j_common::{ZetaError, codes};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

/// 문자열 함수 실행
pub fn evaluate(func: &ScalarFunction, args: &[Value]) -> Result<Value> {
    match func {
        ScalarFunction::ToUpper => {
            check_args!(args, 1, "toUpper");
            propagate_null!(args, first);
            match &args[0] {
                Value::String(s) => Ok(Value::String(s.to_uppercase())),
                _ => Err(GraphError::Zeta(ZetaError::TypeMismatch {
                    code: codes::cypher::TYPE_MISMATCH,
                    expected: "String".to_string(),
                    found: args[0].type_name().to_string(),
                })),
            }
        }

        ScalarFunction::ToLower => {
            check_args!(args, 1, "toLower");
            propagate_null!(args, first);
            match &args[0] {
                Value::String(s) => Ok(Value::String(s.to_lowercase())),
                _ => Err(GraphError::Zeta(ZetaError::TypeMismatch {
                    code: codes::cypher::TYPE_MISMATCH,
                    expected: "String".to_string(),
                    found: args[0].type_name().to_string(),
                })),
            }
        }

        ScalarFunction::Trim => {
            check_args!(args, 1, "trim");
            propagate_null!(args, first);
            match &args[0] {
                Value::String(s) => Ok(Value::String(s.trim().to_string())),
                _ => Err(GraphError::Zeta(ZetaError::TypeMismatch {
                    code: codes::cypher::TYPE_MISMATCH,
                    expected: "String".to_string(),
                    found: args[0].type_name().to_string(),
                })),
            }
        }

        ScalarFunction::LTrim => {
            check_args!(args, 1, "ltrim");
            propagate_null!(args, first);
            match &args[0] {
                Value::String(s) => Ok(Value::String(s.trim_start().to_string())),
                _ => Err(GraphError::Zeta(ZetaError::TypeMismatch {
                    code: codes::cypher::TYPE_MISMATCH,
                    expected: "String".to_string(),
                    found: args[0].type_name().to_string(),
                })),
            }
        }

        ScalarFunction::RTrim => {
            check_args!(args, 1, "rtrim");
            propagate_null!(args, first);
            match &args[0] {
                Value::String(s) => Ok(Value::String(s.trim_end().to_string())),
                _ => Err(GraphError::Zeta(ZetaError::TypeMismatch {
                    code: codes::cypher::TYPE_MISMATCH,
                    expected: "String".to_string(),
                    found: args[0].type_name().to_string(),
                })),
            }
        }

        ScalarFunction::Replace => {
            validate_arg_count("replace", args, 3)?;
            match (&args[0], &args[1], &args[2]) {
                (Value::String(s), Value::String(search), Value::String(replacement)) => Ok(
                    Value::String(s.replace(search.as_str(), replacement.as_str())),
                ),
                (Value::Null, _, _) | (_, Value::Null, _) | (_, _, Value::Null) => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "replace() expected (String, String, String), got ({}, {}, {})",
                        args[0].type_name(),
                        args[1].type_name(),
                        args[2].type_name()
                    ),
                }),
            }
        }

        ScalarFunction::Reverse => {
            validate_arg_count("reverse", args, 1)?;
            match &args[0] {
                Value::String(s) => Ok(Value::String(s.chars().rev().collect())),
                Value::Array(arr) => Ok(Value::Array(arr.iter().rev().cloned().collect())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "reverse() expected String or List, got {}",
                        args[0].type_name()
                    ),
                }),
            }
        }

        ScalarFunction::Substring => {
            validate_arg_count_range("substring", args, 2, 3)?;
            match &args[0] {
                Value::String(s) => {
                    let start = match &args[1] {
                        Value::Integer(i) => {
                            if *i < 0 {
                                return Err(GraphError::Validation {
                                    message: "substring() start index cannot be negative"
                                        .to_string(),
                                });
                            }
                            *i as usize
                        }
                        _ => {
                            return Err(GraphError::Validation {
                                message: format!(
                                    "substring() start expected Integer, got {}",
                                    args[1].type_name()
                                ),
                            });
                        }
                    };

                    let chars: Vec<char> = s.chars().collect();
                    if start >= chars.len() {
                        return Ok(Value::String(String::new()));
                    }

                    let length = if args.len() == 3 {
                        match &args[2] {
                            Value::Integer(i) => {
                                if *i < 0 {
                                    return Err(GraphError::Validation {
                                        message: "substring() length cannot be negative"
                                            .to_string(),
                                    });
                                }
                                Some(*i as usize)
                            }
                            Value::Null => None,
                            _ => {
                                return Err(GraphError::Validation {
                                    message: format!(
                                        "substring() length expected Integer, got {}",
                                        args[2].type_name()
                                    ),
                                });
                            }
                        }
                    } else {
                        None
                    };

                    let result: String = match length {
                        Some(len) => chars.iter().skip(start).take(len).collect(),
                        None => chars.iter().skip(start).collect(),
                    };

                    Ok(Value::String(result))
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("substring() expected String, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Left => {
            validate_arg_count("left", args, 2)?;
            match (&args[0], &args[1]) {
                (Value::String(s), Value::Integer(n)) => {
                    if *n < 0 {
                        return Err(GraphError::Validation {
                            message: "left() length cannot be negative".to_string(),
                        });
                    }
                    let chars: Vec<char> = s.chars().collect();
                    let result: String = chars.iter().take(*n as usize).collect();
                    Ok(Value::String(result))
                }
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "left() expected (String, Integer), got ({}, {})",
                        args[0].type_name(),
                        args[1].type_name()
                    ),
                }),
            }
        }

        ScalarFunction::Right => {
            validate_arg_count("right", args, 2)?;
            match (&args[0], &args[1]) {
                (Value::String(s), Value::Integer(n)) => {
                    if *n < 0 {
                        return Err(GraphError::Validation {
                            message: "right() length cannot be negative".to_string(),
                        });
                    }
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len();
                    let n = *n as usize;
                    let start = len.saturating_sub(n);
                    let result: String = chars.iter().skip(start).collect();
                    Ok(Value::String(result))
                }
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "right() expected (String, Integer), got ({}, {})",
                        args[0].type_name(),
                        args[1].type_name()
                    ),
                }),
            }
        }

        ScalarFunction::Split => {
            validate_arg_count("split", args, 2)?;
            match (&args[0], &args[1]) {
                (Value::String(s), Value::String(delimiter)) => {
                    let parts: Vec<Value> = s
                        .split(delimiter.as_str())
                        .map(|p| Value::String(p.to_string()))
                        .collect();
                    Ok(Value::Array(parts))
                }
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "split() expected (String, String), got ({}, {})",
                        args[0].type_name(),
                        args[1].type_name()
                    ),
                }),
            }
        }

        ScalarFunction::ToString => {
            validate_arg_count("toString", args, 1)?;
            match &args[0] {
                Value::String(s) => Ok(Value::String(s.clone())),
                Value::Integer(i) => Ok(Value::String(i.to_string())),
                Value::Float(f) => Ok(Value::String(f.to_string())),
                Value::Boolean(b) => Ok(Value::String(b.to_string())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "toString() cannot convert {} to String",
                        args[0].type_name()
                    ),
                }),
            }
        }

        _ => Err(GraphError::Validation {
            message: format!("Unknown string function: {:?}", func),
        }),
    }
}
