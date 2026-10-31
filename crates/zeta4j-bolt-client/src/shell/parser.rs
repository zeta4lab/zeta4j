// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Cypher shell command parser.
//!
//! 셸 명령 파싱

use super::commands::MetaCommand;
use super::core::ShellError;

// ============================================================================
// ParsedCommand - 파싱된 명령
// ============================================================================

/// 파싱된 명령
#[derive(Debug, Clone)]
pub enum ParsedCommand {
    /// 빈 입력
    Empty,
    /// Cypher 쿼리
    Query(String),
    /// 메타 명령
    Meta(MetaCommand),
    /// 종료
    Exit,
}

// ============================================================================
// ShellParser - 셸 파서
// ============================================================================

/// 셸 명령 파서
#[derive(Debug, Clone)]
pub struct ShellParser {
    /// 멀티라인 모드
    multiline: bool,
    /// 현재 버퍼
    buffer: String,
}

impl ShellParser {
    /// 새 파서 생성
    pub fn new() -> Self {
        Self {
            multiline: false,
            buffer: String::new(),
        }
    }

    /// 명령 파싱
    pub fn parse(&mut self, input: &str) -> Result<ParsedCommand, ShellError> {
        let input = strip_line_comment(input).trim();

        // 빈 입력과 shell script 주석
        if input.is_empty() || input.starts_with('#') {
            return Ok(ParsedCommand::Empty);
        }

        // 멀티라인 모드 처리
        if self.multiline {
            self.buffer.push(' ');
            self.buffer.push_str(input);

            // 세미콜론으로 끝나면 완료
            if input.ends_with(';') {
                let query = std::mem::take(&mut self.buffer);
                self.multiline = false;
                return Ok(ParsedCommand::Query(
                    query.trim_end_matches(';').to_string(),
                ));
            }

            return Ok(ParsedCommand::Empty);
        }

        // 메타 명령 (: 또는 / 로 시작)
        if input.starts_with(':') || input.starts_with('/') {
            return self.parse_meta_command(input);
        }

        // 종료 명령
        let lower = input.to_lowercase();
        if lower == "exit" || lower == "quit" || lower == "bye" {
            return Ok(ParsedCommand::Exit);
        }

        // Cypher 쿼리
        // 세미콜론으로 끝나면 완료
        if input.ends_with(';') {
            return Ok(ParsedCommand::Query(
                input.trim_end_matches(';').to_string(),
            ));
        }

        // 키워드로 시작하고 세미콜론이 없으면 멀티라인 모드
        if self.is_cypher_start(&lower) {
            self.multiline = true;
            self.buffer = input.to_string();
            return Ok(ParsedCommand::Empty);
        }

        // 그 외는 단일 라인 쿼리로 처리
        Ok(ParsedCommand::Query(input.to_string()))
    }

    /// 메타 명령 파싱
    fn parse_meta_command(&self, input: &str) -> Result<ParsedCommand, ShellError> {
        // 접두사 제거
        let cmd = input
            .strip_prefix(':')
            .or_else(|| input.strip_prefix('/'))
            .unwrap_or(input);

        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(ParsedCommand::Empty);
        }

        let command = parts[0].to_lowercase();
        let args: Vec<&str> = parts[1..].to_vec();

