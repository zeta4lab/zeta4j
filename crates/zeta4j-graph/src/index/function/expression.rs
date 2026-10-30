//! 함수 표현식 정의
//!
//! 인덱싱 가능한 함수 표현식을 정의합니다.

use std::fmt;

/// 인덱싱 가능한 함수 표현식
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionExpression {
    // ===== 문자열 함수 =====
    /// 소문자 변환: lower(property)
    Lower(String),
    /// 대문자 변환: upper(property)
    Upper(String),
    /// 공백 제거: trim(property)
    Trim(String),
    /// 왼쪽 공백 제거: ltrim(property)
    LTrim(String),
    /// 오른쪽 공백 제거: rtrim(property)
    RTrim(String),
    /// 문자열 길이: length(property)
    Length(String),

    // ===== 숫자 함수 =====
    /// 절대값: abs(property)
    Abs(String),
    /// 내림: floor(property)
    Floor(String),
    /// 올림: ceil(property)
    Ceil(String),
    /// 반올림: round(property)
    Round(String),
    /// 부호: sign(property)
    Sign(String),

    // ===== 날짜/시간 함수 =====
    /// 연도: year(property)
    Year(String),
    /// 월: month(property)
    Month(String),
    /// 일: day(property)
    Day(String),
    /// 시: hour(property)
    Hour(String),
    /// 분: minute(property)
    Minute(String),
    /// 초: second(property)
    Second(String),
    /// 요일: dayOfWeek(property)
    DayOfWeek(String),
    /// 연간 일수: dayOfYear(property)
    DayOfYear(String),
    /// 분기: quarter(property)
    Quarter(String),
    /// 주차: week(property)
    Week(String),

    // ===== 컬렉션 함수 =====
    /// 배열 크기: size(property)
    Size(String),
}

impl FunctionExpression {
    /// 표현식이 참조하는 프로퍼티 이름 반환
    pub fn source_property(&self) -> &str {
        match self {
            // 문자열 함수
            FunctionExpression::Lower(p) => p,
            FunctionExpression::Upper(p) => p,
            FunctionExpression::Trim(p) => p,
            FunctionExpression::LTrim(p) => p,
            FunctionExpression::RTrim(p) => p,
            FunctionExpression::Length(p) => p,
            // 숫자 함수
            FunctionExpression::Abs(p) => p,
            FunctionExpression::Floor(p) => p,
            FunctionExpression::Ceil(p) => p,
            FunctionExpression::Round(p) => p,
            FunctionExpression::Sign(p) => p,
            // 날짜 함수
            FunctionExpression::Year(p) => p,
            FunctionExpression::Month(p) => p,
            FunctionExpression::Day(p) => p,
            FunctionExpression::Hour(p) => p,
            FunctionExpression::Minute(p) => p,
            FunctionExpression::Second(p) => p,
            FunctionExpression::DayOfWeek(p) => p,
            FunctionExpression::DayOfYear(p) => p,
            FunctionExpression::Quarter(p) => p,
            FunctionExpression::Week(p) => p,
            // 컬렉션 함수
            FunctionExpression::Size(p) => p,
        }
    }

    /// 함수 이름 반환
    pub fn function_name(&self) -> &str {
        match self {
            FunctionExpression::Lower(_) => "lower",
            FunctionExpression::Upper(_) => "upper",
            FunctionExpression::Trim(_) => "trim",
            FunctionExpression::LTrim(_) => "ltrim",
            FunctionExpression::RTrim(_) => "rtrim",
            FunctionExpression::Length(_) => "length",
            FunctionExpression::Abs(_) => "abs",
            FunctionExpression::Floor(_) => "floor",
            FunctionExpression::Ceil(_) => "ceil",
            FunctionExpression::Round(_) => "round",
            FunctionExpression::Sign(_) => "sign",
            FunctionExpression::Year(_) => "year",
            FunctionExpression::Month(_) => "month",
            FunctionExpression::Day(_) => "day",
            FunctionExpression::Hour(_) => "hour",
            FunctionExpression::Minute(_) => "minute",
            FunctionExpression::Second(_) => "second",
            FunctionExpression::DayOfWeek(_) => "dayOfWeek",
            FunctionExpression::DayOfYear(_) => "dayOfYear",
            FunctionExpression::Quarter(_) => "quarter",
            FunctionExpression::Week(_) => "week",
            FunctionExpression::Size(_) => "size",
        }
    }

    /// 문자열에서 함수 표현식 파싱
    ///
    /// # Arguments
    /// * `func_name` - 함수 이름 (예: "lower", "year")
    /// * `property` - 프로퍼티 이름 (예: "name", "date")
    ///
    /// # Returns
    /// 파싱된 FunctionExpression 또는 None (지원하지 않는 함수)
    pub fn parse(func_name: &str, property: &str) -> Option<Self> {
        let prop = property.to_string();
        match func_name.to_lowercase().as_str() {
            // 문자열 함수
            "lower" | "tolower" => Some(FunctionExpression::Lower(prop)),
            "upper" | "toupper" => Some(FunctionExpression::Upper(prop)),
            "trim" => Some(FunctionExpression::Trim(prop)),
            "ltrim" => Some(FunctionExpression::LTrim(prop)),
            "rtrim" => Some(FunctionExpression::RTrim(prop)),
            "length" => Some(FunctionExpression::Length(prop)),
            // 숫자 함수
            "abs" => Some(FunctionExpression::Abs(prop)),
            "floor" => Some(FunctionExpression::Floor(prop)),
            "ceil" | "ceiling" => Some(FunctionExpression::Ceil(prop)),
            "round" => Some(FunctionExpression::Round(prop)),
            "sign" => Some(FunctionExpression::Sign(prop)),
            // 날짜 함수
            "year" => Some(FunctionExpression::Year(prop)),
            "month" => Some(FunctionExpression::Month(prop)),
            "day" => Some(FunctionExpression::Day(prop)),
            "hour" => Some(FunctionExpression::Hour(prop)),
            "minute" => Some(FunctionExpression::Minute(prop)),
            "second" => Some(FunctionExpression::Second(prop)),
            "dayofweek" => Some(FunctionExpression::DayOfWeek(prop)),
            "dayofyear" => Some(FunctionExpression::DayOfYear(prop)),
            "quarter" => Some(FunctionExpression::Quarter(prop)),
            "week" => Some(FunctionExpression::Week(prop)),
            // 컬렉션 함수
            "size" => Some(FunctionExpression::Size(prop)),
            _ => None,
        }
    }
}

impl fmt::Display for FunctionExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.function_name(), self.source_property())
    }
}
