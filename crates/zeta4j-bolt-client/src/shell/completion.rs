// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Cypher completion engine.
//!
//! Cypher 키워드, 메타 명령, 라벨/프로퍼티 자동 완성

use std::collections::HashSet;

// ============================================================================
// Cypher 키워드 정의
// ============================================================================

/// Cypher 절 키워드
pub const CYPHER_CLAUSES: &[&str] = &[
    "MATCH",
    "OPTIONAL MATCH",
    "WHERE",
    "RETURN",
    "WITH",
    "CREATE",
    "MERGE",
    "DELETE",
    "DETACH DELETE",
    "REMOVE",
    "SET",
    "ORDER BY",
    "SKIP",
    "LIMIT",
    "UNION",
    "UNION ALL",
    "UNWIND",
    "FOREACH",
    "CALL",
    "YIELD",
    "LOAD CSV",
    "USING",
    "START",
    "EXPLAIN",
    "PROFILE",
    "CREATE INDEX",
    "DROP INDEX",
    "CREATE CONSTRAINT",
    "DROP CONSTRAINT",
];

/// Cypher 연산자 및 표현식 키워드
pub const CYPHER_OPERATORS: &[&str] = &[
    "AND",
    "OR",
    "XOR",
    "NOT",
    "IN",
    "STARTS WITH",
    "ENDS WITH",
    "CONTAINS",
    "IS NULL",
    "IS NOT NULL",
    "AS",
    "DISTINCT",
    "ALL",
    "ANY",
    "NONE",
    "SINGLE",
    "TRUE",
    "FALSE",
    "NULL",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "ASCENDING",
    "DESC",
    "DESCENDING",
];

/// Cypher 집계 함수
pub const CYPHER_AGGREGATES: &[&str] = &[
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "collect",
    "stDev",
    "stDevP",
    "percentileDisc",
    "percentileCont",
];

/// Cypher 스칼라 함수
pub const CYPHER_SCALAR_FUNCTIONS: &[&str] = &[
    // 문자열 함수
    "toString",
    "toInteger",
    "toFloat",
    "toBoolean",
    "trim",
    "ltrim",
    "rtrim",
    "replace",
    "substring",
    "left",
    "right",
    "split",
    "reverse",
    "toLower",
    "toUpper",
    "size",
    "length",
    // 수학 함수
    "abs",
    "ceil",
    "floor",
    "round",
    "sign",
    "rand",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "sqrt",
    "log",
    "log10",
    "exp",
    "e",
    "pi",
    // 리스트 함수
    "head",
    "tail",
    "last",
    "range",
    "keys",
    "properties",
    "labels",
    "type",
    "nodes",
    "relationships",
    "id",
    "elementId",
    // 시간 함수
    "date",
    "datetime",
    "time",
    "localtime",
    "localdatetime",
    "duration",
    "date.truncate",
    "datetime.truncate",
    // 공간 함수
    "point",
    "distance",
    // 조건 함수
    "coalesce",
    "nullIf",
];

/// Cypher 프로시저 (CALL 용)
pub const CYPHER_PROCEDURES: &[&str] = &[
    "db.labels",
    "db.relationshipTypes",
    "db.propertyKeys",
    "db.indexes",
    "db.constraints",
    "db.schema.visualization",
    "dbms.components",
    "dbms.procedures",
    "dbms.functions",
    "dbms.showCurrentUser",
    "dbms.listTransactions",
    "apoc.meta.data",
    "apoc.help",
];

/// 메타 명령어
pub const META_COMMANDS: &[&str] = &[
    ":help",
    ":?",
    ":exit",
    ":quit",
    ":q",
    ":sysinfo",
    ":schema",
    ":use",
    ":history",
    ":clear",
    ":set",
    ":param",
    ":format",
    ":source",
    ":connect",
    ":disconnect",
    ":version",
    ":databases",
    ":begin",
    ":commit",
    ":rollback",
    ":tx-status",
    ":timeout",
    ":explain",
    ":profile",
    ":page",
    ":more",
    ":export",
];