        match command.as_str() {
            // 도움말
            "help" | "?" | "h" => Ok(ParsedCommand::Meta(MetaCommand::Help)),

            // 종료
            "exit" | "quit" | "q" => Ok(ParsedCommand::Exit),

            // 시스템 정보
            "sysinfo" | "sys" | "status" => Ok(ParsedCommand::Meta(MetaCommand::Sysinfo)),

            // 스키마
            "schema" => Ok(ParsedCommand::Meta(MetaCommand::Schema)),

            // 데이터베이스 전환
            "use" => {
                if args.is_empty() {
                    return Err(ShellError::ParseError("Usage: :use <database>".to_string()));
                }
                Ok(ParsedCommand::Meta(MetaCommand::Use(args[0].to_string())))
            }

            // 히스토리
            "history" | "hist" => {
                let count = args.first().and_then(|s| s.parse().ok());
                Ok(ParsedCommand::Meta(MetaCommand::History(count)))
            }

            // 클리어
            "clear" | "cls" => Ok(ParsedCommand::Meta(MetaCommand::Clear)),

            // 설정
            "set" => {
                if args.len() < 2 {
                    return Err(ShellError::ParseError(
                        "Usage: :set <key> <value>".to_string(),
                    ));
                }
                let value = args[1..].join(" ");
                Ok(ParsedCommand::Meta(MetaCommand::Set(
                    args[0].to_string(),
                    value,
                )))
            }

            // 파라미터
            "param" | "p" => {
                if args.is_empty() {
                    Ok(ParsedCommand::Meta(MetaCommand::ParamList))
                } else if args.len() == 1 && args[0] == "clear" {
                    Ok(ParsedCommand::Meta(MetaCommand::ParamClear))
                } else if args.len() >= 2 {
                    let value = args[1..].join(" ");
                    Ok(ParsedCommand::Meta(MetaCommand::Param(
                        args[0].to_string(),
                        value,
                    )))
                } else {
                    Err(ShellError::ParseError(
                        "Usage: :param <name> <value>".to_string(),
                    ))
                }
            }

            // 포맷
            "format" | "fmt" => {
                if args.is_empty() {
                    return Err(ShellError::ParseError(
                        "Usage: :format <table|csv|json|plain>".to_string(),
                    ));
                }
                Ok(ParsedCommand::Meta(MetaCommand::Format(
                    args[0].to_string(),
                )))
            }

            // 소스 파일 실행
            "source" | "src" | "load" => {
                if args.is_empty() {
                    return Err(ShellError::ParseError("Usage: :source <file>".to_string()));
                }
                Ok(ParsedCommand::Meta(MetaCommand::Source(
                    args[0].to_string(),
                )))
            }

            // 연결
            "connect" | "conn" => {
                if args.is_empty() {
                    Ok(ParsedCommand::Meta(MetaCommand::Connect(None)))
                } else {
                    Ok(ParsedCommand::Meta(MetaCommand::Connect(Some(
                        args[0].to_string(),
                    ))))
                }
            }

            // 연결 해제
            "disconnect" | "disc" => Ok(ParsedCommand::Meta(MetaCommand::Disconnect)),

            // 버전
            "version" | "ver" => Ok(ParsedCommand::Meta(MetaCommand::Version)),

            // 데이터베이스 목록
            "databases" | "dbs" => Ok(ParsedCommand::Meta(MetaCommand::Databases)),

            // 명시적 트랜잭션
            "begin" => Ok(ParsedCommand::Meta(MetaCommand::Begin)),
            "commit" => Ok(ParsedCommand::Meta(MetaCommand::Commit)),
            "rollback" => Ok(ParsedCommand::Meta(MetaCommand::Rollback)),
            "tx-status" | "txstatus" => Ok(ParsedCommand::Meta(MetaCommand::TxStatus)),

            // 쿼리 타임아웃
            "timeout" | "to" => {
                let seconds = args.first().and_then(|s| s.parse().ok());
                Ok(ParsedCommand::Meta(MetaCommand::Timeout(seconds)))
            }

            // EXPLAIN 실행 계획
            "explain" | "exp" => {
                if args.is_empty() {
                    return Err(ShellError::ParseError(
                        "Usage: :explain <query>".to_string(),
                    ));
                }
                let query = args.join(" ").trim_end_matches(';').to_string();
                Ok(ParsedCommand::Meta(MetaCommand::Explain(query)))
            }

            // PROFILE 실행 통계
            "profile" | "prof" => {
                if args.is_empty() {
                    return Err(ShellError::ParseError(
                        "Usage: :profile <query>".to_string(),
                    ));
                }
                let query = args.join(" ").trim_end_matches(';').to_string();
                Ok(ParsedCommand::Meta(MetaCommand::Profile(query)))
            }

            // 페이지 크기 설정
            "page" | "pg" => {
                let size = args.first().and_then(|s| s.parse().ok());
                Ok(ParsedCommand::Meta(MetaCommand::Page(size)))
            }

            // 다음 페이지
            "more" | "m" => Ok(ParsedCommand::Meta(MetaCommand::More)),

            // 결과 내보내기
            "export" | "save" => {
                if args.is_empty() {
                    return Err(ShellError::ParseError("Usage: :export <file>".to_string()));
                }
                Ok(ParsedCommand::Meta(MetaCommand::Export(
                    args[0].to_string(),
                )))
            }

            // 알 수 없는 명령
            _ => Err(ShellError::ParseError(format!(
                "Unknown command: :{}",
                command
            ))),
        }
    }

    /// Cypher 시작 키워드 체크
    fn is_cypher_start(&self, input: &str) -> bool {
        let keywords = [
            "match", "create", "merge", "delete", "detach", "set", "remove", "return", "with",
            "unwind", "call", "optional", "where", "order", "skip", "limit", "union", "foreach",
            "load", "using", "start",
        ];
        keywords.iter().any(|kw| input.starts_with(kw))
    }

    /// 멀티라인 모드 여부
    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    /// 버퍼 클리어
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
        self.multiline = false;
    }

    /// 현재 버퍼
    pub fn buffer(&self) -> &str {
        &self.buffer
    }
}

