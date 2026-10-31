// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Request Tracing
//!
//! Phase 2 Step 5 M5.3: 요청 추적
//!
//! W3C Trace Context 호환 분산 추적 시스템
//! - Trace ID / Span ID 생성
//! - HTTP 헤더 추출/주입
//! - 추적 컨텍스트 전파

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Trace ID (128-bit, W3C Trace Context 호환)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(u128);

impl TraceId {
    /// 새 Trace ID 생성
    pub fn new() -> Self {
        let mut rng = rand::rng();
        Self(rng.random())
    }

    /// 값으로 생성
    pub fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// 16진수 문자열에서 파싱
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let value =
            u128::from_str_radix(s, 16).map_err(|_| ParseError::InvalidTraceId(s.to_string()))?;
        Ok(Self(value))
    }

    /// u128 값 조회
    pub fn value(&self) -> u128 {
        self.0
    }

    /// 16진수 문자열 (32자리, 0-padded)
    pub fn to_hex(&self) -> String {
        format!("{:032x}", self.0)
    }

    /// 짧은 형식 (16자리)
    pub fn short(&self) -> String {
        let hex = self.to_hex();
        hex[..16].to_string()
    }

    /// 유효한 Trace ID인지 확인 (0이 아님)
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// Span ID (64-bit)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId(u64);

impl SpanId {
    /// 새 Span ID 생성
    pub fn new() -> Self {
        let mut rng = rand::rng();
        Self(rng.random())
    }

    /// 값으로 생성
    pub fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// 16진수 문자열에서 파싱
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let value =
            u64::from_str_radix(s, 16).map_err(|_| ParseError::InvalidSpanId(s.to_string()))?;
        Ok(Self(value))
    }

    /// u64 값 조회
    pub fn value(&self) -> u64 {
        self.0
    }

    /// 16진수 문자열 (16자리, 0-padded)
    pub fn to_hex(&self) -> String {
        format!("{:016x}", self.0)
    }

    /// 유효한 Span ID인지 확인 (0이 아님)
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SpanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// 파싱 에러
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Invalid trace ID: {0}")]
    InvalidTraceId(String),

    #[error("Invalid span ID: {0}")]
    InvalidSpanId(String),

    #[error("Invalid traceparent header: {0}")]
    InvalidTraceParent(String),

    #[error("Missing header: {0}")]
    MissingHeader(String),
}

/// 추적 로그 심각도
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceSeverity {
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for TraceSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceSeverity::Debug => write!(f, "DEBUG"),
            TraceSeverity::Info => write!(f, "INFO"),
            TraceSeverity::Warn => write!(f, "WARN"),
            TraceSeverity::Error => write!(f, "ERROR"),
        }
    }
}

/// 추적 로그
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceLog {
    /// 타임스탬프 (Unix ms)
    pub timestamp_ms: u64,
    /// 심각도
    pub severity: TraceSeverity,
    /// 메시지
    pub message: String,
    /// 추가 속성
    pub attributes: HashMap<String, String>,
}

impl TraceLog {
    /// 새 로그 생성
    pub fn new(severity: TraceSeverity, message: &str) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            timestamp_ms,
            severity,
            message: message.to_string(),
            attributes: HashMap::new(),
        }
    }

    /// 속성 추가
    pub fn with_attribute(mut self, key: &str, value: &str) -> Self {
        self.attributes.insert(key.to_string(), value.to_string());
        self
    }

    /// Debug 로그
    pub fn debug(message: &str) -> Self {
        Self::new(TraceSeverity::Debug, message)
    }

    /// Info 로그
    pub fn info(message: &str) -> Self {
        Self::new(TraceSeverity::Info, message)
    }

    /// Warn 로그
    pub fn warn(message: &str) -> Self {
        Self::new(TraceSeverity::Warn, message)
    }

    /// Error 로그
    pub fn error(message: &str) -> Self {
        Self::new(TraceSeverity::Error, message)
    }
}

/// 추적 플래그
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TraceFlags(u8);

impl TraceFlags {
    /// 샘플링됨
    pub const SAMPLED: Self = Self(0x01);

    /// 빈 플래그
    pub fn empty() -> Self {
        Self(0)
    }

