// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Rustyline integration for the Cypher shell.
//!
//! 대화형 셸을 위한 Readline 래퍼를 제공합니다.
//!
//! # 기능
//!
//! - 방향키 편집
//! - Ctrl+R 히스토리 검색
//! - Tab 자동 완성
//! - 구문 하이라이팅 (선택적)

use std::borrow::Cow;
use std::path::PathBuf;

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::{
    MatchingBracketValidator, ValidationContext, ValidationResult, Validator,
};
use rustyline::{
    Cmd, CompletionType, Config, EditMode, Editor, EventHandler, KeyCode, KeyEvent, Modifiers,
};

use super::completion::{CompletionEngine, SchemaInfo};

// =============================================================================
// CypherHelper - Rustyline Helper 구현
// =============================================================================

/// Cypher Shell Helper
///
/// Rustyline의 Helper trait들을 구현하여 자동 완성, 힌트, 하이라이팅 등을 제공합니다.
pub struct CypherHelper {
    /// 자동 완성 엔진
    completion_engine: CompletionEngine,
    /// 히스토리 힌트
    hinter: HistoryHinter,
    /// 괄호 하이라이터
    highlighter: MatchingBracketHighlighter,
    /// 괄호 검증기
    validator: MatchingBracketValidator,
    /// 컬러 출력 활성화
    colored: bool,
}

impl CypherHelper {
    /// 새 헬퍼 생성
    pub fn new() -> Self {
        Self {
            completion_engine: CompletionEngine::new(),
            hinter: HistoryHinter::new(),
            highlighter: MatchingBracketHighlighter::new(),
            validator: MatchingBracketValidator::new(),
            colored: true,
        }
    }

    /// 스키마 설정
    pub fn with_schema(mut self, schema: SchemaInfo) -> Self {
        self.completion_engine = self.completion_engine.with_schema(schema);
        self
    }

    /// 컬러 출력 설정
    pub fn with_colored(mut self, colored: bool) -> Self {
        self.colored = colored;
        self
    }

    /// 완성 엔진 참조
    pub fn completion_engine(&self) -> &CompletionEngine {
        &self.completion_engine
    }

    /// 완성 엔진 변경
    pub fn completion_engine_mut(&mut self) -> &mut CompletionEngine {
        &mut self.completion_engine
    }
}

impl Default for CypherHelper {
    fn default() -> Self {
        Self::new()
    }
}

impl Completer for CypherHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // 완성 엔진에서 결과 가져오기
        let completions = self.completion_engine.complete(line, pos);

        if completions.is_empty() {
            return Ok((pos, vec![]));
        }

        // 완성 시작 위치 계산
        let start_pos = find_completion_start(line, pos);

        // Pair로 변환
        let pairs: Vec<Pair> = completions
            .into_iter()
            .map(|c| {
                let display = if let Some(desc) = &c.description {
                    format!("{} [{}] {}", c.kind.icon(), c.text, desc)
                } else {
                    format!("{} {}", c.kind.icon(), c.text)
                };
                Pair {
                    display,
                    replacement: c.text,
                }
            })
            .collect();

        Ok((start_pos, pairs))
    }
}

impl Hinter for CypherHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &rustyline::Context<'_>) -> Option<String> {
        // 히스토리 기반 힌트
        self.hinter.hint(line, pos, ctx)
    }
}

impl Highlighter for CypherHelper {
    fn highlight<'l>(&self, line: &'l str, pos: usize) -> Cow<'l, str> {
        if !self.colored {
            return Cow::Borrowed(line);
        }

        // 괄호 하이라이팅
        self.highlighter.highlight(line, pos)
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        if self.colored {
            // 프롬프트 컬러링: 녹색
            Cow::Owned(format!("\x1b[1;32m{}\x1b[0m", prompt))
        } else {
            Cow::Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        if self.colored {
            // 힌트: 회색 (dim)
            Cow::Owned(format!("\x1b[90m{}\x1b[0m", hint))
        } else {
            Cow::Borrowed(hint)
        }
    }

    fn highlight_char(&self, line: &str, pos: usize, forced: bool) -> bool {
        self.highlighter.highlight_char(line, pos, forced)
    }
}

impl Validator for CypherHelper {
    fn validate(&self, ctx: &mut ValidationContext<'_>) -> rustyline::Result<ValidationResult> {
        // 괄호 검증 (멀티라인 입력 지원)
        self.validator.validate(ctx)
    }
}

impl rustyline::Helper for CypherHelper {}

// =============================================================================
// ShellEditor - 편집기 래퍼
// =============================================================================

/// 셸 편집기
///
/// Rustyline Editor를 래핑하여 Cypher Shell에 맞는 설정과 기능을 제공합니다.
pub struct ShellEditor {
    /// Rustyline 편집기
    editor: Editor<CypherHelper, DefaultHistory>,
    /// 히스토리 파일 경로
    history_path: Option<PathBuf>,
}

impl ShellEditor {
    /// 새 편집기 생성
    pub fn new() -> Result<Self, ReadlineError> {
        let config = Config::builder()
            .history_ignore_space(true)
            .completion_type(CompletionType::List)
            .edit_mode(EditMode::Emacs)
            .max_history_size(1000)?
            .build();

        let helper = CypherHelper::new();
        let mut editor = Editor::with_config(config)?;
        editor.set_helper(Some(helper));

        // 키 바인딩 설정
        setup_key_bindings(&mut editor);

        Ok(Self {
            editor,
            history_path: None,
        })
    }

