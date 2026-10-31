// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 시간/날짜 관련 헬퍼 함수 모듈
//!
//! Date, Time, DateTime, Duration 타입의 속성 접근 및 파싱

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use zeta4j_graph::core::{CypherDuration, Value};
use zeta4j_graph::{GraphError, Result};

/// Date 타입의 속성 접근 (year, month, day, dayOfWeek, dayOfYear)
pub fn get_date_property(d: &NaiveDate, prop: &str) -> Result<Value> {
    match prop {
        "year" => Ok(Value::Integer(d.year() as i64)),
        "month" => Ok(Value::Integer(d.month() as i64)),
        "day" => Ok(Value::Integer(d.day() as i64)),
        "dayOfWeek" | "dayofweek" => {
            // 월요일=1, 일요일=7 (ISO 8601)
            Ok(Value::Integer(
                d.weekday().num_days_from_monday() as i64 + 1,
            ))
        }
        "dayOfYear" | "dayofyear" => Ok(Value::Integer(d.ordinal() as i64)),
        "quarter" => Ok(Value::Integer(((d.month() - 1) / 3 + 1) as i64)),
        "week" | "weekOfYear" => Ok(Value::Integer(d.iso_week().week() as i64)),
        _ => Err(GraphError::Validation {
            message: format!("Unknown date property: {}", prop),
        }),
    }
}

/// Time 타입의 속성 접근 (hour, minute, second, millisecond, microsecond, nanosecond)
pub fn get_time_property(t: &NaiveTime, prop: &str) -> Result<Value> {
    match prop {
        "hour" => Ok(Value::Integer(t.hour() as i64)),
        "minute" => Ok(Value::Integer(t.minute() as i64)),
        "second" => Ok(Value::Integer(t.second() as i64)),
        "millisecond" => Ok(Value::Integer((t.nanosecond() / 1_000_000) as i64)),
        "microsecond" => Ok(Value::Integer((t.nanosecond() / 1_000) as i64)),
        "nanosecond" => Ok(Value::Integer(t.nanosecond() as i64)),
        _ => Err(GraphError::Validation {
            message: format!("Unknown time property: {}", prop),
        }),
    }
}

/// DateTime 타입의 속성 접근 (날짜 + 시간 속성)
pub fn get_datetime_property(dt: &chrono::DateTime<Utc>, prop: &str) -> Result<Value> {
    match prop {
        // 날짜 속성
        "year" => Ok(Value::Integer(dt.year() as i64)),
        "month" => Ok(Value::Integer(dt.month() as i64)),
        "day" => Ok(Value::Integer(dt.day() as i64)),
        "dayOfWeek" | "dayofweek" => Ok(Value::Integer(
            dt.weekday().num_days_from_monday() as i64 + 1,
        )),
        "dayOfYear" | "dayofyear" => Ok(Value::Integer(dt.ordinal() as i64)),
        "quarter" => Ok(Value::Integer(((dt.month() - 1) / 3 + 1) as i64)),
        "week" | "weekOfYear" => Ok(Value::Integer(dt.iso_week().week() as i64)),
        // 시간 속성
        "hour" => Ok(Value::Integer(dt.hour() as i64)),
        "minute" => Ok(Value::Integer(dt.minute() as i64)),
        "second" => Ok(Value::Integer(dt.second() as i64)),
        "millisecond" => Ok(Value::Integer((dt.nanosecond() / 1_000_000) as i64)),
        "microsecond" => Ok(Value::Integer((dt.nanosecond() / 1_000) as i64)),
        "nanosecond" => Ok(Value::Integer(dt.nanosecond() as i64)),
        // epoch (Unix timestamp)
        "epochSeconds" | "epochseconds" => Ok(Value::Integer(dt.timestamp())),
        "epochMillis" | "epochmillis" => Ok(Value::Integer(dt.timestamp_millis())),
        _ => Err(GraphError::Validation {
            message: format!("Unknown datetime property: {}", prop),
        }),
    }
}

