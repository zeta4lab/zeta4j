// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Constraint command parsing
//!
//! CREATE/DROP/SHOW CONSTRAINT 명령 파싱
//!
//! 지원하는 제약 조건:
//! - 기존: UNIQUE, NOT NULL, NODE KEY
//! - 프로퍼티 레벨: RANGE, LENGTH, PATTERN, ENUM, IMMUTABLE, DEFAULT
//! - 클래스 레벨: CHECK, AT LEAST ONE OF, MUTUALLY EXCLUSIVE
//! - 관계 레벨: CARDINALITY, ON DELETE CASCADE, REFERENTIAL INTEGRITY

use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_until, take_while1},
    character::complete::{char, digit1, multispace0, multispace1},
    combinator::{map, opt, recognize},
    multi::separated_list1,
    sequence::tuple,
};

use super::utils::identifier;
use crate::ast::{CascadeAction, ConstraintCommand, ReferentialAction};
use zeta4j_graph::core::Value;

/// 제약조건 프로퍼티 파싱 (단일: n.prop 또는 복합: (n.a, n.b))
fn parse_constraint_properties<'a>(input: &'a str, var: &str) -> IResult<&'a str, Vec<String>> {
    // 복합 프로퍼티 시도: (n.a, n.b)
    if let Ok((remaining, props)) = parse_composite_constraint_properties(input, var) {
        return Ok((remaining, props));
    }

    // 단일 프로퍼티: n.prop
    let (input, _) = tag(var)(input)?;
    let (input, _) = char('.')(input)?;
    let (input, prop) = identifier(input)?;

    Ok((input, vec![prop]))
}

/// 복합 프로퍼티 파싱: (n.a, n.b)
fn parse_composite_constraint_properties<'a>(
    input: &'a str,
    var: &str,
) -> IResult<&'a str, Vec<String>> {
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;

    let (input, props) = separated_list1(tuple((multispace0, char(','), multispace0)), |i| {
        let (i, _) = tag(var)(i)?;
        let (i, _) = char('.')(i)?;
        identifier(i)
    })(input)?;

    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((input, props))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop IS UNIQUE
