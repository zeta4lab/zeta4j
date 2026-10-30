//! Clause parsing
//!
//! MATCH, CREATE, DELETE, SET, MERGE, RETURN, WITH, UNWIND, CALL 절 파싱

use nom::{
    IResult,
    branch::alt,
    bytes::complete::{escaped_transform, tag, tag_no_case},
    character::complete::{char, digit1, multispace0, multispace1, none_of},
    combinator::{map, map_res, opt, value},
    multi::{many0, many1, separated_list1},
    sequence::{delimited, preceded, tuple},
};

use super::expressions::{parse_expression, parse_map_expression};
use super::patterns::parse_pattern;
use super::utils::{identifier, ws};
use crate::ast::*;
use zeta4j_graph::core::Value;

// ============================================================
// MATCH 절
// ============================================================

/// WHERE 절 파싱
pub fn parse_where(input: &str) -> IResult<&str, Expression> {
    preceded(tag_no_case("WHERE"), ws(parse_expression))(input)
}

/// MATCH 절 파싱
/// 콤마로 분리된 여러 패턴 지원: MATCH (a), (b), (c)-[:REL]->(d) WHERE ...
pub fn parse_match(input: &str) -> IResult<&str, ReadingClause> {
    map(
        tuple((
            opt(tuple((tag_no_case("OPTIONAL"), multispace1))),
            tag_no_case("MATCH"),
            ws(separated_list1(ws(char(',')), ws(parse_pattern))),
            opt(ws(parse_where)),
        )),
        |(optional, _, patterns, where_clause)| ReadingClause::Match {
            patterns,
            optional: optional.is_some(),
            where_clause,
        },
    )(input)
}

// ============================================================
// LOAD CSV 절
// ============================================================

/// LOAD CSV 절 파싱
/// LOAD CSV [WITH HEADERS] FROM 'url' AS variable [FIELDTERMINATOR 'char']
pub fn parse_load_csv(input: &str) -> IResult<&str, ReadingClause> {
    let (input, _) = ws(tag_no_case("LOAD"))(input)?;
    let (input, _) = ws(tag_no_case("CSV"))(input)?;

    // WITH HEADERS 옵션
    let (input, with_headers) =
        opt(tuple((ws(tag_no_case("WITH")), ws(tag_no_case("HEADERS")))))(input)?;

    let (input, _) = ws(tag_no_case("FROM"))(input)?;

    // URL 파싱 (문자열 리터럴)
    let (input, url) = ws(parse_string_literal)(input)?;

    let (input, _) = ws(tag_no_case("AS"))(input)?;
    let (input, variable) = ws(identifier)(input)?;

    // FIELDTERMINATOR 옵션
    let (input, field_terminator) = opt(tuple((
        ws(tag_no_case("FIELDTERMINATOR")),
        ws(parse_string_literal),
    )))(input)?;

    let terminator = field_terminator.and_then(|(_, s)| s.chars().next());

    Ok((
        input,
        ReadingClause::LoadCsv {
            with_headers: with_headers.is_some(),
            url,
            variable,
            field_terminator: terminator,
        },
    ))
}

/// 문자열 리터럴 파싱 ('...' 또는 "...")
/// 지원하는 이스케이프: \\, \', \", \n, \r, \t
fn parse_string_literal(input: &str) -> IResult<&str, String> {
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

    alt((empty_single, empty_double, single_quote, double_quote))(input)
}

// ============================================================
// CREATE 절
// ============================================================

/// CREATE 절 파싱
/// 콤마로 구분된 여러 패턴 지원: CREATE (a:Node), (b:Node), (c:Node)
pub fn parse_create(input: &str) -> IResult<&str, UpdatingClause> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, patterns) = ws(separated_list1(ws(char(',')), parse_pattern))(input)?;

    // 여러 패턴의 elements와 parts를 하나의 Pattern으로 병합
    let mut combined_elements = Vec::new();
    let mut combined_parts = Vec::new();

    for pattern in patterns {
        combined_elements.extend(pattern.elements);
        combined_parts.extend(pattern.parts);
    }

    let combined_pattern = Pattern {
        elements: combined_elements,
        parts: combined_parts,
        variable: None,
        path_function: None,
    };

    Ok((input, UpdatingClause::Create(combined_pattern)))
}

