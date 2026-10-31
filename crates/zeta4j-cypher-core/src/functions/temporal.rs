// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 시간 함수 구현 모듈
//!
//! 시간 관련 함수들의 구현:
//! - date(): 날짜 생성/파싱
//! - datetime(): 날짜/시간 생성/파싱
//! - time(): 시간 생성/파싱
//! - localTime(): 로컬 시간 생성/파싱
//! - localDateTime(): 로컬 날짜/시간 생성/파싱
//! - duration(): 기간 생성

use crate::ast::ScalarFunction;
use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use zeta4j_graph::core::{CypherDuration, Value};
use zeta4j_graph::error::{GraphError, Result};

/// 시간 함수 평가
pub fn evaluate(func: &ScalarFunction, args: &[Value]) -> Result<Value> {
    match func {
        ScalarFunction::Date => eval_date(args),
        ScalarFunction::Datetime => eval_datetime(args),
        ScalarFunction::Time => eval_time(args),
        ScalarFunction::LocalTime => eval_local_time(args),
        ScalarFunction::LocalDateTime => eval_local_datetime(args),
        ScalarFunction::Duration => eval_duration(args),
        _ => Err(GraphError::Validation {
            message: format!("{:?} is not a temporal function", func),
        }),
    }
}

/// date() - 현재 날짜
/// date(string) - 문자열에서 날짜 파싱
/// date({year: ..., month: ..., day: ...}) - 맵에서 생성
fn eval_date(args: &[Value]) -> Result<Value> {
    if args.is_empty() {
        // 현재 날짜 반환 (로컬)
        let today = Local::now().date_naive();
        return Ok(Value::Date(today));
    }

    match &args[0] {
        Value::String(s) => {
            // ISO 8601 날짜 형식 파싱 (YYYY-MM-DD)
            match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                Ok(d) => Ok(Value::Date(d)),
                Err(_) => Err(GraphError::Validation {
                    message: format!("Cannot parse '{}' as date. Expected format: YYYY-MM-DD", s),
                }),
            }
        }
        Value::Map(map) => {
            let year = map
                .get("year")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| GraphError::Validation {
                    message: "date() requires 'year' field".to_string(),
                })? as i32;
            let month = map.get("month").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            let day = map.get("day").and_then(|v| v.as_integer()).unwrap_or(1) as u32;

            match NaiveDate::from_ymd_opt(year, month, day) {
                Some(d) => Ok(Value::Date(d)),
                None => Err(GraphError::Validation {
                    message: format!("Invalid date: {}-{}-{}", year, month, day),
                }),
            }
        }
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!("date() expected String or Map, got {}", args[0].type_name()),
        }),
    }
}

/// datetime() - 현재 UTC 날짜시간
/// datetime(string) - ISO 8601 문자열 파싱
/// datetime({...}) - 맵에서 생성
fn eval_datetime(args: &[Value]) -> Result<Value> {
    if args.is_empty() {
        return Ok(Value::DateTime(Utc::now()));
    }

    match &args[0] {
        Value::String(s) => {
            // RFC3339 형식 파싱 시도
            if let Ok(dt) = s.parse::<chrono::DateTime<Utc>>() {
                return Ok(Value::DateTime(dt));
            }
            // ISO 8601 형식 파싱 시도 (날짜시간)
            if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
                return Ok(Value::DateTime(ndt.and_utc()));
            }
            if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                return Ok(Value::DateTime(ndt.and_utc()));
            }
            Err(GraphError::Validation {
                message: format!("Cannot parse '{}' as datetime", s),
            })
        }
        Value::Map(map) => {
            let year = map
                .get("year")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| GraphError::Validation {
                    message: "datetime() requires 'year' field".to_string(),
                })? as i32;
            let month = map.get("month").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            let day = map.get("day").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            let hour = map.get("hour").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let minute = map.get("minute").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let second = map.get("second").and_then(|v| v.as_integer()).unwrap_or(0) as u32;

            match NaiveDate::from_ymd_opt(year, month, day)
                .and_then(|d| d.and_hms_opt(hour, minute, second))
            {
                Some(ndt) => Ok(Value::DateTime(ndt.and_utc())),
                None => Err(GraphError::Validation {
                    message: format!(
                        "Invalid datetime: {}-{}-{} {}:{}:{}",
                        year, month, day, hour, minute, second
                    ),
                }),
            }
        }
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!(
                "datetime() expected String or Map, got {}",
                args[0].type_name()
            ),
        }),
    }
}

