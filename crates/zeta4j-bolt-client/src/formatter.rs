//! Bolt shell output formatters.
//!
//! 다양한 출력 포맷 지원 (Table, CSV, JSON, Plain)

use super::shell::{QueryResult, QueryStats};

// ============================================================================
// OutputFormat - 출력 포맷 타입
// ============================================================================

/// 출력 포맷
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// 테이블 포맷
    #[default]
    Table,
    /// CSV 포맷
    Csv,
    /// JSON 포맷
    Json,
    /// 일반 텍스트
    Plain,
}

impl OutputFormat {
    /// 문자열에서 변환
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "table" | "t" => Some(OutputFormat::Table),
            "csv" => Some(OutputFormat::Csv),
            "json" | "j" => Some(OutputFormat::Json),
            "plain" | "p" | "text" => Some(OutputFormat::Plain),
            _ => None,
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Table => write!(f, "table"),
            OutputFormat::Csv => write!(f, "csv"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Plain => write!(f, "plain"),
        }
    }
}

// ============================================================================
// OutputFormatter - 출력 포맷터 트레이트
// ============================================================================

/// 출력 포맷터 트레이트
pub trait OutputFormatter: Send + Sync {
    /// 결과 포맷
    fn format(&self, result: &QueryResult) -> String;

    /// 헤더 포맷
    fn format_header(&self, columns: &[String]) -> String;

    /// 행 포맷
    fn format_row(&self, values: &[serde_json::Value]) -> String;

    /// 푸터 포맷
    fn format_footer(&self, stats: &QueryStats, row_count: usize) -> String;
}

// ============================================================================
// FormatterFactory - 포맷터 팩토리
// ============================================================================

/// 포맷터 팩토리
pub struct FormatterFactory;

impl FormatterFactory {
    /// 포맷터 생성
    pub fn create(format: OutputFormat, color: bool) -> Box<dyn OutputFormatter> {
        match format {
            OutputFormat::Table => Box::new(TableFormatter::new(color)),
            OutputFormat::Csv => Box::new(CsvFormatter::new()),
            OutputFormat::Json => Box::new(JsonFormatter::new(true)),
            OutputFormat::Plain => Box::new(PlainFormatter::new()),
        }
    }
}

// ============================================================================
// TableFormatter - 테이블 포맷터 (Unicode Box Drawing)
// ============================================================================

/// ANSI 색상 코드
mod colors {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";

    // 전경색
    pub const CYAN: &str = "\x1b[36m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const GREEN: &str = "\x1b[32m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const WHITE: &str = "\x1b[37m";

    // 밝은 색상
    pub const BRIGHT_CYAN: &str = "\x1b[96m";
    pub const BRIGHT_YELLOW: &str = "\x1b[93m";
    pub const BRIGHT_GREEN: &str = "\x1b[92m";
}

/// Unicode 박스 그리기 문자 (둥근 모서리)
mod box_chars {
    pub const TOP_LEFT: char = '╭';
    pub const TOP_RIGHT: char = '╮';
    pub const BOTTOM_LEFT: char = '╰';
    pub const BOTTOM_RIGHT: char = '╯';
    pub const HORIZONTAL: char = '─';
    pub const VERTICAL: char = '│';
    pub const TOP_T: char = '┬';
    pub const BOTTOM_T: char = '┴';
    pub const LEFT_T: char = '├';
    pub const RIGHT_T: char = '┤';
    pub const CROSS: char = '┼';
}

/// 테이블 포맷터
pub struct TableFormatter {
    color: bool,
    min_width: usize,
    max_width: usize,
}

impl TableFormatter {
    /// 새 테이블 포맷터 생성
    pub fn new(color: bool) -> Self {
        Self {
            color,
            min_width: 3,
            max_width: 60,
        }
    }

    /// 컬럼 너비 계산
    fn calculate_widths(&self, result: &QueryResult) -> Vec<usize> {
        let mut widths: Vec<usize> = result
            .columns
            .iter()
            .map(|c| c.len().max(self.min_width))
            .collect();

        for row in &result.rows {
            for (i, val) in row.values.iter().enumerate() {
                if i < widths.len() {
                    let len = self.value_display_len(val);
                    widths[i] = widths[i].max(len).min(self.max_width);
                }
            }
        }

        widths
    }

