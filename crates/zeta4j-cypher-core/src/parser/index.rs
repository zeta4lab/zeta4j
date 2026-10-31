// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Index command parsing
//!
//! CREATE/DROP/SHOW INDEX 명령 파싱

use nom::{
    IResult,
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{char, digit1, multispace0, multispace1},
    combinator::{map, map_res},
    multi::separated_list1,
    sequence::{delimited, tuple},
};

use super::utils::identifier;
use crate::ast::IndexCommand;

/// FOR (variable:Label) ON (variable.prop, ...) 구문 파싱
fn parse_for_label_properties(input: &str) -> IResult<&str, (String, Vec<String>)> {
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _variable) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, properties) = separated_list1(
        delimited(multispace0, char(','), multispace0),
        map(
            tuple((identifier, char('.'), identifier)),
            |(_, _, prop)| prop,
        ),
    )(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((input, (label, properties)))
}

/// ON :Label(prop, ...) 구문 파싱 (이전 구문)
fn parse_on_label_properties(input: &str) -> IResult<&str, (String, Vec<String>)> {
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, properties) =
        separated_list1(delimited(multispace0, char(','), multispace0), identifier)(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((input, (label, properties)))
}

/// CREATE INDEX [name] FOR (n:Label) ON (n.prop1, n.prop2, ...)
fn parse_create_index(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace0(input)?;

    // FOR 키워드 먼저 확인 (이름 없는 경우)
    if let Ok((rest, _)) = tag_no_case::<&str, &str, nom::error::Error<&str>>("FOR")(input) {
        let (input, (label, properties)) = parse_for_label_properties(rest)?;
        return Ok((
            input,
            IndexCommand::Create {
                name: None,
                label,
                properties,
            },
        ));
    }

    // ON 키워드 확인 (이전 구문, 이름 없음)
    if let Ok((rest, _)) = tag_no_case::<&str, &str, nom::error::Error<&str>>("ON")(input) {
        let (input, (label, properties)) = parse_on_label_properties(rest)?;
        return Ok((
            input,
            IndexCommand::Create {
                name: None,
                label,
                properties,
            },
        ));
    }

    // 인덱스 이름이 있는 경우
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;

    // FOR 또는 ON 확인
    match tag_no_case::<&str, &str, nom::error::Error<&str>>("FOR")(input) {
        Ok((rest, _)) => {
            let (input, (label, properties)) = parse_for_label_properties(rest)?;
            Ok((
                input,
                IndexCommand::Create {
                    name: Some(name),
                    label,
                    properties,
                },
            ))
        }
        _ => {
            let (input, _) = tag_no_case("ON")(input)?;
            let (input, (label, properties)) = parse_on_label_properties(input)?;
            Ok((
                input,
                IndexCommand::Create {
                    name: Some(name),
                    label,
                    properties,
                },
            ))
        }
    }
}

/// DROP INDEX ON :Label(property) 또는 DROP INDEX name
fn parse_drop_index(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace1(input)?;

    let on_result = tuple((
        tag_no_case::<&str, &str, nom::error::Error<&str>>("ON"),
        multispace0,
        char(':'),
    ))(input);

    if let Ok((input, _)) = on_result {
        let (input, label) = identifier(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char('(')(input)?;
        let (input, _) = multispace0(input)?;
        let (input, properties) =
            separated_list1(delimited(multispace0, char(','), multispace0), identifier)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char(')')(input)?;

        Ok((
            input,
            IndexCommand::Drop {
                name: None,
                label: Some(label),
                properties,
            },
        ))
    } else {
        let (input, name) = identifier(input)?;
        Ok((
            input,
            IndexCommand::Drop {
                name: Some(name),
                label: None,
                properties: vec![],
            },
        ))
    }
}

/// SHOW INDEXES
fn parse_show_indexes(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEXES")(input)?;
    Ok((input, IndexCommand::Show))
}

/// CREATE SPATIAL INDEX ON :Label(property)
fn parse_create_spatial_index(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("SPATIAL")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, property) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((input, IndexCommand::CreateSpatial { label, property }))
}

/// DROP SPATIAL INDEX ON :Label(property)
fn parse_drop_spatial_index(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("SPATIAL")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, property) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((input, IndexCommand::DropSpatial { label, property }))
}

/// SHOW SPATIAL INDEXES
fn parse_show_spatial_indexes(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("SPATIAL")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEXES")(input)?;
    Ok((input, IndexCommand::ShowSpatial))
}