/// time() - 현재 시간
/// time(string) - 문자열에서 시간 파싱
/// time({hour: ..., minute: ..., second: ...}) - 맵에서 생성
fn eval_time(args: &[Value]) -> Result<Value> {
    if args.is_empty() {
        let now = Local::now().time();
        return Ok(Value::Time(now));
    }

    match &args[0] {
        Value::String(s) => {
            // HH:MM:SS 또는 HH:MM:SS.sss 형식 파싱
            if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M:%S%.f") {
                return Ok(Value::Time(t));
            }
            if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M:%S") {
                return Ok(Value::Time(t));
            }
            if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M") {
                return Ok(Value::Time(t));
            }
            Err(GraphError::Validation {
                message: format!("Cannot parse '{}' as time. Expected format: HH:MM:SS", s),
            })
        }
        Value::Map(map) => {
            let hour = map.get("hour").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let minute = map.get("minute").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let second = map.get("second").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let nanosecond = map
                .get("nanosecond")
                .and_then(|v| v.as_integer())
                .or_else(|| {
                    map.get("millisecond")
                        .and_then(|v| v.as_integer())
                        .map(|ms| ms * 1_000_000)
                })
                .unwrap_or(0) as u32;

            match NaiveTime::from_hms_nano_opt(hour, minute, second, nanosecond) {
                Some(t) => Ok(Value::Time(t)),
                None => Err(GraphError::Validation {
                    message: format!("Invalid time: {}:{}:{}", hour, minute, second),
                }),
            }
        }
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!("time() expected String or Map, got {}", args[0].type_name()),
        }),
    }
}

/// localtime() - 현재 로컬 시간 (time과 동일하게 동작)
fn eval_local_time(args: &[Value]) -> Result<Value> {
    if args.is_empty() {
        let now = Local::now().time();
        return Ok(Value::Time(now));
    }

    // time()과 동일한 로직
    match &args[0] {
        Value::String(s) => {
            if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M:%S%.f") {
                return Ok(Value::Time(t));
            }
            if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M:%S") {
                return Ok(Value::Time(t));
            }
            if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M") {
                return Ok(Value::Time(t));
            }
            Err(GraphError::Validation {
                message: format!("Cannot parse '{}' as time", s),
            })
        }
        Value::Map(map) => {
            let hour = map.get("hour").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let minute = map.get("minute").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let second = map.get("second").and_then(|v| v.as_integer()).unwrap_or(0) as u32;

            match NaiveTime::from_hms_opt(hour, minute, second) {
                Some(t) => Ok(Value::Time(t)),
                None => Err(GraphError::Validation {
                    message: format!("Invalid time: {}:{}:{}", hour, minute, second),
                }),
            }
        }
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!(
                "localtime() expected String or Map, got {}",
                args[0].type_name()
            ),
        }),
    }
}