    /// 빌더 패턴: 스키마 설정
    pub fn with_schema(mut self, schema: SchemaInfo) -> Self {
        if let Some(helper) = self.editor.helper_mut() {
            helper.completion_engine = CompletionEngine::new().with_schema(schema);
        }
        self
    }

    /// 빌더 패턴: 컬러 설정
    pub fn with_colored(mut self, colored: bool) -> Self {
        if let Some(helper) = self.editor.helper_mut() {
            helper.colored = colored;
        }
        self
    }

    /// 빌더 패턴: 히스토리 파일 설정
    pub fn with_history_file(mut self, path: PathBuf) -> Self {
        self.history_path = Some(path);
        self
    }

    /// 히스토리 로드
    pub fn load_history(&mut self) -> Result<(), ReadlineError> {
        if let Some(ref path) = self.history_path
            && path.exists()
        {
            self.editor.load_history(path)?;
        }
        Ok(())
    }

    /// 히스토리 저장
    pub fn save_history(&mut self) -> Result<(), ReadlineError> {
        if let Some(ref path) = self.history_path {
            // 디렉토리가 없으면 생성
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            self.editor.save_history(path)?;
        }
        Ok(())
    }

    /// 히스토리에 항목 추가
    pub fn add_history_entry(&mut self, line: &str) -> Result<bool, ReadlineError> {
        self.editor.add_history_entry(line)
    }

    /// 라인 읽기
    pub fn readline(&mut self, prompt: &str) -> Result<String, ReadlineError> {
        self.editor.readline(prompt)
    }

    /// Helper 참조
    pub fn helper(&self) -> Option<&CypherHelper> {
        self.editor.helper()
    }

    /// Helper 변경
    pub fn helper_mut(&mut self) -> Option<&mut CypherHelper> {
        self.editor.helper_mut()
    }

    /// 스키마 업데이트
    pub fn update_schema(&mut self, schema: SchemaInfo) {
        if let Some(helper) = self.editor.helper_mut() {
            helper.completion_engine = CompletionEngine::new().with_schema(schema);
        }
    }
}

// =============================================================================
// 유틸리티 함수
// =============================================================================

/// 완성 시작 위치 찾기
fn find_completion_start(line: &str, pos: usize) -> usize {
    let line = if pos < line.len() { &line[..pos] } else { line };

    // 메타 명령어 (: 로 시작) 처리
    let trimmed = line.trim_start();
    if trimmed.starts_with(':') {
        // 공백 이후의 인자 처리
        if let Some(space_idx) = line.find(' ')
            && space_idx < pos
        {
            // 공백 이후의 단어 찾기
            let after_space = &line[space_idx + 1..];
            if let Some(last_space) = after_space.rfind(' ') {
                return space_idx + 1 + last_space + 1;
            }
            return space_idx + 1;
        }
        // 공백 없음 - 메타 명령 전체가 대상
        return line.len() - trimmed.len();
    }

    // Cypher 쿼리의 마지막 단어 시작 위치 찾기
    let bytes = line.as_bytes();
    let mut start = pos;

    for i in (0..pos).rev() {
        let c = bytes[i] as char;
        // 구분자: 공백, 괄호, 콜론(라벨), 점(프로퍼티), 쉼표
        if c.is_whitespace()
            || c == '('
            || c == ')'
            || c == '['
            || c == ']'
            || c == '{'
            || c == '}'
            || c == ','
            || c == ':'
            || c == '.'
        {
            start = i + 1;
            break;
        }
        if i == 0 {
            start = 0;
        }
    }

    start
}

/// 키 바인딩 설정
fn setup_key_bindings<H: rustyline::Helper>(editor: &mut Editor<H, DefaultHistory>) {
    // Ctrl+R: 히스토리 역방향 검색 (기본 제공)
    // Tab: 완성 (기본 제공)
    // Ctrl+A: 줄 처음으로 (기본 제공)
    // Ctrl+E: 줄 끝으로 (기본 제공)
    // Ctrl+U: 줄 전체 삭제 (기본 제공)
    // Ctrl+W: 단어 삭제 (기본 제공)

    // 추가 키 바인딩
    editor.bind_sequence(
        KeyEvent(KeyCode::Tab, Modifiers::SHIFT),
        EventHandler::Simple(Cmd::CompleteBackward),
    );
}

// =============================================================================
// ReadlineError 변환
// =============================================================================

/// 셸 에러로 변환 가능한 ReadlineError 래퍼
#[derive(Debug)]
pub enum ShellReadError {
    /// 인터럽트 (Ctrl+C)
    Interrupted,
    /// EOF (Ctrl+D)
    Eof,
    /// 기타 에러
    Other(String),
}

impl From<ReadlineError> for ShellReadError {
    fn from(err: ReadlineError) -> Self {
        match err {
            ReadlineError::Interrupted => ShellReadError::Interrupted,
            ReadlineError::Eof => ShellReadError::Eof,
            other => ShellReadError::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for ShellReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellReadError::Interrupted => write!(f, "Interrupted"),
            ShellReadError::Eof => write!(f, "End of input"),
            ShellReadError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ShellReadError {}