/// 메타 명령 인자 (설정 키)
pub const META_SET_KEYS: &[&str] = &["format", "color", "page_size"];

/// 출력 포맷 값
pub const META_FORMAT_VALUES: &[&str] = &["table", "csv", "json", "plain"];

// ============================================================================
// CompletionEngine - 자동 완성 엔진
// ============================================================================

/// 자동 완성 결과
#[derive(Debug, Clone)]
pub struct CompletionResult {
    /// 완성된 텍스트
    pub text: String,
    /// 완성 종류
    pub kind: CompletionKind,
    /// 설명 (옵션)
    pub description: Option<String>,
}

/// 완성 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// Cypher 절 키워드
    Clause,
    /// Cypher 연산자/표현식
    Operator,
    /// 집계 함수
    Aggregate,
    /// 스칼라 함수
    Function,
    /// 프로시저
    Procedure,
    /// 메타 명령
    MetaCommand,
    /// 노드 라벨
    Label,
    /// 관계 타입
    RelationshipType,
    /// 프로퍼티 키
    PropertyKey,
    /// 설정 키
    SettingKey,
    /// 설정 값
    SettingValue,
}

impl CompletionKind {
    /// 아이콘 반환 (터미널 표시용)
    pub fn icon(&self) -> &'static str {
        match self {
            CompletionKind::Clause => "K",           // Keyword
            CompletionKind::Operator => "O",         // Operator
            CompletionKind::Aggregate => "A",        // Aggregate
            CompletionKind::Function => "F",         // Function
            CompletionKind::Procedure => "P",        // Procedure
            CompletionKind::MetaCommand => ":",      // Meta command
            CompletionKind::Label => "L",            // Label
            CompletionKind::RelationshipType => "R", // Relationship
            CompletionKind::PropertyKey => ".",      // Property
            CompletionKind::SettingKey => "S",       // Setting
            CompletionKind::SettingValue => "V",     // Value
        }
    }
}

/// 스키마 정보 (동적 완성용)
#[derive(Debug, Clone, Default)]
pub struct SchemaInfo {
    /// 노드 라벨 목록
    pub labels: HashSet<String>,
    /// 관계 타입 목록
    pub relationship_types: HashSet<String>,
    /// 프로퍼티 키 목록
    pub property_keys: HashSet<String>,
}

impl SchemaInfo {
    /// 새 스키마 정보 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 라벨 추가
    pub fn add_label(&mut self, label: impl Into<String>) {
        self.labels.insert(label.into());
    }

    /// 관계 타입 추가
    pub fn add_relationship_type(&mut self, rel_type: impl Into<String>) {
        self.relationship_types.insert(rel_type.into());
    }

    /// 프로퍼티 키 추가
    pub fn add_property_key(&mut self, key: impl Into<String>) {
        self.property_keys.insert(key.into());
    }

    /// Mock 데이터로 초기화
    pub fn with_mock_data() -> Self {
        let mut schema = Self::new();
        // 샘플 라벨
        schema.add_label("Person");
        schema.add_label("Movie");
        schema.add_label("Director");
        schema.add_label("Actor");
        schema.add_label("Company");
        // 샘플 관계 타입
        schema.add_relationship_type("ACTED_IN");
        schema.add_relationship_type("DIRECTED");
        schema.add_relationship_type("PRODUCED");
        schema.add_relationship_type("KNOWS");
        schema.add_relationship_type("WORKS_FOR");
        // 샘플 프로퍼티 키
        schema.add_property_key("name");
        schema.add_property_key("title");
        schema.add_property_key("age");
        schema.add_property_key("year");
        schema.add_property_key("rating");
        schema.add_property_key("born");
        schema
    }
}

/// 자동 완성 엔진
#[derive(Debug, Clone)]
pub struct CompletionEngine {
    /// 스키마 정보
    schema: SchemaInfo,
    /// 대소문자 구분 여부
    case_sensitive: bool,
    /// 최대 결과 수
    max_results: usize,
}