    /// 값의 표시 길이
    fn value_display_len(&self, value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Null => 4,
            serde_json::Value::Bool(b) => {
                if *b {
                    4
                } else {
                    5
                }
            }
            serde_json::Value::Number(n) => n.to_string().len(),
            serde_json::Value::String(s) => s.len(),
            serde_json::Value::Array(arr) => {
                // [elem1, elem2, ...]
                2 + arr
                    .iter()
                    .map(|v| self.value_display_len(v) + 2)
                    .sum::<usize>()
                    .saturating_sub(2)
            }
            serde_json::Value::Object(obj) => {
                // {key: value, ...}
                2 + obj
                    .iter()
                    .map(|(k, v)| k.len() + 2 + self.value_display_len(v) + 2)
                    .sum::<usize>()
                    .saturating_sub(2)
            }
        }
    }

    /// 값이 숫자인지 확인
    fn is_numeric(&self, value: &serde_json::Value) -> bool {
        matches!(value, serde_json::Value::Number(_))
    }

    /// 값을 문자열로 (색상 없이)
    fn value_to_string_plain(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "null".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => {
                let items: Vec<String> =
                    arr.iter().map(|v| self.value_to_string_plain(v)).collect();
                format!("[{}]", items.join(", "))
            }
            serde_json::Value::Object(obj) => {
                let items: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, self.value_to_string_plain(v)))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
        }
    }

    /// 값을 색상 적용된 문자열로
    fn value_to_string_colored(&self, value: &serde_json::Value) -> String {
        if !self.color {
            return self.value_to_string_plain(value);
        }

        match value {
            serde_json::Value::Null => {
                format!("{}{}{}", colors::DIM, "null", colors::RESET)
            }
            serde_json::Value::Bool(b) => {
                format!("{}{}{}", colors::MAGENTA, b, colors::RESET)
            }
            serde_json::Value::Number(n) => {
                format!("{}{}{}", colors::BRIGHT_YELLOW, n, colors::RESET)
            }
            serde_json::Value::String(s) => {
                format!("{}{}{}", colors::BRIGHT_GREEN, s, colors::RESET)
            }
            serde_json::Value::Array(arr) => {
                let items: Vec<String> = arr
                    .iter()
                    .map(|v| self.value_to_string_colored(v))
                    .collect();
                format!("{}[{}]{}", colors::WHITE, items.join(", "), colors::RESET)
            }
            serde_json::Value::Object(obj) => {
                let items: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}{}{}: {}",
                            colors::CYAN,
                            k,
                            colors::RESET,
                            self.value_to_string_colored(v)
                        )
                    })
                    .collect();
                format!("{{{{{}}}}}", items.join(", "))
            }
        }
    }

    /// 상단 선 생성 (╭───┬───╮)
    fn top_line(&self, widths: &[usize]) -> String {
        use box_chars::*;
        let border_color = self.color_code(colors::DIM);
        let reset = self.color_code(colors::RESET);

        let parts: Vec<String> = widths
            .iter()
            .map(|w| HORIZONTAL.to_string().repeat(*w + 2))
            .collect();
        format!(
            "{}{}{}{}{}",
            border_color,
            TOP_LEFT,
            parts.join(&TOP_T.to_string()),
            TOP_RIGHT,
            reset
        )
    }

    /// 구분선 생성 (├───┼───┤)
    fn middle_line(&self, widths: &[usize]) -> String {
        use box_chars::*;
        let border_color = self.color_code(colors::DIM);
        let reset = self.color_code(colors::RESET);

        let parts: Vec<String> = widths
            .iter()
            .map(|w| HORIZONTAL.to_string().repeat(*w + 2))
            .collect();
        format!(
            "{}{}{}{}{}",
            border_color,
            LEFT_T,
            parts.join(&CROSS.to_string()),
            RIGHT_T,
            reset
        )
    }

    /// 하단 선 생성 (╰───┴───╯)
    fn bottom_line(&self, widths: &[usize]) -> String {
        use box_chars::*;
        let border_color = self.color_code(colors::DIM);
        let reset = self.color_code(colors::RESET);

        let parts: Vec<String> = widths
            .iter()
            .map(|w| HORIZONTAL.to_string().repeat(*w + 2))
            .collect();
        format!(
            "{}{}{}{}{}",
            border_color,
            BOTTOM_LEFT,
            parts.join(&BOTTOM_T.to_string()),
            BOTTOM_RIGHT,
            reset
        )
    }

    /// 컬러 코드
    fn color_code<'a>(&self, code: &'a str) -> &'a str {
        if self.color { code } else { "" }
    }
}

