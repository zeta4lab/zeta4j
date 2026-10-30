//! 관리 명령어 파싱
//!
//! 사용자, 데이터베이스 관리 명령어 파싱
//!
//! # 지원 명령
//!
//! ## 사용자 관리
//! - `CREATE USER name SET PASSWORD 'password'`
//! - `DROP USER name`
//! - `ALTER USER name SET PASSWORD 'new_password'`
//! - `SHOW USERS`
//! - `SHOW CURRENT USER`
//!
//! ## 데이터베이스 관리
//! - `CREATE DATABASE name`
//! - `DROP DATABASE name`
//! - `SHOW DATABASES`

use nom::{
    IResult,
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{multispace0, multispace1},
    combinator::opt,
    sequence::tuple,
};

use super::primitives::parse_string as parse_string_value;
use super::utils::identifier_or_quoted;
use crate::ast::AdminCommand;
use zeta4j_graph::core::Value;

/// 문자열만 추출하는 헬퍼
fn parse_string(input: &str) -> IResult<&str, String> {
    let (input, value) = parse_string_value(input)?;
    match value {
        Value::String(s) => Ok((input, s)),
        _ => unreachable!("parse_string_value always returns Value::String"),
    }
}

// ============================================================
// 사용자 관리 명령어
// ============================================================

/// CREATE USER name [IF NOT EXISTS] SET PASSWORD 'password' [CHANGE [NOT] REQUIRED]
fn parse_create_user(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("USER")(input)?;
    let (input, _) = multispace1(input)?;

    // IF NOT EXISTS (optional)
    let (input, if_not_exists) = opt(tuple((
        tag_no_case("IF"),
        multispace1,
        tag_no_case("NOT"),
        multispace1,
        tag_no_case("EXISTS"),
        multispace1,
    )))(input)?;

    let (input, name) = identifier_or_quoted(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("SET")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("PASSWORD")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, password) = parse_string(input)?;
    let (input, _) = multispace0(input)?;

    // CHANGE [NOT] REQUIRED (optional)
    let (input, change_clause) = opt(tuple((
        tag_no_case("CHANGE"),
        multispace1,
        opt(tuple((tag_no_case("NOT"), multispace1))),
        tag_no_case("REQUIRED"),
    )))(input)?;

    let change_required = match change_clause {
        Some((_, _, Some(_), _)) => false, // CHANGE NOT REQUIRED
        Some((_, _, None, _)) => true,     // CHANGE REQUIRED
        None => false,                     // 기본값: 변경 불필요
    };

    Ok((
        input,
        AdminCommand::CreateUser {
            name,
            password,
            change_required,
            if_not_exists: if_not_exists.is_some(),
        },
    ))
}

/// DROP USER name [IF EXISTS]
fn parse_drop_user(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("USER")(input)?;
    let (input, _) = multispace1(input)?;

    let (input, name) = identifier_or_quoted(input)?;
    let (input, _) = multispace0(input)?;

    // IF EXISTS (optional)
    let (input, if_exists) = opt(tuple((
        tag_no_case("IF"),
        multispace1,
        tag_no_case("EXISTS"),
    )))(input)?;

    Ok((
        input,
        AdminCommand::DropUser {
            name,
            if_exists: if_exists.is_some(),
        },
    ))
}

/// ALTER USER name SET PASSWORD 'password' [CHANGE [NOT] REQUIRED]
/// ALTER USER name SET STATUS SUSPENDED
/// ALTER USER name SET STATUS ACTIVE
fn parse_alter_user(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("ALTER")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("USER")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier_or_quoted(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("SET")(input)?;
    let (input, _) = multispace1(input)?;

    // PASSWORD 또는 STATUS
    let (input, (password, change_required, suspended)) =
        alt((parse_alter_password, parse_alter_status))(input)?;

    Ok((
        input,
        AdminCommand::AlterUser {
            name,
            password,
            change_required,
            suspended,
        },
    ))
}

/// SET PASSWORD 'password' [CHANGE [NOT] REQUIRED] 부분 파싱
#[expect(clippy::type_complexity)]
fn parse_alter_password(
    input: &str,
) -> IResult<&str, (Option<String>, Option<bool>, Option<bool>)> {
    let (input, _) = tag_no_case("PASSWORD")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, password) = parse_string(input)?;
    let (input, _) = multispace0(input)?;

    let (input, change_clause) = opt(tuple((
        tag_no_case("CHANGE"),
        multispace1,
        opt(tuple((tag_no_case("NOT"), multispace1))),
        tag_no_case("REQUIRED"),
    )))(input)?;

    let change_required = change_clause.map(|(_, _, not, _)| not.is_none());

    Ok((input, (Some(password), change_required, None)))
}