impl Default for CompletionEngine {
    fn default() -> Self {
        Self {
            schema: SchemaInfo::with_mock_data(),
            case_sensitive: false,
            max_results: 20,
        }
    }
}

impl CompletionEngine {
    /// 새 엔진 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 스키마 설정
    pub fn with_schema(mut self, schema: SchemaInfo) -> Self {
        self.schema = schema;
        self
    }

    /// 대소문자 구분 설정
    pub fn with_case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }

    /// 최대 결과 수 설정
    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results;
        self
    }

    /// 스키마 참조
    pub fn schema(&self) -> &SchemaInfo {
        &self.schema
    }

    /// 스키마 변경
    pub fn schema_mut(&mut self) -> &mut SchemaInfo {
        &mut self.schema
    }

    /// 자동 완성 수행
    pub fn complete(&self, input: &str, cursor_pos: usize) -> Vec<CompletionResult> {
        let input = if cursor_pos < input.len() {
            &input[..cursor_pos]
        } else {
            input
        };

        let trimmed = input.trim();

        // 빈 입력
        if trimmed.is_empty() {
            return self.complete_empty();
        }

        // 메타 명령 완성 (trailing space 유지)
        if trimmed.starts_with(':') {
            return self.complete_meta_command(input.trim_start());
        }

        // Cypher 쿼리 완성
        self.complete_cypher(input)
    }

    /// 빈 입력에 대한 완성
    fn complete_empty(&self) -> Vec<CompletionResult> {
        let mut results = Vec::new();

        // 주요 Cypher 절
        for clause in &["MATCH", "CREATE", "RETURN", "CALL"] {
            results.push(CompletionResult {
                text: clause.to_string(),
                kind: CompletionKind::Clause,
                description: Some(format!("{} clause", clause)),
            });
        }

        // 메타 명령
        for cmd in &[":help", ":schema", ":databases"] {
            results.push(CompletionResult {
                text: cmd.to_string(),
                kind: CompletionKind::MetaCommand,
                description: None,
            });
        }

        results
    }

    /// 메타 명령 완성
    fn complete_meta_command(&self, input: &str) -> Vec<CompletionResult> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("");
        let ends_with_space = input.ends_with(' ');

        // 명령어 자체 완성
        if parts.len() <= 1 && !ends_with_space {
            return self.filter_completions(
                META_COMMANDS.iter().map(|s| CompletionResult {
                    text: s.to_string(),
                    kind: CompletionKind::MetaCommand,
                    description: self.meta_command_description(s),
                }),
                cmd,
            );
        }

        // :set 명령의 인자 완성
        if cmd == ":set" {
            if parts.len() == 1 && ends_with_space {
                // :set 후 바로 공백 - 모든 설정 키 표시
                return META_SET_KEYS
                    .iter()
                    .map(|s| CompletionResult {
                        text: s.to_string(),
                        kind: CompletionKind::SettingKey,
                        description: self.setting_description(s),
                    })
                    .collect();
            } else if parts.len() == 2 && !ends_with_space {
                // 설정 키 완성
                let key_prefix = parts.get(1).copied().unwrap_or("");
                return self.filter_completions(
                    META_SET_KEYS.iter().map(|s| CompletionResult {
                        text: s.to_string(),
                        kind: CompletionKind::SettingKey,
                        description: self.setting_description(s),
                    }),
                    key_prefix,
                );
            } else if (parts.len() == 2 && ends_with_space) || parts.len() == 3 {
                // format 값 완성
                if parts.get(1) == Some(&"format") {
                    let value_prefix = if ends_with_space && parts.len() == 2 {
                        ""
                    } else {
                        parts.get(2).copied().unwrap_or("")
                    };
                    return self.filter_completions(
                        META_FORMAT_VALUES.iter().map(|s| CompletionResult {
                            text: s.to_string(),
                            kind: CompletionKind::SettingValue,
                            description: None,
                        }),
                        value_prefix,
                    );
                }
            }
        }

        // :format 명령의 인자 완성
        if cmd == ":format" {
            if parts.len() == 1 && ends_with_space {
                // :format 후 바로 공백
                return META_FORMAT_VALUES
                    .iter()
                    .map(|s| CompletionResult {
                        text: s.to_string(),
                        kind: CompletionKind::SettingValue,
                        description: None,
                    })
                    .collect();
            } else if parts.len() == 2 && !ends_with_space {
                let value_prefix = parts.get(1).copied().unwrap_or("");
                return self.filter_completions(
                    META_FORMAT_VALUES.iter().map(|s| CompletionResult {
                        text: s.to_string(),
                        kind: CompletionKind::SettingValue,
                        description: None,
                    }),
                    value_prefix,
                );
            }
        }

        Vec::new()
    }

    /// Cypher 쿼리 완성
    fn complete_cypher(&self, input: &str) -> Vec<CompletionResult> {
        let context = self.analyze_context(input);
        let current_word = self.get_current_word(input);

        match context {
            CompletionContext::AfterColon => self.complete_labels(&current_word),
            CompletionContext::AfterOpenBracket => self.complete_relationship_types(&current_word),
            CompletionContext::AfterDot => self.complete_properties(&current_word),
            CompletionContext::AfterCall => self.complete_procedures(&current_word),
            CompletionContext::InFunction => self.complete_functions(&current_word),
            CompletionContext::General => self.complete_general(&current_word),
        }
    }

    /// 컨텍스트 분석
    fn analyze_context(&self, input: &str) -> CompletionContext {
        let trimmed = input.trim_end();
        let upper = trimmed.to_uppercase();

        // CALL 직후 또는 CALL 로 시작하는 단어 입력 중
        if upper.ends_with("CALL") || upper.ends_with("CALL ") {
            return CompletionContext::AfterCall;
        }
        // CALL 이후 프로시저 이름 입력 중
        if let Some(call_pos) = upper.rfind("CALL ") {
            let after_call = &trimmed[call_pos + 5..];
            if !after_call.is_empty()
                && after_call
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '.' || c == '_')
            {
                return CompletionContext::AfterCall;
            }
        }

        // 마지막 문자 기준
        if let Some(last_char) = trimmed.chars().last() {
            match last_char {
                ':' => return CompletionContext::AfterColon,
                '[' => return CompletionContext::AfterOpenBracket,
                '.' => return CompletionContext::AfterDot,
                '(' => return CompletionContext::InFunction,
                _ => {}
            }
        }

        // 대괄호 안에서 관계 타입 입력 중 (닫는 괄호 전)
        if let Some(bracket_pos) = trimmed.rfind('[') {
            let after_bracket = &trimmed[bracket_pos + 1..];
            if !after_bracket.contains(']') {
                // 콜론 찾기 (관계 타입 시작)
                if let Some(colon_pos) = after_bracket.find(':') {
                    let rel_part = &after_bracket[colon_pos + 1..];
                    if rel_part.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        return CompletionContext::AfterOpenBracket;
                    }
                } else {
                    // 콜론 없이 변수명만 있거나 빈 경우
                    return CompletionContext::AfterOpenBracket;
                }
            }
        }

        // 콜론 뒤 라벨 입력 중 (괄호 안에서)
        if let Some(paren_pos) = trimmed.rfind('(') {
            let after_paren = &trimmed[paren_pos + 1..];
            if !after_paren.contains(')') {
                // 괄호 안에서 콜론 찾기
                if let Some(colon_pos) = after_paren.rfind(':') {
                    let after_colon = &after_paren[colon_pos + 1..];
                    if after_colon.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        return CompletionContext::AfterColon;
                    }
                }
            }
        }

        CompletionContext::General
    }

    /// 현재 단어 추출
    fn get_current_word(&self, input: &str) -> String {
        let trimmed = input.trim_end();

        // 마지막 단어 찾기
        let word_start = trimmed
            .rfind(|c: char| c.is_whitespace() || "()[]{}:.,;".contains(c))
            .map(|i| i + 1)
            .unwrap_or(0);

        trimmed[word_start..].to_string()
    }

    /// 라벨 완성
    fn complete_labels(&self, prefix: &str) -> Vec<CompletionResult> {
        self.filter_completions(
            self.schema.labels.iter().map(|s| CompletionResult {
                text: s.clone(),
                kind: CompletionKind::Label,
                description: Some("Node label".to_string()),
            }),
            prefix,
        )
    }

    /// 관계 타입 완성
    fn complete_relationship_types(&self, prefix: &str) -> Vec<CompletionResult> {
        self.filter_completions(
            self.schema
                .relationship_types
                .iter()
                .map(|s| CompletionResult {
                    text: s.clone(),
                    kind: CompletionKind::RelationshipType,
                    description: Some("Relationship type".to_string()),
                }),
            prefix,
        )
    }

    /// 프로퍼티 완성
    fn complete_properties(&self, prefix: &str) -> Vec<CompletionResult> {
        self.filter_completions(
            self.schema.property_keys.iter().map(|s| CompletionResult {
                text: s.clone(),
                kind: CompletionKind::PropertyKey,
                description: Some("Property key".to_string()),
            }),
            prefix,
        )
    }

    /// 프로시저 완성
    fn complete_procedures(&self, prefix: &str) -> Vec<CompletionResult> {
        self.filter_completions(
            CYPHER_PROCEDURES.iter().map(|s| CompletionResult {
                text: s.to_string(),
                kind: CompletionKind::Procedure,
                description: Some("Procedure".to_string()),
            }),
            prefix,
        )
    }

    /// 함수 완성
    fn complete_functions(&self, prefix: &str) -> Vec<CompletionResult> {
        let mut results = Vec::new();

        // 집계 함수
        for func in CYPHER_AGGREGATES {
            results.push(CompletionResult {
                text: format!("{}()", func),
                kind: CompletionKind::Aggregate,
                description: Some("Aggregate function".to_string()),
            });
        }

        // 스칼라 함수
        for func in CYPHER_SCALAR_FUNCTIONS {
            results.push(CompletionResult {
                text: format!("{}()", func),
                kind: CompletionKind::Function,
                description: Some("Scalar function".to_string()),
            });
        }

        self.filter_completions(results.into_iter(), prefix)
    }

    /// 일반 완성 (키워드, 함수 등)
    fn complete_general(&self, prefix: &str) -> Vec<CompletionResult> {
        let mut results = Vec::new();

        // Cypher 절
        for clause in CYPHER_CLAUSES {
            results.push(CompletionResult {
                text: clause.to_string(),
                kind: CompletionKind::Clause,
                description: Some(format!("{} clause", clause)),
            });
        }

        // 연산자
        for op in CYPHER_OPERATORS {
            results.push(CompletionResult {
                text: op.to_string(),
                kind: CompletionKind::Operator,
                description: Some("Operator".to_string()),
            });
        }

        // 집계 함수
        for func in CYPHER_AGGREGATES {
            results.push(CompletionResult {
                text: format!("{}()", func),
                kind: CompletionKind::Aggregate,
                description: Some("Aggregate function".to_string()),
            });
        }

        // 일부 주요 스칼라 함수
        for func in &[
            "id",
            "labels",
            "type",
            "properties",
            "keys",
            "size",
            "length",
        ] {
            results.push(CompletionResult {
                text: format!("{}()", func),
                kind: CompletionKind::Function,
                description: Some("Scalar function".to_string()),
            });
        }

        self.filter_completions(results.into_iter(), prefix)
    }

    /// 접두사로 필터링
    fn filter_completions<I>(&self, iter: I, prefix: &str) -> Vec<CompletionResult>
    where
        I: Iterator<Item = CompletionResult>,
    {
        let prefix_lower = prefix.to_lowercase();

        iter.filter(|r| {
            if self.case_sensitive {
                r.text.starts_with(prefix)
            } else {
                r.text.to_lowercase().starts_with(&prefix_lower)
            }
        })
        .take(self.max_results)
        .collect()
    }

    /// 메타 명령 설명
    fn meta_command_description(&self, cmd: &str) -> Option<String> {
        match cmd {
            ":help" | ":?" => Some("Show help".to_string()),
            ":exit" | ":quit" | ":q" => Some("Exit shell".to_string()),
            ":sysinfo" => Some("System information".to_string()),
            ":schema" => Some("Database schema".to_string()),
            ":use" => Some("Switch database".to_string()),
            ":history" => Some("Command history".to_string()),
            ":clear" => Some("Clear screen".to_string()),
            ":set" => Some("Set option".to_string()),
            ":param" => Some("Set parameter".to_string()),
            ":format" => Some("Output format".to_string()),
            ":source" => Some("Run file".to_string()),
            ":connect" => Some("Connect to server".to_string()),
            ":disconnect" => Some("Disconnect".to_string()),
            ":version" => Some("Version info".to_string()),
            ":databases" => Some("List databases".to_string()),
            _ => None,
        }
    }

    /// 설정 키 설명
    fn setting_description(&self, key: &str) -> Option<String> {
        match key {
            "format" => Some("Output format (table, csv, json, plain)".to_string()),
            "color" => Some("Color output (true/false)".to_string()),
            "page_size" => Some("Page size (0 = unlimited)".to_string()),
            _ => None,
        }
    }
}