impl OutputFormatter for TableFormatter {
    fn format(&self, result: &QueryResult) -> String {
        if result.columns.is_empty() {
            // DDL 명령: 컬럼 없이 데이터만 있는 경우
            if !result.rows.is_empty() {
                let mut output = String::new();
                for row in &result.rows {
                    for val in &row.values {
                        if let serde_json::Value::String(s) = val {
                            output.push_str(s);
                            output.push('\n');
                        } else {
                            output.push_str(&self.value_to_string_colored(val));
                            output.push('\n');
                        }
                    }
                }
                output.push_str(&self.format_footer(&result.stats, result.row_count()));
                return output;
            }
            return self.format_footer(&result.stats, result.row_count());
        }

        let widths = self.calculate_widths(result);
        let mut output = String::new();

        // 상단 선 (╭───┬───╮)
        output.push_str(&self.top_line(&widths));
        output.push('\n');

        // 헤더
        output.push_str(&self.format_header_with_widths(&result.columns, &widths));
        output.push('\n');

        // 구분선 (├───┼───┤)
        output.push_str(&self.middle_line(&widths));
        output.push('\n');

        // 데이터 행
        for row in &result.rows {
            output.push_str(&self.format_row_with_widths(&row.values, &widths));
            output.push('\n');
        }

        // 하단 선 (╰───┴───╯)
        output.push_str(&self.bottom_line(&widths));
        output.push('\n');

        // 푸터
        output.push_str(&self.format_footer(&result.stats, result.row_count()));

        output
    }

    fn format_header(&self, columns: &[String]) -> String {
        let widths: Vec<usize> = columns
            .iter()
            .map(|c| c.len().max(self.min_width))
            .collect();
        self.format_header_with_widths(columns, &widths)
    }

    fn format_row(&self, values: &[serde_json::Value]) -> String {
        let widths: Vec<usize> = values
            .iter()
            .map(|v| self.value_display_len(v).max(self.min_width))
            .collect();
        self.format_row_with_widths(values, &widths)
    }

    fn format_footer(&self, stats: &QueryStats, row_count: usize) -> String {
        let dim = self.color_code(colors::DIM);
        let cyan = self.color_code(colors::CYAN);
        let reset = self.color_code(colors::RESET);

        let mut parts = Vec::new();

        if stats.nodes_created > 0 {
            parts.push(format!(
                "{}+{} nodes{}",
                colors::GREEN,
                stats.nodes_created,
                reset
            ));
        }
        if stats.nodes_deleted > 0 {
            parts.push(format!(
                "{}-{} nodes{}",
                colors::MAGENTA,
                stats.nodes_deleted,
                reset
            ));
        }
        if stats.relationships_created > 0 {
            parts.push(format!(
                "{}+{} rels{}",
                colors::GREEN,
                stats.relationships_created,
                reset
            ));
        }
        if stats.relationships_deleted > 0 {
            parts.push(format!(
                "{}-{} rels{}",
                colors::MAGENTA,
                stats.relationships_deleted,
                reset
            ));
        }
        if stats.properties_set > 0 {
            parts.push(format!(
                "{}~{} props{}",
                colors::YELLOW,
                stats.properties_set,
                reset
            ));
        }
        if stats.labels_added > 0 {
            parts.push(format!(
                "{}+{} labels{}",
                colors::BLUE,
                stats.labels_added,
                reset
            ));
        }

        let stats_str = if parts.is_empty() {
            String::new()
        } else if self.color {
            format!("{}\n", parts.join(", "))
        } else {
            // 색상 없는 경우 기존 형식 사용
            let mut plain_parts = Vec::new();
            if stats.nodes_created > 0 {
                plain_parts.push(format!("{} nodes created", stats.nodes_created));
            }
            if stats.nodes_deleted > 0 {
                plain_parts.push(format!("{} nodes deleted", stats.nodes_deleted));
            }
            if stats.relationships_created > 0 {
                plain_parts.push(format!(
                    "{} relationships created",
                    stats.relationships_created
                ));
            }
            if stats.relationships_deleted > 0 {
                plain_parts.push(format!(
                    "{} relationships deleted",
                    stats.relationships_deleted
                ));
            }
            if stats.properties_set > 0 {
                plain_parts.push(format!("{} properties set", stats.properties_set));
            }
            if stats.labels_added > 0 {
                plain_parts.push(format!("{} labels added", stats.labels_added));
            }
            format!("{}\n", plain_parts.join(", "))
        };

        format!(
            "{}{}{} row{}{}, {}{}ms{}",
            stats_str,
            cyan,
            row_count,
            if row_count == 1 { "" } else { "s" },
            reset,
            dim,
            stats.execution_time_ms,
            reset
        )
    }
}

