// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Cypher Query Parser
//!
//! OpenCypher 쿼리 문자열을 파싱하여 AST(Abstract Syntax Tree)로 변환하는 파서 모듈입니다.
//!
//! # 주요 기능
//!
//! - **완전한 Cypher 문법 지원**: MATCH, CREATE, SET, DELETE, MERGE, WITH, UNWIND, RETURN 등
//! - **표현식 파싱**: 리터럴, 변수, 속성 접근, 비교/논리 연산, 함수 호출
//! - **패턴 매칭**: 노드, 관계, 경로 패턴 (가변 길이 포함)
//! - **인덱스/스키마/제약조건 DDL**: CREATE INDEX, CREATE CONSTRAINT 등
//! - **UNION 지원**: 여러 쿼리 결합
//! - **EXPLAIN/ANALYZE**: 쿼리 실행 계획 분석
//!
//! # 사용법
//!
//! ```ignore
//! use crate::parser::parse_query;
//!
//! // 기본 쿼리 파싱
//! let (remaining, query) = parse_query("MATCH (n:Person) RETURN n.name").unwrap();
//! assert!(remaining.is_empty());
//!
//! // EXPLAIN 쿼리
//! let (_, query) = parse_query("EXPLAIN MATCH (n) RETURN n").unwrap();
//! assert!(query.explain);
//!
//! // UNION 쿼리
//! let (_, query) = parse_query("RETURN 1 AS n UNION RETURN 2 AS n").unwrap();
//! ```
//!
//! # 파서 구조
//!
//! ```text
//! parser/
//! ├── mod.rs          - 메인 파서 (parse_query, parse_single_query)
//! ├── utils.rs        - 공통 유틸리티 (ws, parse_identifier)
//! ├── primitives.rs   - 기본 값 파서 (문자열, 숫자, 불리언, null)
//! ├── values.rs       - 프로퍼티 패턴 파서 (표현식 지원)
//! ├── expressions.rs  - 표현식 파서
//! ├── patterns.rs     - 패턴 파서 (노드, 관계)
//! ├── clauses.rs      - 절 파서 (MATCH, CREATE, RETURN 등)
//! ├── functions.rs    - 함수 호출 파서
//! ├── index.rs        - 인덱스 명령 파서
//! ├── schema.rs       - 스키마 명령 파서
//! └── constraints.rs  - 제약조건 명령 파서
//! ```
//!
//! # Re-exports
//!
//! - [`parse_expression`]: 표현식 파서 (WHERE 조건 등에서 사용)
//! - [`parse_pattern`]: 패턴 파서 (MATCH, CREATE 패턴에서 사용)

const MAX_QUERY_INPUT_BYTES: usize = 1024 * 1024;
const MAX_QUERY_DELIMITER_DEPTH: usize = 128;
const MAX_QUERY_TOKENS: usize = 32_768;
const MAX_QUERY_CHAIN_OPERATORS: usize = 1_024;
const MAX_QUERY_UNIONS: usize = 256;
const MAX_QUERY_CASE_EXPRESSIONS: usize = 128;

#[derive(Default)]
struct QueryInputStats {
    tokens: usize,
    chain_operators: usize,
    unions: usize,
    case_expressions: usize,
    can_end_expression: bool,
}

impl QueryInputStats {
    fn within_limits(&self) -> bool {
        self.tokens <= MAX_QUERY_TOKENS
            && self.chain_operators <= MAX_QUERY_CHAIN_OPERATORS
            && self.unions <= MAX_QUERY_UNIONS
            && self.case_expressions <= MAX_QUERY_CASE_EXPRESSIONS
    }