// ============================================================
// DELETE 절
// ============================================================

/// DELETE 절 파싱 (DELETE n, m 또는 DETACH DELETE n)
pub fn parse_delete(input: &str) -> IResult<&str, UpdatingClause> {
    let (input, detach) = opt(ws(tag_no_case("DETACH")))(input)?;
    let (input, _) = ws(tag_no_case("DELETE"))(input)?;
    let (input, variables) = ws(separated_list1(ws(char(',')), identifier))(input)?;

    Ok((
        input,
        UpdatingClause::Delete {
            variables,
            detach: detach.is_some(),
        },
    ))
}

// ============================================================
// SET 절
// ============================================================

/// SET 개별 속성 파싱 (n.prop = value)
fn parse_set_property(input: &str) -> IResult<&str, SetItem> {
    map(
        tuple((
            identifier,
            ws(char('.')),
            identifier,
            ws(char('=')),
            parse_expression,
        )),
        |(variable, _, property, _, value)| SetItem::Property {
            variable,
            property,
            value,
        },
    )(input)
}

/// SET 속성 병합 파싱 (n += {props})
fn parse_set_merge_properties(input: &str) -> IResult<&str, SetItem> {
    map(
        tuple((
            identifier,
            ws(nom::bytes::complete::tag("+=")),
            parse_map_expression,
        )),
        |(variable, _, properties)| SetItem::MergeProperties {
            variable,
            properties,
        },
    )(input)
}

/// SET 속성 전체 교체 파싱 (n = {props})
fn parse_set_all_properties(input: &str) -> IResult<&str, SetItem> {
    map(
        tuple((identifier, ws(char('=')), parse_map_expression)),
        |(variable, _, properties)| SetItem::AllProperties {
            variable,
            properties,
        },
    )(input)
}

/// SET 레이블 추가 파싱 (n:Label 또는 n:Label1:Label2)
fn parse_set_add_label(input: &str) -> IResult<&str, SetItem> {
    map(
        tuple((identifier, many1(preceded(char(':'), identifier)))),
        |(variable, labels)| SetItem::AddLabel { variable, labels },
    )(input)
}

/// SET 항목 파싱 (모든 타입)
fn parse_set_item(input: &str) -> IResult<&str, SetItem> {
    alt((
        parse_set_property,
        parse_set_merge_properties,
        parse_set_all_properties,
        parse_set_add_label,
    ))(input)
}

/// SET 절 파싱 (SET n.prop = value, m.prop = value)
pub fn parse_set(input: &str) -> IResult<&str, UpdatingClause> {
    preceded(
        tag_no_case("SET"),
        ws(map(
            separated_list1(ws(char(',')), ws(parse_set_item)),
            |items| UpdatingClause::Set { items },
        )),
    )(input)
}

// ============================================================
// REMOVE 절
// ============================================================

/// REMOVE 속성 파싱 (n.prop)
fn parse_remove_property(input: &str) -> IResult<&str, RemoveItem> {
    map(
        tuple((identifier, ws(char('.')), identifier)),
        |(variable, _, property)| RemoveItem::Property { variable, property },
    )(input)
}

/// REMOVE 레이블 파싱 (n:Label 또는 n:Label1:Label2)
fn parse_remove_label(input: &str) -> IResult<&str, RemoveItem> {
    map(
        tuple((identifier, many1(preceded(char(':'), identifier)))),
        |(variable, labels)| RemoveItem::Label { variable, labels },
    )(input)
}

/// REMOVE 항목 파싱 (모든 타입)
fn parse_remove_item(input: &str) -> IResult<&str, RemoveItem> {
    alt((parse_remove_property, parse_remove_label))(input)
}