impl TableFormatter {
    fn format_header_with_widths(&self, columns: &[String], widths: &[usize]) -> String {
        use box_chars::VERTICAL;

        let border_color = self.color_code(colors::DIM);
        let header_color = self.color_code(colors::BRIGHT_CYAN);
        let bold = self.color_code(colors::BOLD);
        let reset = self.color_code(colors::RESET);

        let cells: Vec<String> = columns
            .iter()
            .zip(widths.iter())
            .map(|(col, width)| {
                format!(
                    " {}{}{:^width$}{} ",
                    bold,
                    header_color,
                    col,
                    reset,
                    width = width
                )
            })
            .collect();

        let separator = format!("{}{}{}", border_color, VERTICAL, reset);
        format!(
            "{}{}{}{}{}",
            border_color,
            VERTICAL,
            reset,
            cells.join(&separator),
            separator
        )
    }

    fn format_row_with_widths(&self, values: &[serde_json::Value], widths: &[usize]) -> String {
        use box_chars::VERTICAL;

        let border_color = self.color_code(colors::DIM);
        let reset = self.color_code(colors::RESET);

        let cells: Vec<String> = values
            .iter()
            .zip(widths.iter())
            .map(|(val, width)| {
                let plain_str = self.value_to_string_plain(val);
                let colored_str = self.value_to_string_colored(val);

                let plain_char_count = plain_str.chars().count();
                let truncated_plain = if plain_char_count > *width {
                    let truncated: String = plain_str.chars().take(width - 1).collect();
                    format!("{}…", truncated)
                } else {
                    plain_str.clone()
                };

                let truncated_colored = if plain_char_count > *width {
                    // 색상 적용된 문자열 잘라내기 - 문자 단위로 자름
                    let truncated: String = plain_str.chars().take(width - 1).collect();
                    format!("{}…", truncated)
                } else {
                    colored_str
                };

                // 숫자는 오른쪽 정렬, 나머지는 왼쪽 정렬
                if self.is_numeric(val) {
                    if self.color {
                        let padding = width.saturating_sub(plain_char_count);
                        format!(" {}{} ", " ".repeat(padding), truncated_colored)
                    } else {
                        format!(" {:>width$} ", truncated_plain, width = width)
                    }
                } else if self.color {
                    let padding = width.saturating_sub(plain_char_count);
                    format!(" {}{} ", truncated_colored, " ".repeat(padding))
                } else {
                    format!(" {:width$} ", truncated_plain, width = width)
                }
            })
            .collect();

        let separator = format!("{}{}{}", border_color, VERTICAL, reset);
        format!(
            "{}{}{}{}{}",
            border_color,
            VERTICAL,
            reset,
            cells.join(&separator),
            separator
        )
    }
}

// ============================================================================
// CsvFormatter - CSV 포맷터
// ============================================================================

/// CSV 포맷터
pub struct CsvFormatter {
    delimiter: char,
    quote: char,
}

impl CsvFormatter {
    /// 새 CSV 포맷터 생성
    pub fn new() -> Self {
        Self {
            delimiter: ',',
            quote: '"',
        }
    }

    /// 구분자 설정
    pub fn with_delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// 값 이스케이프
    fn escape_value(&self, value: &str) -> String {
        if value.contains(self.delimiter) || value.contains(self.quote) || value.contains('\n') {
            let escaped = value.replace(self.quote, &format!("{}{}", self.quote, self.quote));
            format!("{}{}{}", self.quote, escaped, self.quote)
        } else {
            value.to_string()
        }
    }

    /// 값을 문자열로
    fn value_to_string(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => String::new(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => serde_json::to_string(arr).unwrap_or_default(),
            serde_json::Value::Object(obj) => serde_json::to_string(obj).unwrap_or_default(),
        }
    }
}

impl Default for CsvFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputFormatter for CsvFormatter {
    fn format(&self, result: &QueryResult) -> String {
        let mut output = String::new();

        // 헤더
        output.push_str(&self.format_header(&result.columns));
        output.push('\n');

        // 데이터 행
        for row in &result.rows {
            output.push_str(&self.format_row(&row.values));
            output.push('\n');
        }

        output
    }

    fn format_header(&self, columns: &[String]) -> String {
        columns
            .iter()
            .map(|c| self.escape_value(c))
            .collect::<Vec<_>>()
            .join(&self.delimiter.to_string())
    }

    fn format_row(&self, values: &[serde_json::Value]) -> String {
        values
            .iter()
            .map(|v| self.escape_value(&self.value_to_string(v)))
            .collect::<Vec<_>>()
            .join(&self.delimiter.to_string())
    }

    fn format_footer(&self, _stats: &QueryStats, _row_count: usize) -> String {
        String::new()
    }
}

