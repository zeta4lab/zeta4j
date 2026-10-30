//! Schema DDL parsing
//!
//! CREATE/DROP NODE LABEL, RELATIONSHIP TYPE 명령 파싱

use nom::{
    IResult,
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{char, multispace0, multispace1},
    combinator::{map, opt},
    multi::separated_list0,
    sequence::{delimited, preceded, tuple},
};

use super::utils::identifier;
use super::values::parse_value;
use crate::ast::{SchemaCommand, SchemaPropertyDef};

/// 스키마 타입 파싱 (STRING, INTEGER, FLOAT, LIST<STRING> 등)
fn parse_schema_type(input: &str) -> IResult<&str, String> {
    alt((
        // LIST<TYPE> 형식
        map(
            tuple((
                tag_no_case("LIST"),
                multispace0,
                char('<'),
                multispace0,
                parse_schema_type,
                multispace0,
                char('>'),
            )),
            |(_, _, _, _, inner, _, _)| format!("LIST<{}>", inner),
        ),
        // 기본 타입들
        map(tag_no_case("STRING"), |_| "STRING".to_string()),
        map(tag_no_case("INTEGER"), |_| "INTEGER".to_string()),
        map(tag_no_case("INT"), |_| "INTEGER".to_string()),
        map(tag_no_case("FLOAT"), |_| "FLOAT".to_string()),
        map(tag_no_case("DOUBLE"), |_| "FLOAT".to_string()),
        map(tag_no_case("BOOLEAN"), |_| "BOOLEAN".to_string()),
        map(tag_no_case("BOOL"), |_| "BOOLEAN".to_string()),
        map(tag_no_case("DATE"), |_| "DATE".to_string()),
        map(tag_no_case("DATETIME"), |_| "DATETIME".to_string()),
        map(tag_no_case("LOCALDATETIME"), |_| {
            "LOCALDATETIME".to_string()
        }),
        map(tag_no_case("TIME"), |_| "TIME".to_string()),
        map(tag_no_case("DURATION"), |_| "DURATION".to_string()),
        map(tag_no_case("MAP"), |_| "MAP".to_string()),
        map(tag_no_case("ANY"), |_| "ANY".to_string()),
    ))(input)
}

/// 스키마 프로퍼티 정의 파싱
fn parse_schema_property_def(input: &str) -> IResult<&str, SchemaPropertyDef> {
    let (input, _) = multispace0(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, property_type) = parse_schema_type(input)?;
    let (input, _) = multispace0(input)?;

    // NOT NULL 체크
    let (input, not_null) = opt(tuple((
        tag_no_case("NOT"),
        multispace1,
        tag_no_case("NULL"),
    )))(input)?;
    let nullable = not_null.is_none();
    let (input, _) = multispace0(input)?;

    // DEFAULT value 체크
    let (input, default_value) = opt(preceded(
        tuple((tag_no_case("DEFAULT"), multispace1)),
        parse_value,
    ))(input)?;

    Ok((
        input,
        SchemaPropertyDef {
            name,
            property_type,
            nullable,
            default_value,
        },
    ))
}

/// CREATE NODE LABEL :Label (prop TYPE, ...)
fn parse_create_node_label(input: &str) -> IResult<&str, SchemaCommand> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("NODE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("LABEL")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = opt(char(':'))(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;

    let (input, properties) = opt(delimited(
        char('('),
        separated_list0(
            delimited(multispace0, char(','), multispace0),
            parse_schema_property_def,
        ),
        preceded(multispace0, char(')')),
    ))(input)?;

    Ok((
        input,
        SchemaCommand::CreateNodeLabel {
            label,
            properties: properties.unwrap_or_default(),
        },
    ))
}

/// CREATE RELATIONSHIP TYPE :TYPE (prop TYPE, ...) [FROM :Label [TO :Label]]
fn parse_create_relationship_type(input: &str) -> IResult<&str, SchemaCommand> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("RELATIONSHIP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("TYPE")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = opt(char(':'))(input)?;
    let (input, rel_type) = identifier(input)?;
    let (input, _) = multispace0(input)?;

    let (input, properties) = opt(delimited(
        char('('),
        separated_list0(
            delimited(multispace0, char(','), multispace0),
            parse_schema_property_def,
        ),
        preceded(multispace0, char(')')),
    ))(input)?;
    let (input, _) = multispace0(input)?;

    let (input, from_labels) = opt(preceded(
        tuple((tag_no_case("FROM"), multispace0, opt(char(':')))),
        identifier,
    ))(input)?;
    let (input, _) = multispace0(input)?;

    let (input, to_labels) = opt(preceded(
        tuple((tag_no_case("TO"), multispace0, opt(char(':')))),
        identifier,
    ))(input)?;

    Ok((
        input,
        SchemaCommand::CreateRelationshipType {
            rel_type,
            properties: properties.unwrap_or_default(),
            from_labels: from_labels.map(|l| vec![l]),
            to_labels: to_labels.map(|l| vec![l]),
        },
    ))
}

/// DROP NODE LABEL :Label
fn parse_drop_node_label(input: &str) -> IResult<&str, SchemaCommand> {
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("NODE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("LABEL")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = opt(char(':'))(input)?;
    let (input, label) = identifier(input)?;

    Ok((input, SchemaCommand::DropNodeLabel { label }))
}

/// DROP RELATIONSHIP TYPE :TYPE
fn parse_drop_relationship_type(input: &str) -> IResult<&str, SchemaCommand> {
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("RELATIONSHIP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("TYPE")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = opt(char(':'))(input)?;
    let (input, rel_type) = identifier(input)?;

    Ok((input, SchemaCommand::DropRelationshipType { rel_type }))
}

/// SHOW NODE LABELS
fn parse_show_node_labels(input: &str) -> IResult<&str, SchemaCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("NODE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("LABELS")(input)?;
    Ok((input, SchemaCommand::ShowNodeLabels))
}

/// SHOW RELATIONSHIP TYPES
fn parse_show_relationship_types(input: &str) -> IResult<&str, SchemaCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("RELATIONSHIP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("TYPES")(input)?;
    Ok((input, SchemaCommand::ShowRelationshipTypes))
}

/// SHOW SCHEMA FOR :Label
fn parse_show_schema_for(input: &str) -> IResult<&str, SchemaCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("SCHEMA")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, name) = identifier(input)?;

    Ok((
        input,
        SchemaCommand::ShowSchemaFor {
            is_node: true,
            name,
        },
    ))
}

/// 스키마 명령 파서
pub fn parse_schema_command(input: &str) -> IResult<&str, SchemaCommand> {
    alt((
        parse_create_node_label,
        parse_create_relationship_type,
        parse_drop_node_label,
        parse_drop_relationship_type,
        parse_show_node_labels,
        parse_show_relationship_types,
        parse_show_schema_for,
    ))(input)
}
