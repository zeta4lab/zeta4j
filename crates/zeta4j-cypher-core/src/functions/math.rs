//! 수학 스칼라 함수 구현
//!
//! abs, ceil, floor, round, sqrt, sin, cos 등

use super::{validate_arg_count, validate_arg_count_range};
use crate::ast::ScalarFunction;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

/// 수학 함수 실행
pub fn evaluate(func: &ScalarFunction, args: &[Value]) -> Result<Value> {
    match func {
        ScalarFunction::Abs => {
            validate_arg_count("abs", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Integer(i.abs())),
                Value::Float(f) => Ok(Value::Float(f.abs())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("abs() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Ceil => {
            validate_arg_count("ceil", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Integer(*i)),
                Value::Float(f) => Ok(Value::Integer(f.ceil() as i64)),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("ceil() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Floor => {
            validate_arg_count("floor", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Integer(*i)),
                Value::Float(f) => Ok(Value::Integer(f.floor() as i64)),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("floor() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Round => {
            validate_arg_count_range("round", args, 1, 2)?;
            let precision = if args.len() == 2 {
                match &args[1] {
                    Value::Integer(p) => *p as i32,
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!(
                                "round() precision expected Integer, got {}",
                                args[1].type_name()
                            ),
                        });
                    }
                }
            } else {
                0
            };

            match &args[0] {
                Value::Integer(i) => Ok(Value::Integer(*i)),
                Value::Float(f) => {
                    // Neo4j uses "round half up" (towards positive infinity)
                    // -7.5 -> -7, 3.5 -> 4
                    // Neo4j returns Float (not Integer) when input is Float
                    if precision == 0 {
                        Ok(Value::Float((f + 0.5).floor()))
                    } else {
                        let multiplier = 10f64.powi(precision);
                        Ok(Value::Float(((f * multiplier) + 0.5).floor() / multiplier))
                    }
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("round() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Sign => {
            validate_arg_count("sign", args, 1)?;
            match &args[0] {
                Value::Integer(i) => {
                    let sign = if *i > 0 {
                        1
                    } else if *i < 0 {
                        -1
                    } else {
                        0
                    };
                    Ok(Value::Integer(sign))
                }
                Value::Float(f) => {
                    let sign = if *f > 0.0 {
                        1
                    } else if *f < 0.0 {
                        -1
                    } else {
                        0
                    };
                    Ok(Value::Integer(sign))
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("sign() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Sqrt => {
            validate_arg_count("sqrt", args, 1)?;
            match &args[0] {
                Value::Integer(i) => {
                    if *i < 0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float((*i as f64).sqrt()))
                    }
                }
                Value::Float(f) => {
                    if *f < 0.0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float(f.sqrt()))
                    }
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("sqrt() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Rand => {
            validate_arg_count("rand", args, 0)?;
            Ok(Value::Float(rand::random::<f64>()))
        }

        ScalarFunction::Log => {
            validate_arg_count("log", args, 1)?;
            match &args[0] {
                Value::Integer(i) => {
                    if *i <= 0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float((*i as f64).ln()))
                    }
                }
                Value::Float(f) => {
                    if *f <= 0.0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float(f.ln()))
                    }
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("log() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Log10 => {
            validate_arg_count("log10", args, 1)?;
            match &args[0] {
                Value::Integer(i) => {
                    if *i <= 0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float((*i as f64).log10()))
                    }
                }
                Value::Float(f) => {
                    if *f <= 0.0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float(f.log10()))
                    }
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("log10() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Exp => {
            validate_arg_count("exp", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Float((*i as f64).exp())),
                Value::Float(f) => Ok(Value::Float(f.exp())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("exp() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Power => {
            validate_arg_count("power", args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Integer(base), Value::Integer(exp)) => {
                    Ok(Value::Float((*base as f64).powf(*exp as f64)))
                }
                (Value::Integer(base), Value::Float(exp)) => {
                    Ok(Value::Float((*base as f64).powf(*exp)))
                }
                (Value::Float(base), Value::Integer(exp)) => {
                    Ok(Value::Float(base.powf(*exp as f64)))
                }
                (Value::Float(base), Value::Float(exp)) => Ok(Value::Float(base.powf(*exp))),
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "power() expected (Number, Number), got ({}, {})",
                        args[0].type_name(),
                        args[1].type_name()
                    ),
                }),
            }
        }

        ScalarFunction::Sin => {
            validate_arg_count("sin", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Float((*i as f64).sin())),
                Value::Float(f) => Ok(Value::Float(f.sin())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("sin() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Cos => {
            validate_arg_count("cos", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Float((*i as f64).cos())),
                Value::Float(f) => Ok(Value::Float(f.cos())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("cos() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Tan => {
            validate_arg_count("tan", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Float((*i as f64).tan())),
                Value::Float(f) => Ok(Value::Float(f.tan())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("tan() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Asin => {
            validate_arg_count("asin", args, 1)?;
            match &args[0] {
                Value::Integer(i) => {
                    let v = *i as f64;
                    if !(-1.0..=1.0).contains(&v) {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float(v.asin()))
                    }
                }
                Value::Float(f) => {
                    if !(-1.0..=1.0).contains(f) {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float(f.asin()))
                    }
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("asin() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Acos => {
            validate_arg_count("acos", args, 1)?;
            match &args[0] {
                Value::Integer(i) => {
                    let v = *i as f64;
                    if !(-1.0..=1.0).contains(&v) {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float(v.acos()))
                    }
                }
                Value::Float(f) => {
                    if !(-1.0..=1.0).contains(f) {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Float(f.acos()))
                    }
                }
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("acos() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Atan => {
            validate_arg_count("atan", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Float((*i as f64).atan())),
                Value::Float(f) => Ok(Value::Float(f.atan())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("atan() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Atan2 => {
            validate_arg_count("atan2", args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Integer(y), Value::Integer(x)) => {
                    Ok(Value::Float((*y as f64).atan2(*x as f64)))
                }
                (Value::Integer(y), Value::Float(x)) => Ok(Value::Float((*y as f64).atan2(*x))),
                (Value::Float(y), Value::Integer(x)) => Ok(Value::Float(y.atan2(*x as f64))),
                (Value::Float(y), Value::Float(x)) => Ok(Value::Float(y.atan2(*x))),
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!(
                        "atan2() expected (Number, Number), got ({}, {})",
                        args[0].type_name(),
                        args[1].type_name()
                    ),
                }),
            }
        }

        ScalarFunction::Degrees => {
            validate_arg_count("degrees", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Float((*i as f64).to_degrees())),
                Value::Float(f) => Ok(Value::Float(f.to_degrees())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("degrees() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Radians => {
            validate_arg_count("radians", args, 1)?;
            match &args[0] {
                Value::Integer(i) => Ok(Value::Float((*i as f64).to_radians())),
                Value::Float(f) => Ok(Value::Float(f.to_radians())),
                Value::Null => Ok(Value::Null),
                _ => Err(GraphError::Validation {
                    message: format!("radians() expected Number, got {}", args[0].type_name()),
                }),
            }
        }

        ScalarFunction::Pi => {
            validate_arg_count("pi", args, 0)?;
            Ok(Value::Float(std::f64::consts::PI))
        }

        ScalarFunction::E => {
            validate_arg_count("e", args, 0)?;
            Ok(Value::Float(std::f64::consts::E))
        }

        _ => Err(GraphError::Validation {
            message: format!("Unknown math function: {:?}", func),
        }),
    }
}