    /// 샘플링 여부
    pub fn is_sampled(&self) -> bool {
        self.0 & 0x01 != 0
    }

    /// 샘플링 설정
    pub fn set_sampled(&mut self, sampled: bool) {
        if sampled {
            self.0 |= 0x01;
        } else {
            self.0 &= !0x01;
        }
    }

    /// 값 조회
    pub fn value(&self) -> u8 {
        self.0
    }

    /// 16진수 문자열
    pub fn to_hex(&self) -> String {
        format!("{:02x}", self.0)
    }

    /// 파싱
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let value = u8::from_str_radix(s, 16)
            .map_err(|_| ParseError::InvalidTraceParent(format!("invalid flags: {}", s)))?;
        Ok(Self(value))
    }
}

/// 추적 컨텍스트
///
/// W3C Trace Context 호환
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// Trace ID
    trace_id: TraceId,
    /// 현재 Span ID
    span_id: SpanId,
    /// Parent Span ID
    parent_span_id: Option<SpanId>,
    /// 추적 플래그
    flags: TraceFlags,
    /// 시작 시간
    started_at: Instant,
    /// 종료 시간
    ended_at: Option<Instant>,
    /// 작업 이름
    operation: Option<String>,
    /// 태그
    tags: HashMap<String, String>,
    /// 로그
    logs: Vec<TraceLog>,
}

impl TraceContext {
    /// 새 추적 컨텍스트 생성 (루트 스팬)
    pub fn new() -> Self {
        Self {
            trace_id: TraceId::new(),
            span_id: SpanId::new(),
            parent_span_id: None,
            flags: TraceFlags::SAMPLED,
            started_at: Instant::now(),
            ended_at: None,
            operation: None,
            tags: HashMap::new(),
            logs: Vec::new(),
        }
    }

    /// 작업 이름으로 생성
    pub fn with_operation(operation: &str) -> Self {
        let mut ctx = Self::new();
        ctx.operation = Some(operation.to_string());
        ctx
    }

    /// 자식 스팬 생성
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id,
            span_id: SpanId::new(),
            parent_span_id: Some(self.span_id),
            flags: self.flags,
            started_at: Instant::now(),
            ended_at: None,
            operation: None,
            tags: HashMap::new(),
            logs: Vec::new(),
        }
    }

    /// 작업 이름으로 자식 스팬 생성
    pub fn child_with_operation(&self, operation: &str) -> Self {
        let mut child = self.child();
        child.operation = Some(operation.to_string());
        child
    }

    /// Trace ID
    pub fn trace_id(&self) -> TraceId {
        self.trace_id
    }

    /// Span ID
    pub fn span_id(&self) -> SpanId {
        self.span_id
    }

    /// Parent Span ID
    pub fn parent_span_id(&self) -> Option<SpanId> {
        self.parent_span_id
    }

    /// 플래그
    pub fn flags(&self) -> TraceFlags {
        self.flags
    }

    /// 샘플링 여부
    pub fn is_sampled(&self) -> bool {
        self.flags.is_sampled()
    }

    /// 작업 이름
    pub fn operation(&self) -> Option<&str> {
        self.operation.as_deref()
    }

    /// 작업 이름 설정
    pub fn set_operation(&mut self, operation: &str) {
        self.operation = Some(operation.to_string());
    }

    /// 태그 설정
    pub fn set_tag(&mut self, key: &str, value: &str) {
        self.tags.insert(key.to_string(), value.to_string());
    }

    /// 태그 조회
    pub fn get_tag(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(|s| s.as_str())
    }

    /// 모든 태그
    pub fn tags(&self) -> &HashMap<String, String> {
        &self.tags
    }

    /// 로그 추가
    pub fn log(&mut self, log: TraceLog) {
        self.logs.push(log);
    }

    /// Debug 로그 추가
    pub fn log_debug(&mut self, message: &str) {
        self.logs.push(TraceLog::debug(message));
    }

    /// Info 로그 추가
    pub fn log_info(&mut self, message: &str) {
        self.logs.push(TraceLog::info(message));
    }

    /// Error 로그 추가
    pub fn log_error(&mut self, message: &str) {
        self.logs.push(TraceLog::error(message));
    }

    /// 로그 조회
    pub fn logs(&self) -> &[TraceLog] {
        &self.logs
    }

    /// 스팬 종료
    pub fn finish(&mut self) {
        self.ended_at = Some(Instant::now());
    }

    /// 종료 여부
    pub fn is_finished(&self) -> bool {
        self.ended_at.is_some()
    }

    /// 경과 시간
    pub fn elapsed(&self) -> Duration {
        if let Some(ended) = self.ended_at {
            ended.duration_since(self.started_at)
        } else {
            self.started_at.elapsed()
        }
    }

    /// traceparent 헤더 생성 (W3C Trace Context)
    /// 형식: {version}-{trace-id}-{span-id}-{flags}
    pub fn to_traceparent(&self) -> String {
        format!(
            "00-{}-{}-{}",
            self.trace_id.to_hex(),
            self.span_id.to_hex(),
            self.flags.to_hex()
        )
    }

    /// traceparent 헤더에서 파싱
    pub fn from_traceparent(header: &str) -> Result<Self, ParseError> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return Err(ParseError::InvalidTraceParent(header.to_string()));
        }

        let _version = parts[0]; // 현재 "00"만 지원
        let trace_id = TraceId::parse(parts[1])?;
        let span_id = SpanId::parse(parts[2])?;
        let flags = TraceFlags::parse(parts[3])?;

        Ok(Self {
            trace_id,
            span_id,
            parent_span_id: None,
            flags,
            started_at: Instant::now(),
            ended_at: None,
            operation: None,
            tags: HashMap::new(),
            logs: Vec::new(),
        })
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