/// REMOVE 절 파싱 (REMOVE n.prop, m:Label)
pub fn parse_remove(input: &str) -> IResult<&str, UpdatingClause> {
    preceded(
        tag_no_case("REMOVE"),
        ws(map(
            separated_list1(ws(char(',')), ws(parse_remove_item)),
            |items| UpdatingClause::Remove { items },
        )),
    )(input)
}

// ============================================================
// FOREACH 절
// ============================================================

/// FOREACH 내부의 업데이트 절 파싱
fn parse_foreach_update(input: &str) -> IResult<&str, UpdatingClause> {
    alt((
        parse_create,
        parse_delete,
        parse_set,
        parse_remove,
        parse_merge,
        parse_foreach, // FOREACH 중첩 지원
    ))(input)
}

/// FOREACH 절 파싱
/// FOREACH (variable IN list | updates...)
pub fn parse_foreach(input: &str) -> IResult<&str, UpdatingClause> {
    let (input, _) = ws(tag_no_case("FOREACH"))(input)?;
    let (input, _) = ws(char('('))(input)?;
    let (input, variable) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("IN"))(input)?;
    let (input, list) = ws(parse_expression)(input)?;
    let (input, _) = ws(char('|'))(input)?;
    let (input, updates) = many1(ws(parse_foreach_update))(input)?;
    let (input, _) = ws(char(')'))(input)?;

    Ok((
        input,
        UpdatingClause::Foreach {
            variable,
            list,
            updates,
        },
    ))
}

// ============================================================
// MERGE 절
// ============================================================

/// ON CREATE SET 또는 ON MATCH SET 파싱
fn parse_on_action_set(input: &str) -> IResult<&str, (&str, Vec<SetItem>)> {
    let (input, _) = ws(tag_no_case("ON"))(input)?;
    let (input, action) = ws(alt((tag_no_case("CREATE"), tag_no_case("MATCH"))))(input)?;
    let (input, _) = ws(tag_no_case("SET"))(input)?;
    let (input, items) = ws(separated_list1(ws(char(',')), ws(parse_set_item)))(input)?;
    Ok((input, (action, items)))
}

/// MERGE 절 파싱
pub fn parse_merge(input: &str) -> IResult<&str, UpdatingClause> {
    let (input, _) = ws(tag_no_case("MERGE"))(input)?;
    let (input, pattern) = ws(parse_pattern)(input)?;
    let (input, actions) = many0(parse_on_action_set)(input)?;

    let mut on_create = None;
    let mut on_match = None;

    for (action, items) in actions {
        if action.eq_ignore_ascii_case("CREATE") {
            on_create = Some(items);
        } else if action.eq_ignore_ascii_case("MATCH") {
            on_match = Some(items);
        }
    }

    Ok((
        input,
        UpdatingClause::Merge {
            pattern,
            on_create,
            on_match,
        },
    ))
}

// ============================================================
// RETURN 절
// ============================================================

/// RETURN 항목 파싱
pub fn parse_return_item(input: &str) -> IResult<&str, ReturnItem> {
    let (input, expr) = ws(alt((
        map(nom::bytes::complete::tag("*"), |_| {
            (Expression::Literal(Value::Null), Some("*".to_string()))
        }),
        |i| {
            let (i, e) = parse_expression(i)?;
            let (i, alias) = opt(preceded(ws(tag_no_case("AS")), ws(identifier)))(i)?;
            Ok((i, (e, alias)))
        },
    )))(input)?;

    if expr.1 == Some("*".to_string()) {
        return Ok((input, ReturnItem::Star));
    }

    Ok((
        input,
        ReturnItem::Expression {
            expr: expr.0,
            alias: expr.1,
        },
    ))
}

/// ORDER BY 항목 파싱
fn parse_order_item(input: &str) -> IResult<&str, OrderItem> {
    let (input, expr) = ws(parse_expression)(input)?;
    let (input, direction) = opt(ws(alt((
        value(true, tag_no_case("ASC")),
        value(false, tag_no_case("DESC")),
    ))))(input)?;

    Ok((
        input,
        OrderItem {
            expr,
            ascending: direction.unwrap_or(true),
        },
    ))
}

