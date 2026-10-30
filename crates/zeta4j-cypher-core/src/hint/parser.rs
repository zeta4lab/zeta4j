//! 힌트 파서 모듈

use super::types::QueryHint;
use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag_no_case, take_while1},
    character::complete::{char, multispace0, multispace1},
    combinator::opt,
    multi::many0,
    sequence::{delimited, preceded},
};
use zeta4j_graph::error::{GraphError, Result};

/// 힌트 파서
pub struct HintParser;

impl HintParser {
    /// 단일 USING 절 파싱
    ///
    /// 지원하는 형식:
    /// - `USING INDEX var:Label(property)`
    /// - `USING SCAN var:Label`
    /// - `USING JOIN ON var`
    pub fn parse(input: &str) -> Result<QueryHint> {
        let trimmed = input.trim();

        match parse_using_hint(trimmed) {
            Ok((_, hint)) => Ok(hint),
            Err(e) => Err(GraphError::Parse {
                message: format!("Hint parsing failed: {} - {:?}", input, e),
            }),
        }
    }

    /// 여러 힌트 파싱 (공백 또는 쉼표로 구분)
    pub fn parse_all(input: &str) -> Result<Vec<QueryHint>> {
        let trimmed = input.trim();

        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        match parse_multiple_hints(trimmed) {
            Ok((_, hints)) => Ok(hints),
            Err(e) => Err(GraphError::Parse {
                message: format!("Hint parsing failed: {} - {:?}", input, e),
            }),
        }
    }

    /// USING 절이 포함된 쿼리에서 힌트 추출
    ///
    /// 쿼리 문자열에서 모든 USING 절을 찾아 파싱합니다.
    pub fn extract_from_query(query: &str) -> Result<Vec<QueryHint>> {
        let mut hints = Vec::new();
        let upper = query.to_uppercase();

        // "USING "으로 시작하는 모든 위치 찾기
        let mut search_start = 0;
        while let Some(pos) = upper[search_start..].find("USING ") {
            let actual_pos = search_start + pos;

            // USING 절의 끝 찾기 (다음 키워드 또는 줄바꿈까지)
            let hint_start = actual_pos;
            let remaining = &query[hint_start..];

            // 다음 주요 키워드 찾기
            let end_pos = find_hint_end(remaining);
            let hint_text = &remaining[..end_pos].trim();

            if let Ok(hint) = Self::parse(hint_text) {
                hints.push(hint);
            }

            search_start = actual_pos + end_pos;
        }

        Ok(hints)
    }
}

/// 힌트 끝 위치 찾기 (다음 USING 또는 주요 키워드까지)
fn find_hint_end(input: &str) -> usize {
    let upper = input.to_uppercase();
    let keywords = [
        "USING ", "WHERE", "RETURN", "WITH", "ORDER", "MATCH", "CREATE", "DELETE", "SET", "MERGE",
    ];

    let mut min_pos = input.len();

    for keyword in &keywords {
        if let Some(pos) = upper[6..].find(keyword) {
            let actual_pos = pos + 6;
            if actual_pos < min_pos {
                min_pos = actual_pos;
            }
        }
    }

    min_pos
}

// === nom 파서 ===

/// 식별자 파싱 (변수명, 레이블, 속성명)
fn identifier(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| c.is_alphanumeric() || c == '_')(input)
}

/// USING INDEX 파싱: USING INDEX var:Label(property)
fn parse_using_index(input: &str) -> IResult<&str, QueryHint> {
    let (input, _) = tag_no_case("USING")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, variable) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, property) = delimited(
        char('('),
        preceded(multispace0, identifier),
        preceded(multispace0, char(')')),
    )(input)?;

    Ok((input, QueryHint::use_index(variable, label, property)))
}

/// USING SCAN 파싱: USING SCAN var:Label
fn parse_using_scan(input: &str) -> IResult<&str, QueryHint> {
    let (input, _) = tag_no_case("USING")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("SCAN")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, variable) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, label) = identifier(input)?;

    Ok((input, QueryHint::use_scan(variable, label)))
}

/// USING JOIN 파싱: USING JOIN ON var
fn parse_using_join(input: &str) -> IResult<&str, QueryHint> {
    let (input, _) = tag_no_case("USING")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("JOIN")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, variable) = identifier(input)?;

    Ok((input, QueryHint::use_join(variable)))
}

/// 단일 USING 힌트 파싱
fn parse_using_hint(input: &str) -> IResult<&str, QueryHint> {
    let (input, _) = multispace0(input)?;
    let (input, hint) = alt((parse_using_index, parse_using_scan, parse_using_join))(input)?;
    let (input, _) = multispace0(input)?;

    Ok((input, hint))
}

/// 여러 USING 힌트 파싱
fn parse_multiple_hints(input: &str) -> IResult<&str, Vec<QueryHint>> {
    let (input, _) = multispace0(input)?;
    let (input, hints) = many0(parse_single_hint)(input)?;
    let (input, _) = multispace0(input)?;

    Ok((input, hints))
}

/// 단일 힌트 파싱 (쉼표 구분자 옵션 포함)
fn parse_single_hint(input: &str) -> IResult<&str, QueryHint> {
    let (input, _) = multispace0(input)?;
    let (input, _) = opt(char(','))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, hint) = alt((parse_using_index, parse_using_scan, parse_using_join))(input)?;
    let (input, _) = multispace0(input)?;

    Ok((input, hint))
}