fn parse_create_constraint_unique(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CONSTRAINT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, var) = identifier(input)?;
    let (input, _) = char(':')(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("REQUIRE")(input)?;
    let (input, _) = multispace1(input)?;

    let (input, properties) = parse_constraint_properties(input, &var)?;

    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("IS")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("UNIQUE")(input)?;

    Ok((
        input,
        ConstraintCommand::CreateUnique {
            name,
            label,
            properties,
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop IS NOT NULL
fn parse_create_constraint_not_null(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CONSTRAINT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, var) = identifier(input)?;
    let (input, _) = char(':')(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("REQUIRE")(input)?;
    let (input, _) = multispace1(input)?;

    let (input, properties) = parse_constraint_properties(input, &var)?;

    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("IS")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("NOT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("NULL")(input)?;

    Ok((
        input,
        ConstraintCommand::CreateNotNull {
            name,
            label,
            properties,
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE (n.a, n.b) IS NODE KEY
fn parse_create_constraint_node_key(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CONSTRAINT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, var) = identifier(input)?;
    let (input, _) = char(':')(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("REQUIRE")(input)?;
    let (input, _) = multispace1(input)?;

    let (input, properties) = parse_constraint_properties(input, &var)?;

    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("IS")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("NODE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("KEY")(input)?;

    Ok((
        input,
        ConstraintCommand::CreateNodeKey {
            name,
            label,
            properties,
        },
    ))
}

/// DROP CONSTRAINT name
fn parse_drop_constraint(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CONSTRAINT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;

    Ok((input, ConstraintCommand::Drop { name }))
}

/// SHOW CONSTRAINTS
fn parse_show_constraints(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CONSTRAINTS")(input)?;

    Ok((input, ConstraintCommand::Show))
}

/// SHOW CONSTRAINTS FOR :Label
fn parse_show_constraints_for(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CONSTRAINTS")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, label) = identifier(input)?;

    Ok((input, ConstraintCommand::ShowFor { label }))
}

// ============================================================
// 헬퍼 함수
// ============================================================

/// 숫자 파싱 (정수 또는 실수)
fn parse_number(input: &str) -> IResult<&str, f64> {
    let (input, num_str) = recognize(tuple((
        opt(char('-')),
        digit1,
        opt(tuple((char('.'), digit1))),
    )))(input)?;

    let num: f64 = num_str.parse().unwrap_or(0.0);
    Ok((input, num))
}

/// 단일 따옴표 문자열 파싱
fn parse_single_quoted_string(input: &str) -> IResult<&str, String> {
    let (input, _) = char('\'')(input)?;
    let (input, content) = take_while1(|c| c != '\'')(input)?;
    let (input, _) = char('\'')(input)?;
    Ok((input, content.to_string()))
}

/// 단일 프로퍼티 파싱 (n.prop)
fn parse_single_property<'a>(input: &'a str, var: &str) -> IResult<&'a str, String> {
    let (input, _) = tag(var)(input)?;
    let (input, _) = char('.')(input)?;
    let (input, prop) = identifier(input)?;
    Ok((input, prop))
}

/// 공통 제약조건 헤더 파싱: CREATE CONSTRAINT name FOR (n:Label) REQUIRE
fn parse_constraint_header(input: &str) -> IResult<&str, (String, String, String)> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CONSTRAINT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, var) = identifier(input)?;
    let (input, _) = char(':')(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("REQUIRE")(input)?;
    let (input, _) = multispace1(input)?;

    Ok((input, (name, label, var)))
}

/// 관계 제약조건 헤더 파싱: CREATE CONSTRAINT name FOR ()-[r:REL]->() REQUIRE
fn parse_rel_constraint_header(input: &str) -> IResult<&str, (String, String)> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CONSTRAINT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    // ()-[r:REL]->()
    let (input, _) = tag("()")(input)?;
    let (input, _) = char('-')(input)?;
    let (input, _) = char('[')(input)?;
    let (input, _var) = identifier(input)?;
    let (input, _) = char(':')(input)?;
    let (input, rel_type) = identifier(input)?;
    let (input, _) = char(']')(input)?;
    let (input, _) = tag("->")(input)?;
    let (input, _) = tag("()")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("REQUIRE")(input)?;
    let (input, _) = multispace1(input)?;

    Ok((input, (name, rel_type)))
}

// ============================================================
// 프로퍼티 레벨 제약 파서
// ============================================================

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop RANGE(min, max)
fn parse_create_constraint_range(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, var)) = parse_constraint_header(input)?;
    let (input, property) = parse_single_property(input, &var)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("RANGE")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, min) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(',')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, max) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((
        input,
        ConstraintCommand::CreateRange {
            name,
            label,
            property,
            min: Some(min),
            max: Some(max),
            inclusive: true,
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop LENGTH(min, max)
fn parse_create_constraint_length(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, var)) = parse_constraint_header(input)?;
    let (input, property) = parse_single_property(input, &var)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("LENGTH")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, min_str) = digit1(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(',')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, max_str) = digit1(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    let min_len: usize = min_str.parse().unwrap_or(0);
    let max_len: usize = max_str.parse().unwrap_or(usize::MAX);

    Ok((
        input,
        ConstraintCommand::CreateLength {
            name,
            label,
            property,
            min_len: Some(min_len),
            max_len: Some(max_len),
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop PATTERN('regex')
fn parse_create_constraint_pattern(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, var)) = parse_constraint_header(input)?;
    let (input, property) = parse_single_property(input, &var)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("PATTERN")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, regex) = parse_single_quoted_string(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((
        input,
        ConstraintCommand::CreatePattern {
            name,
            label,
            property,
            regex,
            message: None,
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop IN ('a', 'b', 'c')
fn parse_create_constraint_enum(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, var)) = parse_constraint_header(input)?;
    let (input, property) = parse_single_property(input, &var)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("IN")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;

    let (input, values) = separated_list1(
        tuple((multispace0, char(','), multispace0)),
        parse_single_quoted_string,
    )(input)?;

    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((
        input,
        ConstraintCommand::CreateEnum {
            name,
            label,
            property,
            values,
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop IS IMMUTABLE
fn parse_create_constraint_immutable(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, var)) = parse_constraint_header(input)?;
    let (input, property) = parse_single_property(input, &var)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("IS")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("IMMUTABLE")(input)?;

    Ok((
        input,
        ConstraintCommand::CreateImmutable {
            name,
            label,
            property,
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop DEFAULT value
fn parse_create_constraint_default(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, var)) = parse_constraint_header(input)?;
    let (input, property) = parse_single_property(input, &var)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DEFAULT")(input)?;
    let (input, _) = multispace1(input)?;

    // 문자열 또는 숫자 값 파싱
    let (input, value) = alt((
        map(parse_single_quoted_string, Value::String),
        map(parse_number, |n| {
            if n.fract() == 0.0 {
                Value::Integer(n as i64)
            } else {
                Value::Float(n)
            }
        }),
        map(tag_no_case("true"), |_| Value::Boolean(true)),
        map(tag_no_case("false"), |_| Value::Boolean(false)),
        map(tag_no_case("null"), |_| Value::Null),
    ))(input)?;

    Ok((
        input,
        ConstraintCommand::CreateDefault {
            name,
            label,
            property,
            value,
        },
    ))
}

// ============================================================
// 클래스 레벨 제약 파서
// ============================================================

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE CHECK(expression)
fn parse_create_constraint_check(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, _var)) = parse_constraint_header(input)?;
    let (input, _) = tag_no_case("CHECK")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;

    // 괄호 안의 표현식 추출 (간단한 구현: 닫는 괄호까지)
    let (input, expression) = take_until(")")(input)?;
    let (input, _) = char(')')(input)?;

    Ok((
        input,
        ConstraintCommand::CreateCheck {
            name,
            label,
            expression: expression.trim().to_string(),
            message: None,
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE AT LEAST ONE OF (n.a, n.b)
fn parse_create_constraint_require_at_least_one(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, var)) = parse_constraint_header(input)?;
    let (input, _) = tag_no_case("AT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("LEAST")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("ONE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("OF")(input)?;
    let (input, _) = multispace0(input)?;

    let (input, properties) = parse_constraint_properties(input, &var)?;

    Ok((
        input,
        ConstraintCommand::CreateRequireAtLeastOne {
            name,
            label,
            properties,
        },
    ))
}

/// CREATE CONSTRAINT name FOR (n:Label) REQUIRE MUTUALLY EXCLUSIVE (n.a, n.b)
fn parse_create_constraint_mutually_exclusive(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, label, var)) = parse_constraint_header(input)?;
    let (input, _) = tag_no_case("MUTUALLY")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("EXCLUSIVE")(input)?;
    let (input, _) = multispace0(input)?;

    let (input, properties) = parse_constraint_properties(input, &var)?;

    Ok((
        input,
        ConstraintCommand::CreateMutuallyExclusive {
            name,
            label,
            properties,
        },
    ))
}

// ============================================================
// 관계 레벨 제약 파서
// ============================================================

/// CREATE CONSTRAINT name FOR ()-[r:REL]->() REQUIRE CARDINALITY(min, max)
fn parse_create_constraint_cardinality(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, rel_type)) = parse_rel_constraint_header(input)?;
    let (input, _) = tag_no_case("CARDINALITY")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, min_str) = digit1(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(',')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, max_str) = digit1(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    let min: u32 = min_str.parse().unwrap_or(0);
    let max: u32 = max_str.parse().unwrap_or(u32::MAX);

    Ok((
        input,
        ConstraintCommand::CreateCardinality {
            name,
            rel_type,
            min,
            max: Some(max),
        },
    ))
}

/// CREATE CONSTRAINT name FOR ()-[r:REL]->() REQUIRE ON DELETE CASCADE/SET NULL/SET DEFAULT/RESTRICT
fn parse_create_constraint_cascade(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, rel_type)) = parse_rel_constraint_header(input)?;
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DELETE")(input)?;
    let (input, _) = multispace1(input)?;

    let (input, action) = alt((
        map(tag_no_case("CASCADE"), |_| CascadeAction::Delete),
        map(
            tuple((tag_no_case("SET"), multispace1, tag_no_case("NULL"))),
            |_| CascadeAction::SetNull,
        ),
        map(
            tuple((tag_no_case("SET"), multispace1, tag_no_case("DEFAULT"))),
            |_| CascadeAction::SetDefault,
        ),
        map(tag_no_case("RESTRICT"), |_| CascadeAction::Restrict),
    ))(input)?;

    Ok((
        input,
        ConstraintCommand::CreateCascade {
            name,
            rel_type,
            action,
        },
    ))
}

/// 참조 동작 파싱
fn parse_referential_action(input: &str) -> IResult<&str, ReferentialAction> {
    alt((
        map(tag_no_case("CASCADE"), |_| ReferentialAction::Cascade),
        map(
            tuple((tag_no_case("SET"), multispace1, tag_no_case("NULL"))),
            |_| ReferentialAction::SetNull,
        ),
        map(
            tuple((tag_no_case("SET"), multispace1, tag_no_case("DEFAULT"))),
            |_| ReferentialAction::SetDefault,
        ),
        map(tag_no_case("RESTRICT"), |_| ReferentialAction::Restrict),
        map(
            tuple((tag_no_case("NO"), multispace1, tag_no_case("ACTION"))),
            |_| ReferentialAction::NoAction,
        ),
    ))(input)
}

/// CREATE CONSTRAINT name FOR ()-[r:REL]->() REQUIRE ON DELETE action ON UPDATE action
fn parse_create_constraint_referential_integrity(input: &str) -> IResult<&str, ConstraintCommand> {
    let (input, (name, rel_type)) = parse_rel_constraint_header(input)?;

    // ON DELETE action
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DELETE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, on_delete) = parse_referential_action(input)?;
    let (input, _) = multispace1(input)?;

    // ON UPDATE action
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("UPDATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, on_update) = parse_referential_action(input)?;

    Ok((
        input,
        ConstraintCommand::CreateReferentialIntegrity {
            name,
            rel_type,
            on_delete,
            on_update,
        },
    ))
}

/// 제약조건 명령 파서
pub fn parse_constraint_command(input: &str) -> IResult<&str, ConstraintCommand> {
    alt((
        // 기존 제약
        parse_create_constraint_node_key,
        parse_create_constraint_not_null,
        parse_create_constraint_unique,
        // 프로퍼티 레벨 제약
        parse_create_constraint_range,
        parse_create_constraint_length,
        parse_create_constraint_pattern,
        parse_create_constraint_enum,
        parse_create_constraint_immutable,
        parse_create_constraint_default,
        // 클래스 레벨 제약
        parse_create_constraint_check,
        parse_create_constraint_require_at_least_one,
        parse_create_constraint_mutually_exclusive,
        // 관계 레벨 제약
        parse_create_constraint_cardinality,
        parse_create_constraint_referential_integrity,
        parse_create_constraint_cascade,
        // 기타
        parse_drop_constraint,
        parse_show_constraints_for,
        parse_show_constraints,
    ))(input)
}
