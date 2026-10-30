//! Parser utility functions
//!
//! 공백 처리, 식별자 파싱, 키워드 파싱 등 기본 유틸리티

use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag_no_case, take_until},
    character::complete::{alpha1, alphanumeric1, char, multispace0},
    combinator::{map, recognize},
    multi::many0,
    sequence::{delimited, pair},
};

/// 공백 무시하며 내부 파서 실행
pub fn ws<'a, F, O, E: nom::error::ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
where
    F: FnMut(&'a str) -> IResult<&'a str, O, E>,
{
    delimited(multispace0, inner, multispace0)
}

/// 식별자 파싱 (알파벳 또는 언더스코어로 시작, 영숫자/언더스코어 허용)
/// Neo4j Cypher와 호환: _id, _labels 등 내부 속성 접근 지원
pub fn identifier(input: &str) -> IResult<&str, String> {
    map(
        recognize(pair(
            alt((alpha1, nom::bytes::complete::tag("_"))),
            many0(alt((alphanumeric1, nom::bytes::complete::tag("_")))),
        )),
        |s: &str| s.to_string(),
    )(input)
}

/// 키워드 경계 체크 (키워드 뒤에 영숫자가 오면 매치 실패)
pub fn keyword<'a>(kw: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    move |input: &'a str| {
        let (remaining, matched) = tag_no_case(kw)(input)?;
        // 키워드 뒤에 영숫자나 _가 오면 매치 실패
        if let Some(c) = remaining.chars().next()
            && (c.is_alphanumeric() || c == '_')
        {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
        Ok((remaining, matched))
    }
}

/// 따옴표 또는 백틱으로 감싼 식별자 파싱
/// "quoted name" 또는 `backtick name` 또는 'single quoted' 형식 지원
pub fn quoted_identifier(input: &str) -> IResult<&str, String> {
    alt((
        // 더블 쿼트: "name with spaces"
        delimited(
            char('"'),
            map(take_until("\""), |s: &str| s.to_string()),
            char('"'),
        ),
        // 백틱: `name with spaces`
        delimited(
            char('`'),
            map(take_until("`"), |s: &str| s.to_string()),
            char('`'),
        ),
        // 싱글 쿼트: 'name with spaces'
        delimited(
            char('\''),
            map(take_until("'"), |s: &str| s.to_string()),
            char('\''),
        ),
    ))(input)
}

/// 식별자 또는 따옴표로 감싼 식별자
/// identifier | "quoted" | `backtick` | 'single'
pub fn identifier_or_quoted(input: &str) -> IResult<&str, String> {
    alt((quoted_identifier, identifier))(input)
}
