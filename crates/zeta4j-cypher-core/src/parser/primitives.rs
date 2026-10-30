//! Primitive value parsing functions
//!
//! 기본 값 파싱: 문자열, 숫자, 불리언, null
//! 순환 의존성을 피하기 위해 분리된 모듈

use nom::{
    IResult,
    branch::alt,
    bytes::complete::{escaped_transform, tag, tag_no_case},
    character::complete::{char, digit1, none_of},
    combinator::{map, opt, recognize, value},
    sequence::{delimited, tuple},
};

use zeta4j_graph::core::Value;

/// 문자열 파싱 ('...' 또는 "...")
/// 지원하는 이스케이프: \\, \', \", \n, \r, \t
pub fn parse_string(input: &str) -> IResult<&str, Value> {
    // 빈 문자열 처리 (escaped_transform은 빈 입력에서 에러 반환)
    let empty_single = map(tag("''"), |_| String::new());
    let empty_double = map(tag("\"\""), |_| String::new());

    let single_quote = delimited(
        char('\''),
        escaped_transform(
            none_of("\\'"),
            '\\',
            alt((
                value("\\", tag("\\")),
                value("'", tag("'")),
                value("\"", tag("\"")),
                value("\n", tag("n")),
                value("\r", tag("r")),
                value("\t", tag("t")),
            )),
        ),
        char('\''),
    );
    let double_quote = delimited(
        char('"'),
        escaped_transform(
            none_of("\\\""),
            '\\',
            alt((
                value("\\", tag("\\")),
                value("\"", tag("\"")),
                value("'", tag("'")),
                value("\n", tag("n")),
                value("\r", tag("r")),
                value("\t", tag("t")),
            )),
        ),
        char('"'),
    );

    map(
        alt((empty_single, empty_double, single_quote, double_quote)),
        |s: String| Value::String(s),
    )(input)
}

/// 불리언 파싱 (TRUE/FALSE)
pub fn parse_boolean(input: &str) -> IResult<&str, Value> {
    alt((
        value(Value::Boolean(true), tag_no_case("TRUE")),
        value(Value::Boolean(false), tag_no_case("FALSE")),
    ))(input)
}

/// NULL 파싱
pub fn parse_null(input: &str) -> IResult<&str, Value> {
    value(Value::Null, tag_no_case("NULL"))(input)
}

/// 숫자 파싱 (정수 또는 실수)
pub fn parse_number(input: &str) -> IResult<&str, Value> {
    let (input, matched) = recognize(tuple((
        opt(char('-')),
        digit1,
        opt(tuple((char('.'), digit1))),
    )))(input)?;

    if matched.contains('.') {
        match matched.parse::<f64>() {
            Ok(f) => Ok((input, Value::Float(f))),
            Err(_) => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Float,
            ))),
        }
    } else {
        match matched.parse::<i64>() {
            Ok(n) => Ok((input, Value::Integer(n))),
            Err(_) => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Digit,
            ))),
        }
    }
}

/// 기본 값 파싱 (null, 숫자, 문자열, 불리언)
pub fn parse_value(input: &str) -> IResult<&str, Value> {
    alt((parse_null, parse_number, parse_string, parse_boolean))(input)
}