/// LocalDateTime 타입의 속성 접근
pub fn get_local_datetime_property(ldt: &NaiveDateTime, prop: &str) -> Result<Value> {
    match prop {
        // 날짜 속성
        "year" => Ok(Value::Integer(ldt.year() as i64)),
        "month" => Ok(Value::Integer(ldt.month() as i64)),
        "day" => Ok(Value::Integer(ldt.day() as i64)),
        "dayOfWeek" | "dayofweek" => Ok(Value::Integer(
            ldt.weekday().num_days_from_monday() as i64 + 1,
        )),
        "dayOfYear" | "dayofyear" => Ok(Value::Integer(ldt.ordinal() as i64)),
        "quarter" => Ok(Value::Integer(((ldt.month() - 1) / 3 + 1) as i64)),
        "week" | "weekOfYear" => Ok(Value::Integer(ldt.iso_week().week() as i64)),
        // 시간 속성
        "hour" => Ok(Value::Integer(ldt.hour() as i64)),
        "minute" => Ok(Value::Integer(ldt.minute() as i64)),
        "second" => Ok(Value::Integer(ldt.second() as i64)),
        "millisecond" => Ok(Value::Integer((ldt.nanosecond() / 1_000_000) as i64)),
        "microsecond" => Ok(Value::Integer((ldt.nanosecond() / 1_000) as i64)),
        "nanosecond" => Ok(Value::Integer(ldt.nanosecond() as i64)),
        // epoch
        "epochSeconds" | "epochseconds" => Ok(Value::Integer(ldt.and_utc().timestamp())),
        "epochMillis" | "epochmillis" => Ok(Value::Integer(ldt.and_utc().timestamp_millis())),
        _ => Err(GraphError::Validation {
            message: format!("Unknown localdatetime property: {}", prop),
        }),
    }
}

/// Duration 타입의 속성 접근
pub fn get_duration_property(dur: &CypherDuration, prop: &str) -> Result<Value> {
    match prop {
        "years" => Ok(Value::Integer(dur.years())),
        "months" => Ok(Value::Integer(dur.months)),
        "monthsOfYear" | "monthsofyear" => Ok(Value::Integer(dur.months_of_year())),
        "weeks" => Ok(Value::Integer(dur.days / 7)),
        "days" => Ok(Value::Integer(dur.days)),
        "daysOfWeek" | "daysofweek" => Ok(Value::Integer(dur.days % 7)),
        "hours" => Ok(Value::Integer(dur.hours())),
        "hoursOfDay" | "hoursofday" => Ok(Value::Integer(dur.hours() % 24)),
        "minutes" => Ok(Value::Integer(dur.seconds / 60)),
        "minutesOfHour" | "minutesofhour" => Ok(Value::Integer(dur.minutes_of_hour())),
        "seconds" => Ok(Value::Integer(dur.seconds)),
        "secondsOfMinute" | "secondsofminute" => Ok(Value::Integer(dur.seconds_of_minute())),
        "milliseconds" => Ok(Value::Integer(dur.milliseconds())),
        "microseconds" => Ok(Value::Integer(dur.nanoseconds / 1000)),
        "nanoseconds" => Ok(Value::Integer(dur.nanoseconds)),
        _ => Err(GraphError::Validation {
            message: format!("Unknown duration property: {}", prop),
        }),
    }
}

/// ISO 8601 duration 문자열 파싱
pub fn parse_iso8601_duration(s: &str) -> Result<CypherDuration> {
    // P[n]Y[n]M[n]DT[n]H[n]M[n]S 형식 파싱
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

/// 인자 개수 검증
pub fn validate_arg_count(name: &str, args: &[Value], expected: usize) -> Result<()> {
    if args.len() != expected {
        return Err(GraphError::Validation {
            message: format!(
                "{} requires {} argument(s), got {}",
                name,
                expected,
                args.len()
            ),
        });
    }
    Ok(())
}