/// 완성 컨텍스트
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionContext {
    /// 콜론 뒤 (라벨)
    AfterColon,
    /// 대괄호 안 (관계 타입)
    AfterOpenBracket,
    /// 점 뒤 (프로퍼티)
    AfterDot,
    /// CALL 뒤 (프로시저)
    AfterCall,
    /// 함수 괄호 안
    InFunction,
    /// 일반 (키워드, 함수 등)
    General,
}

// ============================================================================
// CompletionFormatter - 완성 결과 포맷터
// ============================================================================

/// 완성 결과 포맷터
pub struct CompletionFormatter {
    /// 컬러 출력 여부
    color: bool,
    /// 설명 표시 여부
    show_description: bool,
    /// 아이콘 표시 여부
    show_icon: bool,
}

impl Default for CompletionFormatter {
    fn default() -> Self {
        Self {
            color: true,
            show_description: true,
            show_icon: true,
        }
    }
}

impl CompletionFormatter {
    /// 새 포맷터 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 컬러 설정
    pub fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    /// 설명 표시 설정
    pub fn with_description(mut self, show: bool) -> Self {
        self.show_description = show;
        self
    }

    /// 아이콘 표시 설정
    pub fn with_icon(mut self, show: bool) -> Self {
        self.show_icon = show;
        self
    }