/// SET STATUS SUSPENDED/ACTIVE 부분 파싱
#[expect(clippy::type_complexity)]
fn parse_alter_status(input: &str) -> IResult<&str, (Option<String>, Option<bool>, Option<bool>)> {
    let (input, _) = tag_no_case("STATUS")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, status) = alt((tag_no_case("SUSPENDED"), tag_no_case("ACTIVE")))(input)?;

    let suspended = status.to_uppercase() == "SUSPENDED";

    Ok((input, (None, None, Some(suspended))))
}

/// SHOW USERS
fn parse_show_users(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("USERS")(input)?;
    Ok((input, AdminCommand::ShowUsers))
}

/// SHOW CURRENT USER
fn parse_show_current_user(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("CURRENT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("USER")(input)?;
    Ok((input, AdminCommand::ShowCurrentUser))
}

// ============================================================
// 데이터베이스 관리 명령어
// ============================================================

/// CREATE DATABASE name [IF NOT EXISTS]
fn parse_create_database(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("CREATE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DATABASE")(input)?;
    let (input, _) = multispace1(input)?;

    // 데이터베이스 이름
    let (input, name) = identifier_or_quoted(input)?;
    let (input, _) = multispace0(input)?;

    // IF NOT EXISTS (optional, 이름 뒤에 위치)
    let (input, if_not_exists) = opt(tuple((
        tag_no_case("IF"),
        multispace1,
        tag_no_case("NOT"),
        multispace1,
        tag_no_case("EXISTS"),
    )))(input)?;

    Ok((
        input,
        AdminCommand::CreateDatabase {
            name,
            if_not_exists: if_not_exists.is_some(),
        },
    ))
}

/// DROP DATABASE name [IF EXISTS]
fn parse_drop_database(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("DROP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DATABASE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier_or_quoted(input)?;
    let (input, _) = multispace0(input)?;

    let (input, if_exists) = opt(tuple((
        tag_no_case("IF"),
        multispace1,
        tag_no_case("EXISTS"),
    )))(input)?;

    Ok((
        input,
        AdminCommand::DropDatabase {
            name,
            if_exists: if_exists.is_some(),
        },
    ))
}

/// SHOW DATABASES
fn parse_show_databases(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DATABASES")(input)?;
    Ok((input, AdminCommand::ShowDatabases))
}

/// SHOW DEFAULT DATABASE
fn parse_show_default_database(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DEFAULT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DATABASE")(input)?;
    Ok((input, AdminCommand::ShowDefaultDatabase))
}

/// START DATABASE name
fn parse_start_database(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("START")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DATABASE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier_or_quoted(input)?;

    Ok((input, AdminCommand::StartDatabase { name }))
}

/// STOP DATABASE name
fn parse_stop_database(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("STOP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("DATABASE")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = identifier_or_quoted(input)?;

    Ok((input, AdminCommand::StopDatabase { name }))
}

// ============================================================
// 프로시저 관리 명령어
// ============================================================

/// SHOW PROCEDURES
fn parse_show_procedures(input: &str) -> IResult<&str, AdminCommand> {
    let (input, _) = tag_no_case("SHOW")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("PROCEDURES")(input)?;
    Ok((input, AdminCommand::ShowProcedures))
}

// ============================================================
// 통합 파서
// ============================================================

/// 관리 명령 파서
///
/// 모든 관리 명령을 파싱합니다.
/// CREATE/DROP/ALTER USER, CREATE/DROP DATABASE, SHOW 등
pub fn parse_admin_command(input: &str) -> IResult<&str, AdminCommand> {
    // nom::alt는 최대 21개까지만 지원하므로 중첩 alt 사용
    alt((
        // 사용자 관리 (순서 중요: 더 구체적인 것 먼저)
        parse_create_user,
        parse_drop_user,
        parse_alter_user,
        parse_show_current_user, // SHOW CURRENT USER before SHOW USERS
        parse_show_users,
        // 데이터베이스 관리
        parse_create_database,
        parse_drop_database,
        parse_show_default_database, // SHOW DEFAULT DATABASE before SHOW DATABASES
        parse_show_databases,
        // 나머지 명령 (중첩 alt)
        alt((
            parse_start_database,
            parse_stop_database,
            // 프로시저 관리
            parse_show_procedures,
        )),
    ))(input)
}
