//! M7.3: Structured Logging System
//!
//! JSON 구조화 로깅, 동적 로그 레벨, 슬로우 쿼리 로그

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

// ============================================================================
// LogLevel - 로그 레벨
// ============================================================================

/// 로그 레벨
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(usize)]
#[derive(Default)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    #[default]
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl LogLevel {
    /// 문자열에서 변환
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Some(LogLevel::Trace),
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "warn" | "warning" => Some(LogLevel::Warn),
            "error" => Some(LogLevel::Error),
            _ => None,
        }
    }

    /// usize에서 변환
    pub fn from_usize(v: usize) -> Option<Self> {
        match v {
            0 => Some(LogLevel::Trace),
            1 => Some(LogLevel::Debug),
            2 => Some(LogLevel::Info),
            3 => Some(LogLevel::Warn),
            4 => Some(LogLevel::Error),
            _ => None,
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

// ============================================================================
// LogEntry - 로그 엔트리
// ============================================================================

/// 로그 엔트리
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// 타임스탬프
    pub timestamp: DateTime<Utc>,
    /// 로그 레벨
    pub level: LogLevel,
    /// 메시지
    pub message: String,
    /// 추가 필드
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, serde_json::Value>,
    /// 스팬 (컨텍스트)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    /// 소스 위치
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<LogSource>,
}

/// 로그 소스 위치
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSource {
    pub file: String,
    pub line: u32,
    pub module: String,
}

impl LogEntry {
    /// 새 로그 엔트리 생성
    pub fn new(level: LogLevel, message: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            level,
            message: message.into(),
            fields: HashMap::new(),
            span: None,
            source: None,
        }
    }

    /// 필드 추가
    pub fn with_field(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    /// 스팬 설정
    pub fn with_span(mut self, span: impl Into<String>) -> Self {
        self.span = Some(span.into());
        self
    }

    /// 소스 설정
    pub fn with_source(mut self, file: &str, line: u32, module: &str) -> Self {
        self.source = Some(LogSource {
            file: file.to_string(),
            line,
            module: module.to_string(),
        });
        self
    }

    /// JSON으로 변환
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// 텍스트로 변환
    pub fn to_text(&self) -> String {
        let mut s = format!(
            "{} [{}] {}",
            self.timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
            self.level,
            self.message
        );

        if let Some(ref span) = self.span {
            s.push_str(&format!(" span={}", span));
        }

        if !self.fields.is_empty() {
            for (k, v) in &self.fields {
                s.push_str(&format!(" {}={}", k, v));
            }
        }

        s
    }
}

// ============================================================================
// LogOutput - 로그 출력 트레이트
// ============================================================================

/// 로그 출력 트레이트
pub trait LogOutput: Send + Sync {
    /// 로그 쓰기
    fn write(&self, entry: &LogEntry);

    /// 플러시
    fn flush(&self) {}
}

/// 표준 출력
pub struct StdoutOutput {
    json: bool,
}

impl StdoutOutput {
    pub fn new(json: bool) -> Self {
        Self { json }
    }
}

impl LogOutput for StdoutOutput {
    fn write(&self, entry: &LogEntry) {
        if self.json {
            println!("{}", entry.to_json());
        } else {
            println!("{}", entry.to_text());
        }
    }

    fn flush(&self) {
        let _ = std::io::stdout().flush();
    }
}

/// 메모리 버퍼 출력 (테스트용)
pub struct MemoryOutput {
    entries: RwLock<Vec<LogEntry>>,
    max_entries: usize,
}

impl MemoryOutput {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            max_entries,
        }
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn clear(&self) {
        self.entries
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

impl LogOutput for MemoryOutput {
    fn write(&self, entry: &LogEntry) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.push(entry.clone());
        if entries.len() > self.max_entries {
            entries.remove(0);
        }
    }
}

// ============================================================================
// StructuredLogger - 구조화 로거
// ============================================================================

/// 구조화 로거
pub struct StructuredLogger {
    /// 현재 로그 레벨
    level: AtomicUsize,
    /// 출력 대상들
    outputs: RwLock<Vec<Arc<dyn LogOutput>>>,
    /// 기본 필드
    default_fields: RwLock<HashMap<String, serde_json::Value>>,
    /// 현재 스팬
    current_span: RwLock<Option<String>>,
}

impl StructuredLogger {
    /// 새 로거 생성
    pub fn new(level: LogLevel) -> Self {
        Self {
            level: AtomicUsize::new(level as usize),
            outputs: RwLock::new(Vec::new()),
            default_fields: RwLock::new(HashMap::new()),
            current_span: RwLock::new(None),
        }
    }

    /// 출력 추가
    pub fn add_output(&self, output: Arc<dyn LogOutput>) {
        let mut outputs = self.outputs.write().unwrap_or_else(|e| e.into_inner());
        outputs.push(output);
    }

    /// 기본 필드 추가
    pub fn add_default_field(&self, key: impl Into<String>, value: impl Into<serde_json::Value>) {
        let mut fields = self
            .default_fields
            .write()
            .unwrap_or_else(|e| e.into_inner());
        fields.insert(key.into(), value.into());
    }

    /// 로그 레벨 설정
    pub fn set_level(&self, level: LogLevel) {
        self.level.store(level as usize, Ordering::Relaxed);
    }

    /// 현재 로그 레벨 가져오기
    pub fn get_level(&self) -> LogLevel {
        LogLevel::from_usize(self.level.load(Ordering::Relaxed)).unwrap_or(LogLevel::Info)
    }

    /// 스팬 설정
    pub fn set_span(&self, span: impl Into<String>) {
        let mut current = self.current_span.write().unwrap_or_else(|e| e.into_inner());
        *current = Some(span.into());
    }

    /// 스팬 해제
    pub fn clear_span(&self) {
        let mut current = self.current_span.write().unwrap_or_else(|e| e.into_inner());
        *current = None;
    }

    /// 로그 활성화 여부
    pub fn enabled(&self, level: LogLevel) -> bool {
        level as usize >= self.level.load(Ordering::Relaxed)
    }

    /// 로그 기록
    pub fn log(&self, mut entry: LogEntry) {
        if !self.enabled(entry.level) {
            return;
        }

        // 기본 필드 추가
        let default_fields = self
            .default_fields
            .read()
            .unwrap_or_else(|e| e.into_inner());
        for (k, v) in default_fields.iter() {
            if !entry.fields.contains_key(k) {
                entry.fields.insert(k.clone(), v.clone());
            }
        }

        // 스팬 추가
        if entry.span.is_none() {
            let span = self.current_span.read().unwrap_or_else(|e| e.into_inner());
            entry.span = span.clone();
        }

        // 출력
        let outputs = self.outputs.read().unwrap_or_else(|e| e.into_inner());
        for output in outputs.iter() {
            output.write(&entry);
        }
    }

    /// TRACE 로그
    pub fn trace(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Trace, message));
    }

    /// DEBUG 로그
    pub fn debug(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Debug, message));
    }

    /// INFO 로그
    pub fn info(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Info, message));
    }

    /// WARN 로그
    pub fn warn(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Warn, message));
    }

    /// ERROR 로그
    pub fn error(&self, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Error, message));
    }

    /// 플러시
    pub fn flush(&self) {
        let outputs = self.outputs.read().unwrap_or_else(|e| e.into_inner());
        for output in outputs.iter() {
            output.flush();
        }
    }
}

