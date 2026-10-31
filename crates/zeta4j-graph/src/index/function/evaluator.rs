// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 함수 표현식 평가기
//!
//! 함수 표현식을 평가하여 값을 계산합니다.

use super::FunctionExpression;
use crate::core::Value;
use chrono::{Datelike, Timelike};

/// 함수 표현식 평가기
#[derive(Debug, Clone, Default)]
pub struct FunctionEvaluator;

impl FunctionEvaluator {
    /// 새 평가기 생성
    pub fn new() -> Self {
        Self
    }

    /// 함수 표현식을 평가하여 값 계산
    ///
    /// # Arguments
    /// * `expr` - 함수 표현식
    /// * `value` - 원본 프로퍼티 값
    ///
    /// # Returns
    /// 계산된 값. 타입 불일치 시 None
    pub fn evaluate(&self, expr: &FunctionExpression, value: &Value) -> Option<Value> {
        match expr {
            // ===== 문자열 함수 =====
            FunctionExpression::Lower(_) => self.eval_lower(value),
            FunctionExpression::Upper(_) => self.eval_upper(value),
            FunctionExpression::Trim(_) => self.eval_trim(value),
            FunctionExpression::LTrim(_) => self.eval_ltrim(value),
            FunctionExpression::RTrim(_) => self.eval_rtrim(value),
            FunctionExpression::Length(_) => self.eval_length(value),

            // ===== 숫자 함수 =====
            FunctionExpression::Abs(_) => self.eval_abs(value),
            FunctionExpression::Floor(_) => self.eval_floor(value),
            FunctionExpression::Ceil(_) => self.eval_ceil(value),
            FunctionExpression::Round(_) => self.eval_round(value),
            FunctionExpression::Sign(_) => self.eval_sign(value),

            // ===== 날짜/시간 함수 =====
            FunctionExpression::Year(_) => self.eval_year(value),
            FunctionExpression::Month(_) => self.eval_month(value),
            FunctionExpression::Day(_) => self.eval_day(value),
            FunctionExpression::Hour(_) => self.eval_hour(value),
            FunctionExpression::Minute(_) => self.eval_minute(value),
            FunctionExpression::Second(_) => self.eval_second(value),
            FunctionExpression::DayOfWeek(_) => self.eval_day_of_week(value),
            FunctionExpression::DayOfYear(_) => self.eval_day_of_year(value),
            FunctionExpression::Quarter(_) => self.eval_quarter(value),
            FunctionExpression::Week(_) => self.eval_week(value),

            // ===== 컬렉션 함수 =====
            FunctionExpression::Size(_) => self.eval_size(value),
        }
    }

    // ===== 문자열 함수 구현 =====

    fn eval_lower(&self, value: &Value) -> Option<Value> {
        match value {
            Value::String(s) => Some(Value::String(s.to_lowercase())),
            _ => None,
        }
    }

    fn eval_upper(&self, value: &Value) -> Option<Value> {
        match value {
            Value::String(s) => Some(Value::String(s.to_uppercase())),
            _ => None,
        }
    }

    fn eval_trim(&self, value: &Value) -> Option<Value> {
        match value {
            Value::String(s) => Some(Value::String(s.trim().to_string())),
            _ => None,
        }
    }

    fn eval_ltrim(&self, value: &Value) -> Option<Value> {
        match value {
            Value::String(s) => Some(Value::String(s.trim_start().to_string())),
            _ => None,
        }
    }

    fn eval_rtrim(&self, value: &Value) -> Option<Value> {
        match value {
            Value::String(s) => Some(Value::String(s.trim_end().to_string())),
            _ => None,
        }
    }

    fn eval_length(&self, value: &Value) -> Option<Value> {
        match value {
            Value::String(s) => Some(Value::Integer(s.chars().count() as i64)),
            _ => None,
        }
    }

    // ===== 숫자 함수 구현 =====