/// ORDER BY 절 파싱
fn parse_order_by(input: &str) -> IResult<&str, Vec<OrderItem>> {
    preceded(
        tuple((ws(tag_no_case("ORDER")), ws(tag_no_case("BY")))),
        separated_list1(ws(char(',')), parse_order_item),
    )(input)
}

/// SKIP 절 파싱
fn parse_skip(input: &str) -> IResult<&str, u64> {
    preceded(
        ws(tag_no_case("SKIP")),
        ws(map_res(digit1, |s: &str| s.parse::<u64>())),
    )(input)
}

/// LIMIT 절 파싱
fn parse_limit(input: &str) -> IResult<&str, u64> {
    preceded(
        ws(tag_no_case("LIMIT")),
        ws(map_res(digit1, |s: &str| s.parse::<u64>())),
    )(input)
}

/// RETURN 절 파싱
pub fn parse_return(input: &str) -> IResult<&str, ReturnClause> {
    let (input, _) = ws(tag_no_case("RETURN"))(input)?;
    let (input, distinct) = opt(ws(tag_no_case("DISTINCT")))(input)?;
    let (input, items) = separated_list1(ws(char(',')), parse_return_item)(input)?;
    let (input, order_by) = opt(parse_order_by)(input)?;
    let (input, skip) = opt(parse_skip)(input)?;
    let (input, limit) = opt(parse_limit)(input)?;

    Ok((
        input,
        ReturnClause {
            items,
            distinct: distinct.is_some(),
            order_by,
            skip,
            limit,
        },
    ))
}

// ============================================================
// WITH 절
// ============================================================

/// WITH 절 파싱
pub fn parse_with(input: &str) -> IResult<&str, WithClause> {
    let (input, _) = ws(tag_no_case("WITH"))(input)?;
    let (input, distinct) = opt(ws(tag_no_case("DISTINCT")))(input)?;
    let (input, items) = separated_list1(ws(char(',')), parse_return_item)(input)?;
    let (input, order_by) = opt(parse_order_by)(input)?;
    let (input, skip) = opt(parse_skip)(input)?;
    let (input, limit) = opt(parse_limit)(input)?;
    let (input, where_clause) = opt(ws(parse_where))(input)?;

    Ok((
        input,
        WithClause {
            distinct: distinct.is_some(),
            items,
            order_by,
            skip,
            limit,
            where_clause,
        },
    ))
}

// ============================================================
// UNWIND 절
// ============================================================

/// UNWIND 절 파서 (UNWIND expr AS alias)
pub fn parse_unwind(input: &str) -> IResult<&str, UnwindClause> {
    map(
        tuple((
            tag_no_case("UNWIND"),
            multispace1,
            parse_expression,
            multispace1,
            tag_no_case("AS"),
            multispace1,
            identifier,
        )),
        |(_, _, expr, _, _, _, alias)| UnwindClause { expr, alias },
    )(input)
}

/// LET 절 파서 (Neo4j 5.x)
///
/// WITH 없이 변수를 표현식에 바인딩합니다.
///
/// # 문법
///
/// ```text
/// LET variable = expression
/// ```
///
/// # 예제
///
/// ```text
/// LET threshold = 100
/// MATCH (n:Product) WHERE n.price > threshold RETURN n
/// ```
pub fn parse_let(input: &str) -> IResult<&str, LetClause> {
    map(
        tuple((
            tag_no_case("LET"),
            multispace1,
            identifier,
            ws(char('=')),
            ws(parse_expression),
        )),
        |(_, _, variable, _, expression)| LetClause {
            variable,
            expression,
        },
    )(input)
}