impl Default for StructuredLogger {
    fn default() -> Self {
        Self::new(LogLevel::Info)
    }
}

// ============================================================================
// SlowQueryLog - 슬로우 쿼리 로그
// ============================================================================

/// 슬로우 쿼리 로그
pub struct SlowQueryLog {
    /// 임계값
    threshold: Duration,
    /// 로거
    logger: Arc<StructuredLogger>,
    /// 기록된 슬로우 쿼리들
    queries: RwLock<Vec<SlowQueryEntry>>,
    /// 최대 보관 수
    max_entries: usize,
}

/// 슬로우 쿼리 엔트리
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowQueryEntry {
    pub timestamp: DateTime<Utc>,
    pub query: String,
    pub duration_ms: u64,
    pub database: String,
    pub parameters: Option<HashMap<String, String>>,
}

impl SlowQueryLog {
    /// 새 슬로우 쿼리 로그 생성
    pub fn new(threshold: Duration, logger: Arc<StructuredLogger>) -> Self {
        Self {
            threshold,
            logger,
            queries: RwLock::new(Vec::new()),
            max_entries: 1000,
        }
    }

    /// 임계값 설정
    pub fn set_threshold(&mut self, threshold: Duration) {
        self.threshold = threshold;
    }

    /// 쿼리 기록
    pub fn record(
        &self,
        query: &str,
        duration: Duration,
        database: &str,
        parameters: Option<HashMap<String, String>>,
    ) {
        if duration < self.threshold {
            return;
        }

        let entry = SlowQueryEntry {
            timestamp: Utc::now(),
            query: query.to_string(),
            duration_ms: duration.as_millis() as u64,
            database: database.to_string(),
            parameters,
        };

        // 로그 기록
        self.logger.log(
            LogEntry::new(
                LogLevel::Warn,
                format!("Slow query: {}ms", entry.duration_ms),
            )
            .with_field("query", query)
            .with_field("database", database)
            .with_field("duration_ms", entry.duration_ms as i64),
        );

        // 엔트리 저장
        let mut queries = self.queries.write().unwrap_or_else(|e| e.into_inner());
        queries.push(entry);
        if queries.len() > self.max_entries {
            queries.remove(0);
        }
    }

    /// 최근 슬로우 쿼리들 가져오기
    pub fn recent(&self, count: usize) -> Vec<SlowQueryEntry> {
        let queries = self.queries.read().unwrap_or_else(|e| e.into_inner());
        queries.iter().rev().take(count).cloned().collect()
    }

    /// 모든 슬로우 쿼리 가져오기
    pub fn all(&self) -> Vec<SlowQueryEntry> {
        self.queries
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 클리어
    pub fn clear(&self) {
        self.queries
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

// ============================================================================
// LogSpan - 스팬 가드
// ============================================================================

/// 스팬 가드 (RAII)
pub struct LogSpan {
    logger: Arc<StructuredLogger>,
    start: Instant,
    name: String,
}

impl LogSpan {
    /// 새 스팬 시작
    pub fn new(logger: Arc<StructuredLogger>, name: impl Into<String>) -> Self {
        let name = name.into();
        logger.set_span(&name);
        logger.debug(format!("Entering span: {}", name));
        Self {
            logger,
            start: Instant::now(),
            name,
        }
    }
}

impl Drop for LogSpan {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        self.logger.debug(format!(
            "Exiting span: {} ({}ms)",
            self.name,
            duration.as_millis()
        ));
        self.logger.clear_span();
    }
}
