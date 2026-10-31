// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Pattern parsing
//!
//! 노드, 엣지, 경로 패턴 파싱

use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{char, digit1, multispace0},
    combinator::{map, map_res, opt, value},
    multi::{many0, many1},
    sequence::{delimited, preceded, tuple},
};

use super::utils::{identifier, ws};
use super::values::parse_property_pattern;
use crate::ast::*;

/// u32 숫자 파싱
fn parse_u32_number(input: &str) -> IResult<&str, u32> {
    map_res(digit1, |s: &str| s.parse::<u32>())(input)
}

/// 가변 길이 범위 파싱: *n, *min..max, *..max, *min..
fn parse_variable_length_range(input: &str) -> IResult<&str, VariableLengthRange> {
    let (input, _) = char('*')(input)?;
    let (input, _) = multispace0(input)?;

    let has_dots = input.contains("..");

    if has_dots {
        let (input, min_opt) = opt(parse_u32_number)(input)?;
        let (input, _) = tag("..")(input)?;
        let (input, _) = multispace0(input)?;
        let (input, max_opt) = opt(parse_u32_number)(input)?;

        let min = min_opt.unwrap_or(1);
        let max = max_opt;

        Ok((input, VariableLengthRange::new(min, max)))
    } else {
        let (input, n_opt) = opt(parse_u32_number)(input)?;

        match n_opt {
            Some(n) => Ok((input, VariableLengthRange::exact(n))),
            None => Ok((input, VariableLengthRange::unbounded())),
        }
    }
}

/// 다중 레이블 파싱: :Label1:Label2:Label3
fn parse_labels(input: &str) -> IResult<&str, Vec<String>> {
    many1(preceded(char(':'), identifier))(input)
}

/// 노드 패턴 파싱 (var:Label1:Label2 {props})
pub fn parse_node_pattern(input: &str) -> IResult<&str, NodePattern> {
    map(
        delimited(
            char('('),
            ws(tuple((
                opt(identifier),
                opt(parse_labels),
                opt(ws(parse_property_pattern)),
            ))),
            char(')'),
        ),
        |(var, labels, props)| NodePattern {
            variable: var,
            labels: labels.unwrap_or_default(),
            properties: props,
        },
    )(input)
}

/// 엣지 패턴 파싱: -[r:TYPE*1..3]-> 또는 <-[r:TYPE]- 또는 -[r:TYPE]-
pub fn parse_edge_pattern(input: &str) -> IResult<&str, EdgePattern> {
    // 왼쪽 방향 마커: <- 또는 -
    let (input, left_arrow) = alt((value(true, tag("<-")), value(false, tag("-"))))(input)?;

    // 대괄호 내용: [var:TYPE*min..max {props}]
    let (input, (var, rel_type, var_length, props)) = delimited(
        char('['),
        ws(tuple((
            opt(identifier),
            opt(preceded(char(':'), identifier)),
            opt(ws(parse_variable_length_range)),
            opt(ws(parse_property_pattern)),
        ))),
        char(']'),
    )(input)?;

    // 오른쪽 방향 마커: -> 또는 -
    let (input, right_arrow) = alt((value(true, tag("->")), value(false, tag("-"))))(input)?;

    // 방향 결정
    let direction = match (left_arrow, right_arrow) {
        (false, true) => EdgeDirection::Outgoing,
        (true, false) => EdgeDirection::Incoming,
        _ => EdgeDirection::Both,
    };

    Ok((
        input,
        EdgePattern {
            variable: var,
            rel_types: rel_type.map(|t| vec![t]).unwrap_or_default(),
            properties: props,
            direction,
            variable_length: var_length,
        },
    ))
}

/// 경로 패턴 파싱: (a)-[r]->(b) 등
pub fn parse_path_pattern(input: &str) -> IResult<&str, Pattern> {
    let (input, first_node) = parse_node_pattern(input)?;

    let (input, edge_node_pairs) = many0(tuple((parse_edge_pattern, parse_node_pattern)))(input)?;

    let mut elements = vec![PatternElement::Node(first_node.clone())];
    let mut parts = vec![first_node];

    for (edge, node) in edge_node_pairs {
        elements.push(PatternElement::Edge(edge));
        elements.push(PatternElement::Node(node.clone()));
        parts.push(node);
    }

    Ok((
        input,
        Pattern {
            elements,
            parts,
            variable: None,
            path_function: None,
        },
    ))
}

/// 경로 함수 파싱: shortestPath, allShortestPaths
fn parse_path_function(input: &str) -> IResult<&str, PathFunction> {
    alt((
        value(
            PathFunction::AllShortestPaths,
            tag_no_case("allShortestPaths"),
        ),
        value(PathFunction::ShortestPath, tag_no_case("shortestPath")),
    ))(input)
}

/// 명명된 경로 패턴 파싱: path = shortestPath((a)-[*]-(b)) 또는 path = (a)-[*]-(b)
fn parse_named_pattern(input: &str) -> IResult<&str, Pattern> {
    let (input, var_name) = ws(identifier)(input)?;
    let (input, _) = ws(char('='))(input)?;

    // shortestPath/allShortestPaths 함수인지 확인
    let (input, path_func) = opt(ws(parse_path_function))(input)?;

    let (input, mut pattern) = if path_func.is_some() {
        // shortestPath((a)-[*]-(b)) 형태
        delimited(ws(char('(')), ws(parse_path_pattern), ws(char(')')))(input)?
    } else {
        // path = (a)-[*]-(b) 형태 (경로 함수 없음)
        ws(parse_path_pattern)(input)?
    };

    pattern.variable = Some(var_name);
    pattern.path_function = path_func;

    Ok((input, pattern))
}

/// 패턴 파서 (최상위)
/// - path = shortestPath((a)-[*]-(b))
/// - path = (a)-[*]-(b)
/// - (a)-[*]-(b)
pub fn parse_pattern(input: &str) -> IResult<&str, Pattern> {
    alt((parse_named_pattern, parse_path_pattern))(input)
}