/// localdatetime() - 현재 로컬 날짜시간
/// localdatetime(string) - 문자열에서 파싱
/// localdatetime({...}) - 맵에서 생성
fn eval_local_datetime(args: &[Value]) -> Result<Value> {
    if args.is_empty() {
        return Ok(Value::LocalDateTime(Local::now().naive_local()));
    }

    match &args[0] {
        Value::String(s) => {
            if let Ok(ldt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
                return Ok(Value::LocalDateTime(ldt));
            }
            if let Ok(ldt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
                return Ok(Value::LocalDateTime(ldt));
            }
            if let Ok(ldt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                return Ok(Value::LocalDateTime(ldt));
            }
            Err(GraphError::Validation {
                message: format!("Cannot parse '{}' as localdatetime", s),
            })
        }
        Value::Map(map) => {
            let year = map
                .get("year")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| GraphError::Validation {
                    message: "localdatetime() requires 'year' field".to_string(),
                })? as i32;
            let month = map.get("month").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            let day = map.get("day").and_then(|v| v.as_integer()).unwrap_or(1) as u32;
            let hour = map.get("hour").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let minute = map.get("minute").and_then(|v| v.as_integer()).unwrap_or(0) as u32;
            let second = map.get("second").and_then(|v| v.as_integer()).unwrap_or(0) as u32;

            match NaiveDate::from_ymd_opt(year, month, day)
                .and_then(|d| d.and_hms_opt(hour, minute, second))
            {
                Some(ldt) => Ok(Value::LocalDateTime(ldt)),
                None => Err(GraphError::Validation {
                    message: format!(
                        "Invalid localdatetime: {}-{}-{} {}:{}:{}",
                        year, month, day, hour, minute, second
                    ),
                }),
            }
        }
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!(
                "localdatetime() expected String or Map, got {}",
                args[0].type_name()
            ),
        }),
    }
}

/// duration({years: ..., months: ..., days: ..., hours: ..., minutes: ..., seconds: ...})
/// duration(string) - ISO 8601 duration 파싱 (예: "P1Y2M3D")
fn eval_duration(args: &[Value]) -> Result<Value> {
    if args.len() != 1 {
        return Err(GraphError::Validation {
            message: format!("duration() requires 1 argument, got {}", args.len()),
        });
    }

    match &args[0] {
        Value::Map(map) => Ok(Value::Duration(CypherDuration::from_map(map))),
        Value::String(s) => {
            // ISO 8601 duration 파싱
            parse_iso8601_duration(s).map(Value::Duration)
        }
        Value::Null => Ok(Value::Null),
        _ => Err(GraphError::Validation {
            message: format!(
                "duration() expected Map or String, got {}",
                args[0].type_name()
            ),
        }),
    }
}

/// ISO 8601 duration 문자열 파싱
/// 형식: P[n]Y[n]M[n]DT[n]H[n]M[n]S
pub fn parse_iso8601_duration(s: &str) -> Result<CypherDuration> {
    if !s.starts_with('P') {
        return Err(GraphError::Validation {
            message: format!("Duration must start with 'P': {}", s),
        });
    }

    let s = &s[1..]; // 'P' 제거
    let (date_part, time_part) = if let Some(pos) = s.find('T') {
        (&s[..pos], Some(&s[pos + 1..]))
    } else {
        (s, None)
    };

    let mut years: i64 = 0;
    let mut months: i64 = 0;
    let mut days: i64 = 0;
    let mut hours: i64 = 0;
    let mut minutes: i64 = 0;
    let mut seconds: i64 = 0;

    // 날짜 부분 파싱
    let mut num_str = String::new();
    for c in date_part.chars() {
        if c.is_ascii_digit() || c == '-' {
            num_str.push(c);
        } else {
            let n: i64 = num_str.parse().unwrap_or(0);
            num_str.clear();
            match c {
                'Y' => years = n,
                'M' => months = n,
                'W' => days += n * 7,
                'D' => days = n,
                _ => {}
            }
        }
    }

    // 시간 부분 파싱
    if let Some(time) = time_part {
        num_str.clear();
        for c in time.chars() {
            if c.is_ascii_digit() || c == '.' || c == '-' {
                num_str.push(c);
            } else {
                let n: i64 = num_str.parse::<f64>().unwrap_or(0.0) as i64;
                num_str.clear();
                match c {
                    'H' => hours = n,
                    'M' => minutes = n,
                    'S' => seconds = n,
                    _ => {}
                }
            }
        }
    }

    Ok(CypherDuration::new(
        years * 12 + months,
        days,
        hours * 3600 + minutes * 60 + seconds,
        0,
    ))
}