    fn record_word(&mut self, input: &str, start: usize, end: usize) -> bool {
        if start == end {
            return self.within_limits();
        }
        self.tokens = self.tokens.saturating_add(1);
        let word = &input[start..end];
        if word.eq_ignore_ascii_case("UNION") {
            self.unions = self.unions.saturating_add(1);
            self.chain_operators = self.chain_operators.saturating_add(1);
            self.can_end_expression = false;
        } else if word.eq_ignore_ascii_case("AND") || word.eq_ignore_ascii_case("OR") {
            self.chain_operators = self.chain_operators.saturating_add(1);
            self.can_end_expression = false;
        } else if word.eq_ignore_ascii_case("CASE") {
            self.case_expressions = self.case_expressions.saturating_add(1);
            self.can_end_expression = false;
        } else {
            self.can_end_expression = true;
        }
        self.within_limits()
    }

    fn record_symbol(&mut self, symbol: char) -> bool {
        self.tokens = self.tokens.saturating_add(1);
        match symbol {
            '+' | '-' => {
                if self.can_end_expression {
                    self.chain_operators = self.chain_operators.saturating_add(1);
                }
                self.can_end_expression = false;
            }
            '*' | '/' | '%' | '^' | '=' | '<' | '>' => {
                self.chain_operators = self.chain_operators.saturating_add(1);
                self.can_end_expression = false;
            }
            ')' | ']' | '}' | '\'' | '"' | '`' => self.can_end_expression = true,
            _ => self.can_end_expression = false,
        }
        self.within_limits()
    }
}

fn query_bound_failure(input: &str) -> nom::Err<nom::error::Error<&str>> {
    nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
}