/// Strip a Cypher `//` line comment without treating comment markers inside
/// string literals or quoted identifiers as comments.
fn strip_line_comment(input: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut chars = input.char_indices().peekable();

    while let Some((index, character)) = chars.next() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }

        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
            continue;
        }

        if character == '/'
            && chars
                .peek()
                .is_some_and(|(_, next_character)| *next_character == '/')
        {
            return &input[..index];
        }
    }

    input
}

impl Default for ShellParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_transaction_meta_commands() -> Result<(), ShellError> {
        let mut parser = ShellParser::new();
        assert!(matches!(
            parser.parse(":begin")?,
            ParsedCommand::Meta(MetaCommand::Begin)
        ));
        assert!(matches!(
            parser.parse(":commit")?,
            ParsedCommand::Meta(MetaCommand::Commit)
        ));
        assert!(matches!(
            parser.parse(":rollback")?,
            ParsedCommand::Meta(MetaCommand::Rollback)
        ));
        assert!(matches!(
            parser.parse(":tx-status")?,
            ParsedCommand::Meta(MetaCommand::TxStatus)
        ));
        Ok(())
    }

    #[test]
    fn ignores_comments_outside_string_literals() -> Result<(), ShellError> {
        let mut parser = ShellParser::new();
        assert!(matches!(
            parser.parse("// standalone comment")?,
            ParsedCommand::Empty
        ));
        assert!(matches!(
            parser.parse("# shell script comment")?,
            ParsedCommand::Empty
        ));
        assert!(matches!(
            parser.parse("RETURN 'https://example.test/a;b' AS value; // trailing")?,
            ParsedCommand::Query(query)
                if query == "RETURN 'https://example.test/a;b' AS value"
        ));
        assert!(matches!(
            parser.parse("RETURN 1 AS `value//alias`; // trailing")?,
            ParsedCommand::Query(query)
                if query == "RETURN 1 AS `value//alias`"
        ));
        Ok(())
    }

    #[test]
    fn accumulates_multiline_queries_around_comments() -> Result<(), ShellError> {
        let mut parser = ShellParser::new();
        assert!(matches!(
            parser.parse("CREATE (n:ScriptProbe {")?,
            ParsedCommand::Empty
        ));
        assert!(matches!(
            parser.parse("  // property comment")?,
            ParsedCommand::Empty
        ));
        assert!(matches!(
            parser.parse("  value: 'left;right'")?,
            ParsedCommand::Empty
        ));
        assert!(matches!(
            parser.parse("}) RETURN n.value AS value;")?,
            ParsedCommand::Query(query)
                if query
                    == "CREATE (n:ScriptProbe { value: 'left;right' }) RETURN n.value AS value"
        ));
        Ok(())
    }
}