/// 요청 추적기
///
/// 분산 추적을 위한 Trace ID 및 Span ID 관리
#[derive(Debug, Default)]
pub struct RequestTracer {
    /// 활성 추적 수
    active_traces: AtomicU64,
    /// 총 추적 수
    total_traces: AtomicU64,
}

impl RequestTracer {
    /// 새 추적기 생성
    pub fn new() -> Self {
        Self {
            active_traces: AtomicU64::new(0),
            total_traces: AtomicU64::new(0),
        }
    }

    /// 새 추적 시작
    pub fn start_trace(&self) -> TraceContext {
        self.active_traces.fetch_add(1, Ordering::Relaxed);
        self.total_traces.fetch_add(1, Ordering::Relaxed);
        TraceContext::new()
    }

    /// 작업 이름으로 추적 시작
    pub fn start_trace_with_operation(&self, operation: &str) -> TraceContext {
        self.active_traces.fetch_add(1, Ordering::Relaxed);
        self.total_traces.fetch_add(1, Ordering::Relaxed);
        TraceContext::with_operation(operation)
    }

    /// 자식 스팬 시작
    pub fn start_span(&self, parent: &TraceContext) -> TraceContext {
        parent.child()
    }

    /// 작업 이름으로 자식 스팬 시작
    pub fn start_span_with_operation(
        &self,
        parent: &TraceContext,
        operation: &str,
    ) -> TraceContext {
        parent.child_with_operation(operation)
    }

    /// 추적 종료
    pub fn finish_trace(&self, ctx: &mut TraceContext) {
        ctx.finish();
        self.active_traces.fetch_sub(1, Ordering::Relaxed);
    }

    /// HTTP 헤더에서 추적 컨텍스트 추출
    ///
    /// W3C Trace Context 헤더:
    /// - traceparent: 필수
    /// - tracestate: 선택 (벤더별 상태)
    pub fn extract_from_headers(&self, headers: &HashMap<String, String>) -> Option<TraceContext> {
        let traceparent = headers
            .get("traceparent")
            .or_else(|| headers.get("Traceparent"))
            .or_else(|| headers.get("TRACEPARENT"))?;

        TraceContext::from_traceparent(traceparent).ok()
    }

    /// 추적 컨텍스트를 HTTP 헤더에 주입
    pub fn inject_to_headers(&self, ctx: &TraceContext, headers: &mut HashMap<String, String>) {
        headers.insert("traceparent".to_string(), ctx.to_traceparent());
    }

    /// 활성 추적 수
    pub fn active_traces(&self) -> u64 {
        self.active_traces.load(Ordering::Relaxed)
    }

    /// 총 추적 수
    pub fn total_traces(&self) -> u64 {
        self.total_traces.load(Ordering::Relaxed)
    }
}
