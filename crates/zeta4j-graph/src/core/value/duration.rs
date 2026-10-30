//! Cypher Duration 타입
//!
//! ISO 8601 기반의 기간 표현을 정의합니다.

use std::fmt;
use std::hash::{Hash, Hasher};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::Value;

/// Cypher Duration 타입 (ISO 8601 기반)
/// Neo4j와 호환되는 기간 표현
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CypherDuration {
    /// 개월 수 (년 * 12 + 월)
    pub months: i64,
    /// 일 수
    pub days: i64,
    /// 초 수 (시간 * 3600 + 분 * 60 + 초)
    pub seconds: i64,
    /// 나노초 (밀리초 * 1_000_000 + 마이크로초 * 1000 + 나노초)
    pub nanoseconds: i64,
}

impl CypherDuration {
    /// 새 Duration 생성
    pub fn new(months: i64, days: i64, seconds: i64, nanoseconds: i64) -> Self {
        Self {
            months,
            days,
            seconds,
            nanoseconds,
        }
    }

    /// 빈 Duration 생성
    pub fn zero() -> Self {
        Self {
            months: 0,
            days: 0,
            seconds: 0,
            nanoseconds: 0,
        }
    }

    /// 연도 수 반환
    pub fn years(&self) -> i64 {
        self.months / 12
    }

    /// 월 수 반환 (연도 제외)
    pub fn months_of_year(&self) -> i64 {
        self.months % 12
    }

    /// 시간 수 반환
    pub fn hours(&self) -> i64 {
        self.seconds / 3600
    }

    /// 분 수 반환 (시간 제외)
    pub fn minutes_of_hour(&self) -> i64 {
        (self.seconds % 3600) / 60
    }

    /// 초 수 반환 (분 제외)
    pub fn seconds_of_minute(&self) -> i64 {
        self.seconds % 60
    }

    /// 밀리초 수 반환
    pub fn milliseconds(&self) -> i64 {
        self.nanoseconds / 1_000_000
    }

    /// 맵에서 Duration 생성 (Cypher duration({years: 1, months: 2, ...}) 형식)
    pub fn from_map(map: &IndexMap<String, Value>) -> Self {
        let years = map.get("years").and_then(|v| v.as_integer()).unwrap_or(0);
        let months = map.get("months").and_then(|v| v.as_integer()).unwrap_or(0);
        let weeks = map.get("weeks").and_then(|v| v.as_integer()).unwrap_or(0);
        let days = map.get("days").and_then(|v| v.as_integer()).unwrap_or(0);
        let hours = map.get("hours").and_then(|v| v.as_integer()).unwrap_or(0);
        let minutes = map.get("minutes").and_then(|v| v.as_integer()).unwrap_or(0);
        let seconds = map.get("seconds").and_then(|v| v.as_integer()).unwrap_or(0);
        let milliseconds = map
            .get("milliseconds")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        let microseconds = map
            .get("microseconds")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        let nanoseconds = map
            .get("nanoseconds")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);

        Self {
            months: years * 12 + months,
            days: weeks * 7 + days,
            seconds: hours * 3600 + minutes * 60 + seconds,
            nanoseconds: milliseconds * 1_000_000 + microseconds * 1000 + nanoseconds,
        }
    }
}

impl fmt::Display for CypherDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P")?;
        if self.months != 0 {
            let years = self.years();
            let months = self.months_of_year();
            if years != 0 {
                write!(f, "{}Y", years)?;
            }
            if months != 0 {
                write!(f, "{}M", months)?;
            }
        }
        if self.days != 0 {
            write!(f, "{}D", self.days)?;
        }
        if self.seconds != 0 || self.nanoseconds != 0 {
            write!(f, "T")?;
            let hours = self.hours();
            let minutes = self.minutes_of_hour();
            let seconds = self.seconds_of_minute();
            if hours != 0 {
                write!(f, "{}H", hours)?;
            }
            if minutes != 0 {
                write!(f, "{}M", minutes)?;
            }
            if seconds != 0 || self.nanoseconds != 0 {
                if self.nanoseconds != 0 {
                    write!(f, "{}.{:09}S", seconds, self.nanoseconds)?;
                } else {
                    write!(f, "{}S", seconds)?;
                }
            }
        }
        if self.months == 0 && self.days == 0 && self.seconds == 0 && self.nanoseconds == 0 {
            write!(f, "T0S")?;
        }
        Ok(())
    }
}

impl Hash for CypherDuration {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.months.hash(state);
        self.days.hash(state);
        self.seconds.hash(state);
        self.nanoseconds.hash(state);
    }
}

impl PartialOrd for CypherDuration {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CypherDuration {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.months
            .cmp(&other.months)
            .then(self.days.cmp(&other.days))
            .then(self.seconds.cmp(&other.seconds))
            .then(self.nanoseconds.cmp(&other.nanoseconds))
    }
}

impl Eq for CypherDuration {}