fn validate_query_input_bounds(input: &str) -> IResult<&str, ()> {
    if input.len() > MAX_QUERY_INPUT_BYTES {
        return Err(query_bound_failure(input));
    }

    let mut depth = 0_usize;
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut word_start = None;
    let mut stats = QueryInputStats::default();
    let mut characters = input.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if line_comment {
            if character == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_comment {
            if character == '*' && characters.peek().is_some_and(|(_, next)| *next == '/') {
                let _ = characters.next();
                block_comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        if character == '/' && characters.peek().is_some_and(|(_, next)| *next == '/') {
            if let Some(start) = word_start.take()
                && !stats.record_word(input, start, index)
            {
                return Err(query_bound_failure(input));
            }
            let _ = characters.next();
            line_comment = true;
            continue;
        }
        if character == '/' && characters.peek().is_some_and(|(_, next)| *next == '*') {
            if let Some(start) = word_start.take()
                && !stats.record_word(input, start, index)
            {
                return Err(query_bound_failure(input));
            }
            let _ = characters.next();
            block_comment = true;
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            if let Some(start) = word_start.take()
                && !stats.record_word(input, start, index)
            {
                return Err(query_bound_failure(input));
            }
            if !stats.record_symbol(character) {
                return Err(query_bound_failure(input));
            }
            quote = Some(character);
            continue;
        }
        if character.is_alphanumeric() || matches!(character, '_' | '$') {
            if word_start.is_none() {
                word_start = Some(index);
            }
            continue;
        }
        if let Some(start) = word_start.take()
            && !stats.record_word(input, start, index)
        {
            return Err(query_bound_failure(input));
        }
        if character.is_whitespace() {
            continue;
        }
        if !stats.record_symbol(character) {
            return Err(query_bound_failure(input));
        }
        match character {
            '(' | '[' | '{' => {
                depth += 1;
                if depth > MAX_QUERY_DELIMITER_DEPTH {
                    return Err(query_bound_failure(input));
                }
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if let Some(start) = word_start
        && !stats.record_word(input, start, input.len())
    {
        return Err(query_bound_failure(input));
    }
    Ok((input, ()))
}

mod admin;
mod clauses;
mod constraints;
mod expressions;
mod functions;
mod index;
mod patterns;
mod primitives;
mod schema;
pub mod splitter;
mod utils;
mod values;

use nom::{
    IResult,
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::multispace0,
    combinator::{map, opt},
    multi::many1,
    sequence::{preceded, terminated},
};

use crate::ast::*;
use clauses::{
    parse_call_subquery, parse_create, parse_delete, parse_foreach, parse_let, parse_load_csv,
    parse_match, parse_merge, parse_procedure_call, parse_remove, parse_return, parse_set,
    parse_unwind, parse_with,
};
use utils::ws;
// parse_use는 아래에서 pub use로 export
use admin::parse_admin_command;
use constraints::parse_constraint_command;
use index::parse_index_command;
use schema::parse_schema_command;

// Re-export commonly used parsers
pub use clauses::parse_use;
pub use expressions::parse_expression;
pub use patterns::parse_pattern;
pub use splitter::{split_statements, split_statements_owned};

/// 개별 Statement 파싱
///
/// 입력 문자열에서 하나의 Statement를 파싱합니다.
/// Statement는 MATCH, CREATE, RETURN 등의 Cypher 절입니다.
///
/// # 지원 Statement
///
/// - Reading: MATCH, OPTIONAL MATCH
/// - Updating: CREATE, DELETE, SET, MERGE
/// - With/Unwind: WITH, UNWIND
/// - Return: RETURN
/// - Call: CALL { subquery }
/// - Procedure: CALL db.index.vector.queryNodes(...) YIELD ...
/// - DDL: CREATE INDEX, CREATE CONSTRAINT, CREATE NODE LABEL 등
fn parse_statement(input: &str) -> IResult<&str, Statement> {
    alt((
        // Reading / DDL 절
        alt((
            map(parse_use, Statement::Use),
            map(parse_load_csv, Statement::Reading),
            map(parse_match, Statement::Reading),
            map(parse_admin_command, Statement::Admin),
            map(parse_schema_command, Statement::Schema),
            map(parse_constraint_command, Statement::Constraint),
            map(parse_index_command, Statement::Index),
        )),
        // Updating 절
        alt((
            map(parse_create, Statement::Updating),
            map(parse_merge, Statement::Updating),
            map(parse_foreach, Statement::Updating),
            map(parse_delete, Statement::Updating),
            map(parse_set, Statement::Updating),
            map(parse_remove, Statement::Updating),
        )),
        // Projection / Control 절
        alt((
            map(parse_with, Statement::With),
            map(parse_unwind, Statement::Unwind),
            map(parse_call_subquery, Statement::Call),
            map(parse_procedure_call, Statement::Procedure),
            map(parse_return, Statement::Return),
            map(parse_finish, |_| Statement::Finish),
            map(parse_let, Statement::Let),
        )),
    ))(input)
}

/// FINISH 키워드 파싱 (Neo4j 5.x)
///
/// 쿼리가 결과를 반환하지 않음을 명시합니다.
/// 부작용만 있는 쿼리에 사용됩니다.
///
/// # 예제
///
/// ```text
/// MATCH (n:Person) SET n.processed = true FINISH
/// ```
fn parse_finish(input: &str) -> IResult<&str, ()> {
    let (input, _) = tag_no_case("FINISH")(input)?;
    Ok((input, ()))
}

/// 단일 쿼리 파싱 (UNION 없는 쿼리)
///
/// 하나 이상의 Statement로 구성된 단일 쿼리를 파싱합니다.
/// Statement들은 파이프라인으로 연결되어 순서대로 실행됩니다.
///
/// # 예제
///
/// ```ignore
/// use crate::parser::parse_single_query;
///
/// // 단일 Statement
/// let (_, query) = parse_single_query("RETURN 1").unwrap();
/// assert_eq!(query.statements.len(), 1);
///
/// // 다중 Statement 파이프라인
/// let (_, query) = parse_single_query("MATCH (n) WHERE n.age > 30 RETURN n").unwrap();
/// assert_eq!(query.statements.len(), 2); // MATCH + RETURN
/// ```
///
/// # 반환값
///
/// - `Ok((remaining, SingleQuery))`: 파싱 성공, 남은 문자열과 파싱된 쿼리
/// - `Err(nom::Err)`: 파싱 실패
pub fn parse_single_query(input: &str) -> IResult<&str, SingleQuery> {
    let _ = validate_query_input_bounds(input)?;
    parse_single_query_unchecked(input)
}

fn parse_single_query_unchecked(input: &str) -> IResult<&str, SingleQuery> {
    map(many1(ws(parse_statement)), |statements| SingleQuery {
        statements,
    })(input)
}

/// UNION 키워드 파싱
///
/// UNION 또는 UNION ALL 키워드를 파싱합니다.
///
/// # 반환값
///
/// - `true`: UNION ALL (중복 포함)
/// - `false`: UNION (중복 제거)
fn parse_union_keyword(input: &str) -> IResult<&str, bool> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("UNION")(input)?;
    let (input, all) = opt(preceded(
        nom::character::complete::multispace1,
        tag_no_case("ALL"),
    ))(input)?;
    let (input, _) = multispace0(input)?;
    Ok((input, all.is_some()))
}

/// 쿼리 본체 파싱 (단일 쿼리 또는 UNION)
///
/// 단일 쿼리 또는 UNION으로 결합된 여러 쿼리를 파싱합니다.
/// 여러 UNION이 있는 경우 왼쪽부터 순차적으로 결합됩니다.
///
/// # 예제
///
/// ```text
/// // 단일 쿼리
/// MATCH (n) RETURN n
/// -> QueryBody::Single(...)
///
/// // UNION
/// RETURN 1 AS n UNION RETURN 2 AS n
/// -> QueryBody::Union { left: ..., right: ..., all: false }
///
/// // 다중 UNION
/// RETURN 1 UNION RETURN 2 UNION RETURN 3
/// -> QueryBody::Union {
///      left: QueryBody::Union { ... },  // 1 UNION 2
///      right: QueryBody::Single(...),    // 3
///      all: false
///    }
/// ```
fn parse_query_body(input: &str) -> IResult<&str, QueryBody> {
    let (input, first) = parse_single_query_unchecked(input)?;
    let mut body = QueryBody::Single(first);

    let mut remaining = input;
    while let Ok((after_union, all)) = parse_union_keyword(remaining) {
        match parse_single_query_unchecked(after_union) {
            Ok((after_query, next)) => {
                body = QueryBody::Union {
                    left: Box::new(body),
                    right: Box::new(QueryBody::Single(next)),
                    all,
                };
                remaining = after_query;
            }
            Err(_) => break,
        }
    }

    Ok((remaining, body))
}

/// Cypher 쿼리 파싱 (최상위 파서)
///
/// 완전한 Cypher 쿼리 문자열을 파싱하여 AST로 변환합니다.
/// EXPLAIN/ANALYZE 키워드, UNION, 모든 Statement 유형을 지원합니다.
///
/// # 인자
///
/// * `input` - 파싱할 Cypher 쿼리 문자열
///
/// # 반환값
///
/// * `Ok((remaining, Query))` - 파싱 성공
///   - `remaining`: 파싱 후 남은 문자열 (보통 빈 문자열)
///   - `Query`: 파싱된 쿼리 AST
/// * `Err(nom::Err)` - 파싱 실패
///
/// # 예제
///
/// ```ignore
/// use crate::parser::parse_query;
///
/// // 기본 쿼리
/// let (remaining, query) = parse_query("MATCH (n:Person) RETURN n").unwrap();
/// assert!(remaining.trim().is_empty());
/// assert!(!query.explain);
///
/// // EXPLAIN 쿼리 - 실행 계획만 반환
/// let (_, query) = parse_query("EXPLAIN MATCH (n) RETURN n").unwrap();
/// assert!(query.explain);
/// assert!(!query.analyze);
///
/// // EXPLAIN ANALYZE 쿼리 - 실행하며 통계 수집
/// let (_, query) = parse_query("EXPLAIN ANALYZE MATCH (n) RETURN n").unwrap();
/// assert!(query.explain);
/// assert!(query.analyze);
///
/// // UNION 쿼리
/// let (_, query) = parse_query("RETURN 1 AS n UNION ALL RETURN 2 AS n").unwrap();
/// if let QueryBody::Union { all, .. } = &query.body {
///     assert!(all);
/// }
/// ```
///
/// # 지원 구문
///
/// - **EXPLAIN [ANALYZE]**: 쿼리 실행 계획 분석
/// - **MATCH / OPTIONAL MATCH**: 패턴 매칭
/// - **CREATE / DELETE / SET / MERGE**: 데이터 변경
/// - **WITH / UNWIND**: 중간 결과 처리
/// - **RETURN**: 결과 반환
/// - **UNION / UNION ALL**: 쿼리 결합
/// - **CREATE INDEX / DROP INDEX**: 인덱스 관리
/// - **CREATE CONSTRAINT**: 제약조건 관리
/// - **CREATE NODE LABEL / CREATE RELATIONSHIP TYPE**: 스키마 관리
pub fn parse_query(input: &str) -> IResult<&str, Query> {
    let _ = validate_query_input_bounds(input)?;
    let (input, _) = multispace0(input)?;

    // EXPLAIN [ANALYZE] 키워드 파싱 (선택적)
    let (input, explain_analyze) = opt(preceded(
        terminated(
            tag_no_case("EXPLAIN"),
            nom::character::complete::multispace1,
        ),
        opt(terminated(
            tag_no_case("ANALYZE"),
            nom::character::complete::multispace1,
        )),
    ))(input)?;

    let (explain, analyze) = match explain_analyze {
        Some(Some(_)) => (true, true),
        Some(None) => (true, false),
        None => (false, false),
    };

    let (input, body) = parse_query_body(input)?;

    Ok((
        input,
        Query {
            explain,
            analyze,
            body,
        },
    ))
}

/// ZetaError를 반환하는 안전한 쿼리 파싱 함수
///
/// nom의 `IResult`를 `Result<Query, ZetaError>`로 변환합니다.
/// 파싱 실패 시 구조화된 에러 코드와 함께 에러를 반환합니다.
///
/// # 예제
///
/// ```ignore
/// use zeta4j_cypher::parser::parse_query_safe;
///
/// let query = parse_query_safe("MATCH (n:Person) RETURN n")?;
/// ```
pub fn parse_query_safe(input: &str) -> Result<Query, zeta4j_common::ZetaError> {
    parse_query(input)
        .map_err(query_parse_error)
        .and_then(|(remaining, query)| {
            reject_remaining_input(remaining)?;
            Ok(query)
        })
}

/// ZetaError를 반환하는 안전한 단일 쿼리 파싱 함수
///
/// UNION이 없는 단일 쿼리를 파싱합니다.
pub fn parse_single_query_safe(input: &str) -> Result<SingleQuery, zeta4j_common::ZetaError> {
    parse_single_query(input)
        .map_err(query_parse_error)
        .and_then(|(remaining, query)| {
            reject_remaining_input(remaining)?;
            Ok(query)
        })
}

fn query_parse_error(nom_err: nom::Err<nom::error::Error<&str>>) -> zeta4j_common::ZetaError {
    use zeta4j_common::codes;

    let message = match nom_err {
        nom::Err::Incomplete(_) => "Incomplete query".to_string(),
        nom::Err::Error(error) | nom::Err::Failure(error) => {
            let snippet: String = error.input.chars().take(50).collect();
            format!("Parse error near: '{}'", snippet)
        }
    };
    zeta4j_common::ZetaError::QueryParse {
        code: codes::cypher::SYNTAX_ERROR,
        message,
    }
}

fn reject_remaining_input(remaining: &str) -> Result<(), zeta4j_common::ZetaError> {
    use zeta4j_common::codes;

    if remaining.trim().is_empty() {
        return Ok(());
    }
    let snippet: String = remaining.trim().chars().take(50).collect();
    Err(zeta4j_common::ZetaError::QueryParse {
        code: codes::cypher::SYNTAX_ERROR,
        message: format!("Unexpected trailing input near: '{}'", snippet),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeta4j_common::codes;

    #[test]
    fn safe_parser_rejects_unrecognized_query() -> Result<(), &'static str> {
        let Err(error) = parse_query_safe("THIS IS NOT CYPHER") else {
            return Err("unrecognized query unexpectedly parsed");
        };
        assert_eq!(error.code(), &codes::cypher::SYNTAX_ERROR);
        Ok(())
    }

    #[test]
    fn safe_parser_rejects_trailing_input() -> Result<(), &'static str> {
        let Err(error) = parse_query_safe("RETURN 1 AS value GARBAGE") else {
            return Err("trailing input unexpectedly parsed");
        };
        assert_eq!(error.code(), &codes::cypher::SYNTAX_ERROR);
        Ok(())
    }

    #[test]
    fn parser_rejects_oversized_and_excessively_nested_input_before_grammar_walk() {
        let oversized = format!("RETURN '{}'", "x".repeat(MAX_QUERY_INPUT_BYTES));
        assert!(parse_query_safe(&oversized).is_err());

        let nested = format!(
            "RETURN {}1{}",
            "(".repeat(MAX_QUERY_DELIMITER_DEPTH + 1),
            ")".repeat(MAX_QUERY_DELIMITER_DEPTH + 1)
        );
        assert!(parse_query_safe(&nested).is_err());
    }

    #[test]
    fn input_bound_scanner_ignores_delimiters_in_literals_and_comments() {
        let delimiters = "(".repeat(MAX_QUERY_DELIMITER_DEPTH + 1);
        let query = format!("RETURN '{delimiters}' AS value // {delimiters}");
        assert!(validate_query_input_bounds(&query).is_ok());
    }

    #[test]
    fn parser_rejects_pathological_ast_chains_before_building_them() {
        let arithmetic = format!(
            "RETURN {}",
            std::iter::repeat_n("1", MAX_QUERY_CHAIN_OPERATORS + 2)
                .collect::<Vec<_>>()
                .join("+")
        );
        assert!(parse_query_safe(&arithmetic).is_err());

        let unions = std::iter::repeat_n("RETURN 1", MAX_QUERY_UNIONS + 2)
            .collect::<Vec<_>>()
            .join(" UNION ");
        assert!(parse_query_safe(&unions).is_err());
    }

    #[test]
    fn parser_complexity_budget_preserves_large_vector_literals() {
        let elements = (0..3_072)
            .map(|index| if index % 2 == 0 { "0.125" } else { "-0.125" })
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("RETURN vector([{elements}]) AS embedding");
        assert!(parse_query_safe(&query).is_ok());
    }

    #[test]
    fn admin_parser_accepts_quoted_generated_principal_names() -> Result<(), &'static str> {
        let (remaining, command) = admin::parse_admin_command("DROP USER `operator-generated-123`")
            .map_err(|_| "quoted principal name was rejected")?;
        assert!(remaining.is_empty());
        assert_eq!(
            command,
            AdminCommand::DropUser {
                name: "operator-generated-123".to_string(),
                if_exists: false,
            }
        );
        Ok(())
    }

    #[test]
    fn ontology_ddl_is_not_cypher() {
        assert!(parse_query_safe("CREATE ONTOLOGY medical VERSION '1.0'").is_err());
        assert!(parse_query_safe("USE ONTOLOGY medical").is_err());
    }

    #[test]
    fn validation_command_is_not_cypher() {
        assert!(parse_query_safe("VALIDATE GRAPH").is_err());
    }

    #[test]
    fn role_and_privilege_commands_are_not_cypher() {
        for query in [
            "CREATE ROLE analyst",
            "DROP ROLE analyst",
            "SHOW ROLES",
            "GRANT ROLE analyst TO alice",
            "REVOKE ROLE analyst FROM alice",
            "SHOW PRIVILEGES",
        ] {
            assert!(parse_query_safe(query).is_err(), "{query} still parses");
        }
        assert!(parse_query_safe("SHOW USERS").is_ok());
        assert!(parse_query_safe("SHOW CURRENT USER").is_ok());
    }
}