/// USE 절 파서 (데이터베이스 선택)
///
/// 쿼리를 실행할 데이터베이스를 지정합니다.
///
/// # 문법
///
/// ```text
/// USE database_name
/// USE graph.byName($dbName)
/// ```
///
/// # 예제
///
/// ```text
/// USE mydb
/// MATCH (n) RETURN n
/// ```
pub fn parse_use(input: &str) -> IResult<&str, UseClause> {
    let (input, _) = tag_no_case("USE")(input)?;
    let (input, _) = multispace1(input)?;

    // graph.byName(...) 또는 정적 데이터베이스 이름
    let (input, database) = alt((
        // graph.byName($param) 또는 graph.byName("name")
        map(
            tuple((
                tag_no_case("graph"),
                char('.'),
                tag_no_case("byName"),
                ws(char('(')),
                ws(parse_expression),
                ws(char(')')),
            )),
            |(_, _, _, _, expr, _)| DatabaseTarget::Dynamic(expr),
        ),
        // 정적 데이터베이스 이름
        map(identifier, DatabaseTarget::Named),
    ))(input)?;

    Ok((input, UseClause { database }))
}

// ============================================================
// CALL 서브쿼리 / 프로시저 호출
// ============================================================

/// CALL 서브쿼리 파서
/// CALL { subquery }
pub fn parse_call_subquery(input: &str) -> IResult<&str, CallSubquery> {
    let (input, _) = ws(tag_no_case("CALL"))(input)?;
    let (input, _) = ws(char('{'))(input)?;
    let (input, subquery) = super::parse_single_query(input)?;
    let (input, _) = ws(char('}'))(input)?;
    Ok((input, CallSubquery { subquery }))
}

/// Phase 3: 프로시저 호출 파서
/// CALL db.index.vector.queryNodes('idx', 10, $vec) YIELD node, score
pub fn parse_procedure_call(input: &str) -> IResult<&str, ProcedureCall> {
    use nom::multi::separated_list0;

    let (input, _) = ws(tag_no_case("CALL"))(input)?;

    // 프로시저 이름 파싱 (점으로 구분된 식별자)
    let (input, name) = parse_dotted_procedure_name(input)?;

    // 인수 목록 파싱
    let (input, _) = ws(char('('))(input)?;
    let (input, arguments) =
        separated_list0(ws(char(',')), super::expressions::parse_expression)(input)?;
    let (input, _) = ws(char(')'))(input)?;

    // YIELD 절 파싱 (선택적)
    let (input, yields) = opt(parse_yield_clause)(input)?;

    Ok((
        input,
        ProcedureCall {
            name,
            arguments,
            yields,
        },
    ))
}

/// 점으로 구분된 프로시저 이름 파싱
/// e.g., "db.index.vector.queryNodes"
fn parse_dotted_procedure_name(input: &str) -> IResult<&str, String> {
    use nom::multi::many0;

    let (input, _) = multispace0(input)?;
    let (input, first) = identifier(input)?;
    let (input, rest) = many0(preceded(char('.'), identifier))(input)?;

    if rest.is_empty() {
        Ok((input, first))
    } else {
        let mut name = first;
        for part in rest {
            name.push('.');
            name.push_str(&part);
        }
        Ok((input, name))
    }
}

/// YIELD 절 파서
/// YIELD node, score AS similarity
fn parse_yield_clause(input: &str) -> IResult<&str, Vec<YieldItem>> {
    use nom::multi::separated_list1;

    let (input, _) = ws(tag_no_case("YIELD"))(input)?;
    let (input, items) = separated_list1(ws(char(',')), parse_yield_item)(input)?;

    Ok((input, items))
}

/// 개별 YIELD 항목 파서
/// variable [AS alias]
fn parse_yield_item(input: &str) -> IResult<&str, YieldItem> {
    let (input, _) = multispace0(input)?;
    let (input, variable) = identifier(input)?;

    // AS alias (선택적)
    let (input, alias) = opt(|i| {
        let (i, _) = ws(tag_no_case("AS"))(i)?;
        let (i, _) = multispace0(i)?;
        let (i, alias) = identifier(i)?;
        Ok((i, alias))
    })(input)?;

    Ok((input, YieldItem { variable, alias }))
}
