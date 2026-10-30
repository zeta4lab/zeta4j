//! Cypher 문장 분할 유틸리티
//!
//! 세미콜론으로 구분된 여러 Cypher 문장을 분할합니다.
//! 문자열 리터럴 내부의 세미콜론은 무시합니다.
//!
//! # 사용법
//!
//! ```ignore
//! use crate::parser::split_statements;
//!
//! let input = "CREATE (a:Test); MATCH (n) RETURN n";
//! let statements = split_statements(input);
//! assert_eq!(statements.len(), 2);
//! assert_eq!(statements[0], "CREATE (a:Test)");
//! assert_eq!(statements[1], "MATCH (n) RETURN n");
//! ```
//!
//! # 문자열 리터럴 처리
//!
//! 문자열 리터럴 내부의 세미콜론은 분할하지 않습니다:
//!
//! ```ignore
//! let input = "CREATE (n {text: 'hello; world'})";
//! let statements = split_statements(input);
//! assert_eq!(statements.len(), 1);
//! ```

/// 세미콜론으로 Cypher 문장 분할
///
/// 문자열 리터럴(`'...'` 또는 `"..."`) 내부의 세미콜론은 분할하지 않습니다.
/// 이스케이프된 따옴표(`\'`, `\"`)도 올바르게 처리합니다.
///
/// # 인자
///
/// * `input` - 분할할 Cypher 쿼리 문자열
///
/// # 반환값
///
/// 분할된 문장들의 벡터 (빈 문장 제외)
///
/// # 예제
///
/// ```ignore
/// let stmts = split_statements("CREATE (n:Patient); MATCH (n) RETURN n");
/// assert_eq!(stmts, vec!["CREATE (n:Patient)", "MATCH (n) RETURN n"]);
/// ```
pub fn split_statements(input: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut current_start = 0;
    let mut in_string = false;
    let mut string_char = '"';
    let mut prev_char = '\0';

    let chars: Vec<char> = input.chars().collect();
    let mut byte_pos = 0;

    for &c in chars.iter() {
        match c {
            '"' | '\'' if !in_string => {
                in_string = true;
                string_char = c;
            }
            c if c == string_char && in_string => {
                // 이스케이프된 따옴표인지 확인
                if prev_char != '\\' {
                    in_string = false;
                }
            }
            ';' if !in_string => {
                let stmt = input[current_start..byte_pos].trim();
                if !stmt.is_empty() {
                    statements.push(stmt);
                }
                current_start = byte_pos + c.len_utf8();
            }
            _ => {}
        }
        prev_char = c;
        byte_pos += c.len_utf8();
    }

    // 마지막 문장
    let last = input[current_start..].trim();
    if !last.is_empty() {
        statements.push(last);
    }

    statements
}

/// 세미콜론 분할 결과를 반환 (소유권 이전 버전)
///
/// `split_statements`와 동일하지만 `String` 벡터를 반환합니다.
pub fn split_statements_owned(input: &str) -> Vec<String> {
    split_statements(input)
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}