fn parse_create_fulltext_index(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FULLTEXT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, variable) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("EACH")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('[')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, properties) = separated_list1(
        delimited(multispace0, char(','), multispace0),
        map(
            tuple((identifier, char('.'), identifier)),
            |(property_variable, _, property)| (property_variable, property),
        ),
    )(input)?;
    if properties
        .iter()
        .any(|(property_variable, _)| property_variable != &variable)
    {
        return Err(nom::Err::Failure(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Verify,
        )));
    }
    let properties = properties
        .into_iter()
        .map(|(_, property)| property)
        .collect();
    let (input, _) = multispace0(input)?;
    let (input, _) = char(']')(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("ANALYZER")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, analyzer) = identifier(input)?;
    Ok((
        input,
        IndexCommand::CreateFulltext {
            name,
            label,
            properties,
            analyzer,
        },
    ))
}

fn parse_drop_fulltext_index(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FULLTEXT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    Ok((input, IndexCommand::DropFulltext { name }))
}

fn parse_show_fulltext_indexes(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FULLTEXT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEXES")(input)?;
    Ok((input, IndexCommand::ShowFulltext))
}

fn parse_create_vector_index(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("VECTOR")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("FOR")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, variable) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(':')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, label) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("ON")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, property_variable) = identifier(input)?;
    if property_variable != variable {
        return Err(nom::Err::Failure(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Verify,
        )));
    }
    let (input, _) = char('.')(input)?;
    let (input, property) = identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DIMENSIONS")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, dimensions) = map_res(digit1, str::parse::<u32>)(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DISTANCE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, distance) = identifier(input)?;
    Ok((
        input,
        IndexCommand::CreateVector {
            name,
            label,
            property,
            dimensions,
            distance,
        },
    ))
}

fn parse_drop_vector_index(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("VECTOR")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEX")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier(input)?;
    Ok((input, IndexCommand::DropVector { name }))
}

fn parse_show_vector_indexes(input: &str) -> IResult<&str, IndexCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("VECTOR")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("INDEXES")(input)?;
    Ok((input, IndexCommand::ShowVector))
}

/// 인덱스 명령 파서 (CREATE INDEX, DROP INDEX, SHOW INDEXES, SPATIAL INDEX)
pub fn parse_index_command(input: &str) -> IResult<&str, IndexCommand> {
    alt((
        parse_create_vector_index,
        parse_drop_vector_index,
        parse_show_vector_indexes,
        parse_create_fulltext_index,
        parse_drop_fulltext_index,
        parse_show_fulltext_indexes,
        parse_create_spatial_index,
        parse_drop_spatial_index,
        parse_show_spatial_indexes,
        parse_create_index,
        parse_drop_index,
        parse_show_indexes,
    ))(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fulltext_index_lifecycle_commands() {
        let create = parse_index_command(
            "CREATE FULLTEXT INDEX person_text FOR (n:Person) ON EACH [n.name, n.bio] ANALYZER english",
        );
        assert!(matches!(
            create,
            Ok((
                "",
                IndexCommand::CreateFulltext {
                    name,
                    label,
                    properties,
                    analyzer,
                }
            )) if name == "person_text"
                && label == "Person"
                && properties == ["name", "bio"]
                && analyzer == "english"
        ));
        assert!(matches!(
            parse_index_command("DROP FULLTEXT INDEX person_text"),
            Ok(("", IndexCommand::DropFulltext { name })) if name == "person_text"
        ));
        assert!(matches!(
            parse_index_command("SHOW FULLTEXT INDEXES"),
            Ok(("", IndexCommand::ShowFulltext))
        ));
        assert!(
            parse_index_command(
                "CREATE FULLTEXT INDEX person_text FOR (n:Person) ON EACH [m.name] ANALYZER english"
            )
            .is_err()
        );
    }

    #[test]
    fn parses_vector_index_lifecycle_commands() {
        assert!(matches!(
            parse_index_command(
                "CREATE VECTOR INDEX person_embedding FOR (n:Person) ON (n.embedding) DIMENSIONS 3 DISTANCE cosine"
            ),
            Ok(("", IndexCommand::CreateVector {
                name, label, property, dimensions: 3, distance
            })) if name == "person_embedding" && label == "Person"
                && property == "embedding" && distance == "cosine"
        ));
        assert!(matches!(
            parse_index_command("DROP VECTOR INDEX person_embedding"),
            Ok(("", IndexCommand::DropVector { name })) if name == "person_embedding"
        ));
        assert!(matches!(
            parse_index_command("SHOW VECTOR INDEXES"),
            Ok(("", IndexCommand::ShowVector))
        ));
    }
}