    /// 완성 결과 포맷
    pub fn format(&self, result: &CompletionResult) -> String {
        let mut output = String::new();

        if self.show_icon {
            if self.color {
                output.push_str(&format!("\x1b[90m[{}]\x1b[0m ", result.kind.icon()));
            } else {
                output.push_str(&format!("[{}] ", result.kind.icon()));
            }
        }

        if self.color {
            let color = match result.kind {
                CompletionKind::Clause | CompletionKind::Operator => "\x1b[1;34m", // Bold Blue
                CompletionKind::Aggregate | CompletionKind::Function => "\x1b[33m", // Yellow
                CompletionKind::Procedure => "\x1b[35m",                           // Magenta
                CompletionKind::MetaCommand => "\x1b[32m",                         // Green
                CompletionKind::Label | CompletionKind::RelationshipType => "\x1b[36m", // Cyan
                CompletionKind::PropertyKey => "\x1b[37m",                         // White
                CompletionKind::SettingKey | CompletionKind::SettingValue => "\x1b[33m", // Yellow
            };
            output.push_str(&format!("{}{}\x1b[0m", color, result.text));
        } else {
            output.push_str(&result.text);
        }

        if self.show_description
            && let Some(ref desc) = result.description
        {
            if self.color {
                output.push_str(&format!("  \x1b[90m{}\x1b[0m", desc));
            } else {
                output.push_str(&format!("  {}", desc));
            }
        }

        output
    }

    /// 완성 목록 포맷
    pub fn format_list(&self, results: &[CompletionResult]) -> String {
        results
            .iter()
            .map(|r| self.format(r))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