    fn eval_abs(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Integer(i) => Some(Value::Integer(i.abs())),
            Value::Float(f) => Some(Value::Float(f.abs())),
            _ => None,
        }
    }

    fn eval_floor(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Float(f) => Some(Value::Integer(f.floor() as i64)),
            Value::Integer(i) => Some(Value::Integer(*i)),
            _ => None,
        }
    }

    fn eval_ceil(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Float(f) => Some(Value::Integer(f.ceil() as i64)),
            Value::Integer(i) => Some(Value::Integer(*i)),
            _ => None,
        }
    }

    fn eval_round(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Float(f) => Some(Value::Integer(f.round() as i64)),
            Value::Integer(i) => Some(Value::Integer(*i)),
            _ => None,
        }
    }

    fn eval_sign(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Integer(i) => Some(Value::Integer(i.signum())),
            Value::Float(f) => {
                if f.is_nan() {
                    None
                } else if *f > 0.0 {
                    Some(Value::Integer(1))
                } else if *f < 0.0 {
                    Some(Value::Integer(-1))
                } else {
                    Some(Value::Integer(0))
                }
            }
            _ => None,
        }
    }

    // ===== 날짜/시간 함수 구현 =====

    fn eval_year(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Date(d) => Some(Value::Integer(d.year() as i64)),
            Value::DateTime(dt) => Some(Value::Integer(dt.year() as i64)),
            Value::LocalDateTime(ldt) => Some(Value::Integer(ldt.year() as i64)),
            _ => None,
        }
    }

    fn eval_month(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Date(d) => Some(Value::Integer(d.month() as i64)),
            Value::DateTime(dt) => Some(Value::Integer(dt.month() as i64)),
            Value::LocalDateTime(ldt) => Some(Value::Integer(ldt.month() as i64)),
            _ => None,
        }
    }

    fn eval_day(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Date(d) => Some(Value::Integer(d.day() as i64)),
            Value::DateTime(dt) => Some(Value::Integer(dt.day() as i64)),
            Value::LocalDateTime(ldt) => Some(Value::Integer(ldt.day() as i64)),
            _ => None,
        }
    }

    fn eval_hour(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Time(t) => Some(Value::Integer(t.hour() as i64)),
            Value::DateTime(dt) => Some(Value::Integer(dt.hour() as i64)),
            Value::LocalDateTime(ldt) => Some(Value::Integer(ldt.hour() as i64)),
            _ => None,
        }
    }

    fn eval_minute(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Time(t) => Some(Value::Integer(t.minute() as i64)),
            Value::DateTime(dt) => Some(Value::Integer(dt.minute() as i64)),
            Value::LocalDateTime(ldt) => Some(Value::Integer(ldt.minute() as i64)),
            _ => None,
        }
    }

    fn eval_second(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Time(t) => Some(Value::Integer(t.second() as i64)),
            Value::DateTime(dt) => Some(Value::Integer(dt.second() as i64)),
            Value::LocalDateTime(ldt) => Some(Value::Integer(ldt.second() as i64)),
            _ => None,
        }
    }

    fn eval_day_of_week(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Date(d) => Some(Value::Integer(
                d.weekday().num_days_from_sunday() as i64 + 1,
            )),
            Value::DateTime(dt) => Some(Value::Integer(
                dt.weekday().num_days_from_sunday() as i64 + 1,
            )),
            Value::LocalDateTime(ldt) => Some(Value::Integer(
                ldt.weekday().num_days_from_sunday() as i64 + 1,
            )),
            _ => None,
        }
    }

    fn eval_day_of_year(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Date(d) => Some(Value::Integer(d.ordinal() as i64)),
            Value::DateTime(dt) => Some(Value::Integer(dt.ordinal() as i64)),
            Value::LocalDateTime(ldt) => Some(Value::Integer(ldt.ordinal() as i64)),
            _ => None,
        }
    }

    fn eval_quarter(&self, value: &Value) -> Option<Value> {
        let month = match value {
            Value::Date(d) => d.month(),
            Value::DateTime(dt) => dt.month(),
            Value::LocalDateTime(ldt) => ldt.month(),
            _ => return None,
        };
        let quarter = ((month - 1) / 3) + 1;
        Some(Value::Integer(quarter as i64))
    }

    fn eval_week(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Date(d) => Some(Value::Integer(d.iso_week().week() as i64)),
            Value::DateTime(dt) => Some(Value::Integer(dt.iso_week().week() as i64)),
            Value::LocalDateTime(ldt) => Some(Value::Integer(ldt.iso_week().week() as i64)),
            _ => None,
        }
    }

    // ===== 컬렉션 함수 구현 =====

    fn eval_size(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Array(arr) => Some(Value::Integer(arr.len() as i64)),
            Value::String(s) => Some(Value::Integer(s.len() as i64)),
            Value::Map(m) => Some(Value::Integer(m.len() as i64)),
            _ => None,
        }
    }
}