// ============================================================================
// JsonFormatter - JSON 포맷터
// ============================================================================

/// JSON 포맷터
pub struct JsonFormatter {
    pretty: bool,
}

impl JsonFormatter {
    /// 새 JSON 포맷터 생성
    pub fn new(pretty: bool) -> Self {
        Self { pretty }
    }
}

impl OutputFormatter for JsonFormatter {
    fn format(&self, result: &QueryResult) -> String {
        let rows: Vec<serde_json::Value> = result
            .rows
            .iter()
            .map(|row| {
                let mut obj = serde_json::Map::new();
                for (i, col) in result.columns.iter().enumerate() {
                    if let Some(val) = row.values.get(i) {
                        obj.insert(col.clone(), val.clone());
                    }
                }
                serde_json::Value::Object(obj)
            })
            .collect();

        if self.pretty {
            serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string())
        } else {
            serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string())
        }
    }

    fn format_header(&self, _columns: &[String]) -> String {
        String::new()
    }

    fn format_row(&self, values: &[serde_json::Value]) -> String {
        if self.pretty {
            serde_json::to_string_pretty(values).unwrap_or_else(|_| "[]".to_string())
        } else {
            serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
        }
    }

    fn format_footer(&self, _stats: &QueryStats, _row_count: usize) -> String {
        String::new()
    }
}

// ============================================================================
// PlainFormatter - 일반 텍스트 포맷터
// ============================================================================

/// 일반 텍스트 포맷터
pub struct PlainFormatter;

impl PlainFormatter {
    /// 새 일반 텍스트 포맷터 생성
    pub fn new() -> Self {
        Self
    }

    /// 값을 문자열로
    fn value_to_string(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "null".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| self.value_to_string(v)).collect();
                format!("[{}]", items.join(", "))
            }
            serde_json::Value::Object(obj) => {
                let items: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, self.value_to_string(v)))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
        }
    }
}

impl Default for PlainFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputFormatter for PlainFormatter {
    fn format(&self, result: &QueryResult) -> String {
        let mut output = String::new();

        for row in &result.rows {
            let values: Vec<String> = row.values.iter().map(|v| self.value_to_string(v)).collect();
            output.push_str(&values.join("\t"));
            output.push('\n');
        }

        output.push_str(&self.format_footer(&result.stats, result.row_count()));

        output
    }

    fn format_header(&self, columns: &[String]) -> String {
        columns.join("\t")
    }

    fn format_row(&self, values: &[serde_json::Value]) -> String {
        values
            .iter()
            .map(|v| self.value_to_string(v))
            .collect::<Vec<_>>()
            .join("\t")
    }

    fn format_footer(&self, stats: &QueryStats, row_count: usize) -> String {
        format!(
            "{} row{}, {}ms",
            row_count,
            if row_count == 1 { "" } else { "s" },
            stats.execution_time_ms
        )
    }
}

// ============================================================================
// Paginator - 페이지네이션
// ============================================================================

/// 페이지네이터
pub struct Paginator {
    page_size: usize,
    current_page: usize,
    total_rows: usize,
}

impl Paginator {
    /// 새 페이지네이터 생성
    pub fn new(page_size: usize, total_rows: usize) -> Self {
        Self {
            page_size,
            current_page: 0,
            total_rows,
        }
    }

    /// 총 페이지 수
    pub fn total_pages(&self) -> usize {
        self.total_rows.div_ceil(self.page_size)
    }

    /// 현재 페이지
    pub fn current_page(&self) -> usize {
        self.current_page
    }

    /// 다음 페이지
    pub fn next_page(&mut self) -> bool {
        if self.current_page < self.total_pages() - 1 {
            self.current_page += 1;
            true
        } else {
            false
        }
    }

    /// 이전 페이지
    pub fn prev_page(&mut self) -> bool {
        if self.current_page > 0 {
            self.current_page -= 1;
            true
        } else {
            false
        }
    }

    /// 페이지 행 범위
    pub fn page_range(&self) -> (usize, usize) {
        let start = self.current_page * self.page_size;
        let end = (start + self.page_size).min(self.total_rows);
        (start, end)
    }

    /// 첫 페이지로
    pub fn first_page(&mut self) {
        self.current_page = 0;
    }

    /// 마지막 페이지로
    pub fn last_page(&mut self) {
        self.current_page = self.total_pages().saturating_sub(1);
    }

    /// 페이지 정보 문자열
    pub fn page_info(&self) -> String {
        format!(
            "Page {}/{} ({} rows)",
            self.current_page + 1,
            self.total_pages(),
            self.total_rows
        )
    }
}
