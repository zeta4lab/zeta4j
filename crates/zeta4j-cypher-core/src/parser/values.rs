//! Value parsing functions
//!
//! 프로퍼티 패턴 파싱 (표현식 지원)
//!
//! 이 모듈은 `primitives`에서 기본 값 파서를 가져오고,
//! `expressions`에서 전체 표현식 파서를 가져와 프로퍼티 패턴에서 사용합니다.

use nom::{
    IResult,
    character::complete::char,
    combinator::map,
    multi::separated_list0,
    sequence::{delimited, separated_pair},
};

use super::utils::{identifier, ws};
use crate::ast::PropertyPattern;

// 기본 값 파서는 primitives에서 가져옴 (다른 모듈에서 사용)
pub use super::primitives::parse_value;

/// 프로퍼티 패턴 파싱 { key: expression, ... }
///
/// 전체 표현식을 지원합니다:
/// - 리터럴: `{name: 'Alice', age: 30}`
/// - 파라미터: `{name: $name}`
/// - 함수 호출: `{ts: timestamp(), id: randomUUID()}`
/// - 산술 연산: `{score: 100 * 1.5}`
/// - 중첩 함수: `{hash: substring(randomUUID(), 0, 8)}`
pub fn parse_property_pattern(input: &str) -> IResult<&str, PropertyPattern> {
    use super::expressions::parse_or_expression;

    map(
        delimited(
            char('{'),
            ws(separated_list0(
                ws(char(',')),
                separated_pair(identifier, ws(char(':')), ws(parse_or_expression)),
            )),
            char('}'),
        ),
        |pairs| pairs.into_iter().collect(),
    )(input)
}
