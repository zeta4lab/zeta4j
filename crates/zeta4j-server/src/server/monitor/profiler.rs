// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Server Profiler Module
//!
//! 서버 성능 프로파일링 및 병목 분석 도구
//!
//! ## 기능
//! - CPU 프로파일링 (함수별 실행 시간)
//! - 메모리 프로파일링 (할당/해제 추적)
//! - I/O 프로파일링 (읽기/쓰기 처리량)
//! - 쿼리 프로파일링 (쿼리별 실행 시간)
//! - 연결 프로파일링 (연결별 통계)
//! - 병목 탐지 및 리포트

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

// ============================================================================
// ProfilerConfig - 프로파일러 설정
// ============================================================================

/// 프로파일러 설정
#[derive(Debug, Clone)]
pub struct ProfilerConfig {
    /// 프로파일링 활성화 여부
    pub enabled: bool,
    /// 샘플링 간격 (밀리초)
    pub sampling_interval_ms: u64,
    /// 함수 호출 추적 활성화
    pub trace_function_calls: bool,
    /// 메모리 추적 활성화
    pub trace_memory: bool,
    /// I/O 추적 활성화
    pub trace_io: bool,
    /// 쿼리 추적 활성화
    pub trace_queries: bool,
    /// 느린 쿼리 임계값 (밀리초)
    pub slow_query_threshold_ms: u64,
    /// 최대 추적 항목 수
    pub max_trace_entries: usize,
    /// 히스토그램 버킷 수
    pub histogram_buckets: usize,
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sampling_interval_ms: 100,
            trace_function_calls: true,
            trace_memory: true,
            trace_io: true,
            trace_queries: true,
            slow_query_threshold_ms: 100,
            max_trace_entries: 10_000,
            histogram_buckets: 20,
        }
    }
}

impl ProfilerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_sampling_interval(mut self, ms: u64) -> Self {
        self.sampling_interval_ms = ms;
        self
    }

    pub fn with_slow_query_threshold(mut self, ms: u64) -> Self {
        self.slow_query_threshold_ms = ms;
        self
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

// ============================================================================
// FunctionProfile - 함수 프로파일
// ============================================================================

/// 함수 실행 프로파일
#[derive(Debug, Clone)]
pub struct FunctionProfile {
    /// 함수 이름
    pub name: String,
    /// 총 호출 횟수
    pub call_count: u64,
    /// 총 실행 시간 (나노초)
    pub total_time_ns: u64,
    /// 최소 실행 시간 (나노초)
    pub min_time_ns: u64,
    /// 최대 실행 시간 (나노초)
    pub max_time_ns: u64,
    /// 자체 시간 (하위 호출 제외, 나노초)
    pub self_time_ns: u64,
}

impl FunctionProfile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            call_count: 0,
            total_time_ns: 0,
            min_time_ns: u64::MAX,
            max_time_ns: 0,
            self_time_ns: 0,
        }
    }

    /// 평균 실행 시간 (나노초)
    pub fn avg_time_ns(&self) -> u64 {
        self.total_time_ns.checked_div(self.call_count).unwrap_or(0)
    }

    /// 평균 실행 시간 (밀리초)
    pub fn avg_time_ms(&self) -> f64 {
        self.avg_time_ns() as f64 / 1_000_000.0
    }

    fn record(&mut self, duration_ns: u64) {
        self.call_count += 1;
        self.total_time_ns += duration_ns;
        self.min_time_ns = self.min_time_ns.min(duration_ns);
        self.max_time_ns = self.max_time_ns.max(duration_ns);
    }
}

// ============================================================================
// MemoryProfile - 메모리 프로파일
// ============================================================================

/// 메모리 사용 프로파일
#[derive(Debug, Clone, Default)]
pub struct MemoryProfile {
    /// 현재 할당된 바이트
    pub allocated_bytes: u64,
    /// 총 할당 횟수
    pub allocation_count: u64,
    /// 총 해제 횟수
    pub deallocation_count: u64,
    /// 피크 메모리 사용량
    pub peak_bytes: u64,
    /// 컴포넌트별 메모리 사용량
    pub by_component: HashMap<String, u64>,
}

impl MemoryProfile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_allocation(&mut self, component: &str, bytes: u64) {
        self.allocated_bytes += bytes;
        self.allocation_count += 1;
        self.peak_bytes = self.peak_bytes.max(self.allocated_bytes);
        *self.by_component.entry(component.to_string()).or_insert(0) += bytes;
    }

    pub fn record_deallocation(&mut self, component: &str, bytes: u64) {
        self.allocated_bytes = self.allocated_bytes.saturating_sub(bytes);
        self.deallocation_count += 1;
        if let Some(comp_bytes) = self.by_component.get_mut(component) {
            *comp_bytes = comp_bytes.saturating_sub(bytes);
        }
    }
}

// ============================================================================
// IoProfile - I/O 프로파일
// ============================================================================

/// I/O 프로파일
#[derive(Debug, Clone, Default)]
pub struct IoProfile {
    /// 총 읽기 바이트
    pub bytes_read: u64,
    /// 총 쓰기 바이트
    pub bytes_written: u64,
    /// 읽기 작업 수
    pub read_ops: u64,
    /// 쓰기 작업 수
    pub write_ops: u64,
    /// 총 읽기 시간 (나노초)
    pub read_time_ns: u64,
    /// 총 쓰기 시간 (나노초)
    pub write_time_ns: u64,
}

impl IoProfile {
    pub fn new() -> Self {
        Self::default()
    }

    /// 읽기 처리량 (bytes/sec)
    pub fn read_throughput(&self) -> f64 {
        if self.read_time_ns == 0 {
            0.0
        } else {
            self.bytes_read as f64 / (self.read_time_ns as f64 / 1_000_000_000.0)
        }
    }

    /// 쓰기 처리량 (bytes/sec)
    pub fn write_throughput(&self) -> f64 {
        if self.write_time_ns == 0 {
            0.0
        } else {
            self.bytes_written as f64 / (self.write_time_ns as f64 / 1_000_000_000.0)
        }
    }

    pub fn record_read(&mut self, bytes: u64, duration_ns: u64) {
        self.bytes_read += bytes;
        self.read_ops += 1;
        self.read_time_ns += duration_ns;
    }

    pub fn record_write(&mut self, bytes: u64, duration_ns: u64) {
        self.bytes_written += bytes;
        self.write_ops += 1;
        self.write_time_ns += duration_ns;
    }
}

// ============================================================================
// QueryProfile - 쿼리 프로파일
// ============================================================================

/// 쿼리 프로파일
#[derive(Debug, Clone)]
pub struct QueryProfile {
    /// 쿼리 패턴 (파라미터 제거됨)
    pub pattern: String,
    /// 실행 횟수
    pub execution_count: u64,
    /// 총 실행 시간 (나노초)
    pub total_time_ns: u64,
    /// 최소 실행 시간 (나노초)
    pub min_time_ns: u64,
    /// 최대 실행 시간 (나노초)
    pub max_time_ns: u64,
    /// 총 반환 행 수
    pub total_rows: u64,
    /// 느린 쿼리 횟수
    pub slow_count: u64,
}

impl QueryProfile {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            execution_count: 0,
            total_time_ns: 0,
            min_time_ns: u64::MAX,
            max_time_ns: 0,
            total_rows: 0,
            slow_count: 0,
        }
    }

    /// 평균 실행 시간 (밀리초)
    pub fn avg_time_ms(&self) -> f64 {
        self.total_time_ns
            .checked_div(self.execution_count)
            .unwrap_or(0) as f64
            / 1_000_000.0
    }

    /// 평균 반환 행 수
    pub fn avg_rows(&self) -> f64 {
        if self.execution_count == 0 {
            0.0
        } else {
            self.total_rows as f64 / self.execution_count as f64
        }
    }

    fn record(&mut self, duration_ns: u64, rows: u64, slow_threshold_ns: u64) {
        self.execution_count += 1;
        self.total_time_ns += duration_ns;
        self.min_time_ns = self.min_time_ns.min(duration_ns);
        self.max_time_ns = self.max_time_ns.max(duration_ns);
        self.total_rows += rows;
        if duration_ns > slow_threshold_ns {
            self.slow_count += 1;
        }
    }
}

// ============================================================================
// ConnectionProfile - 연결 프로파일
// ============================================================================

/// 연결 프로파일
#[derive(Debug, Clone, Default)]
pub struct ConnectionProfile {
    /// 활성 연결 수
    pub active_connections: u64,
    /// 총 연결 수
    pub total_connections: u64,
    /// 총 연결 시간 (나노초)
    pub total_connection_time_ns: u64,
    /// 거부된 연결 수
    pub rejected_connections: u64,
    /// 타임아웃된 연결 수
    pub timed_out_connections: u64,
    /// 피크 동시 연결 수
    pub peak_connections: u64,
}

impl ConnectionProfile {
    pub fn new() -> Self {
        Self::default()
    }

    /// 평균 연결 시간 (밀리초)
    pub fn avg_connection_time_ms(&self) -> f64 {
        self.total_connection_time_ns
            .checked_div(self.total_connections)
            .unwrap_or(0) as f64
            / 1_000_000.0
    }

    pub fn record_connect(&mut self) {
        self.active_connections += 1;
        self.total_connections += 1;
        self.peak_connections = self.peak_connections.max(self.active_connections);
    }

    pub fn record_disconnect(&mut self, duration_ns: u64) {
        self.active_connections = self.active_connections.saturating_sub(1);
        self.total_connection_time_ns += duration_ns;
    }

    pub fn record_rejection(&mut self) {
        self.rejected_connections += 1;
    }

    pub fn record_timeout(&mut self) {
        self.timed_out_connections += 1;
    }
}

// ============================================================================
// Bottleneck - 병목 지점
// ============================================================================

/// 병목 유형
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottleneckType {
    /// CPU 병목
    Cpu,
    /// 메모리 병목
    Memory,
    /// I/O 병목
    Io,
    /// 네트워크 병목
    Network,
    /// 락 경합
    LockContention,
    /// 쿼리 병목
    Query,
    /// 연결 풀 병목
    ConnectionPool,
}

/// 병목 심각도
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BottleneckSeverity {
    /// 낮음
    Low,
    /// 중간
    Medium,
    /// 높음
    High,
    /// 심각
    Critical,
}

/// 병목 지점
#[derive(Debug, Clone)]
pub struct Bottleneck {
    /// 병목 유형
    pub bottleneck_type: BottleneckType,
    /// 심각도
    pub severity: BottleneckSeverity,
    /// 위치/컴포넌트
    pub location: String,
    /// 설명
    pub description: String,
    /// 영향도 (0.0 ~ 1.0)
    pub impact: f64,
    /// 권장 조치
    pub recommendation: String,
}

impl Bottleneck {
    pub fn new(
        bottleneck_type: BottleneckType,
        severity: BottleneckSeverity,
        location: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            bottleneck_type,
            severity,
            location: location.into(),
            description: description.into(),
            impact: 0.0,
            recommendation: String::new(),
        }
    }

    pub fn with_impact(mut self, impact: f64) -> Self {
        self.impact = impact.clamp(0.0, 1.0);
        self
    }

    pub fn with_recommendation(mut self, recommendation: impl Into<String>) -> Self {
        self.recommendation = recommendation.into();
        self
    }
}

// ============================================================================
// ProfilerSnapshot - 프로파일러 스냅샷
// ============================================================================

/// 프로파일러 스냅샷
#[derive(Debug, Clone)]
pub struct ProfilerSnapshot {
    /// 스냅샷 시간
    pub timestamp: Instant,
    /// 프로파일링 기간 (나노초)
    pub duration_ns: u64,
    /// 함수 프로파일
    pub functions: Vec<FunctionProfile>,
    /// 메모리 프로파일
    pub memory: MemoryProfile,
    /// I/O 프로파일
    pub io: IoProfile,
    /// 쿼리 프로파일
    pub queries: Vec<QueryProfile>,
    /// 연결 프로파일
    pub connections: ConnectionProfile,
    /// 탐지된 병목
    pub bottlenecks: Vec<Bottleneck>,
}

impl ProfilerSnapshot {
    /// 상위 N개 느린 함수
    pub fn top_slow_functions(&self, n: usize) -> Vec<&FunctionProfile> {
        let mut funcs: Vec<_> = self.functions.iter().collect();
        funcs.sort_by_key(|f| std::cmp::Reverse(f.avg_time_ns()));
        funcs.truncate(n);
        funcs
    }

    /// 상위 N개 느린 쿼리
    pub fn top_slow_queries(&self, n: usize) -> Vec<&QueryProfile> {
        let mut queries: Vec<_> = self.queries.iter().collect();
        queries.sort_by(|a, b| {
            b.avg_time_ms()
                .partial_cmp(&a.avg_time_ms())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        queries.truncate(n);
        queries
    }

    /// 심각한 병목만 필터링
    pub fn critical_bottlenecks(&self) -> Vec<&Bottleneck> {
        self.bottlenecks
            .iter()
            .filter(|b| b.severity >= BottleneckSeverity::High)
            .collect()
    }
}

// ============================================================================
// ServerProfiler - 서버 프로파일러
// ============================================================================

/// 서버 프로파일러
pub struct ServerProfiler {
    config: ProfilerConfig,
    enabled: AtomicBool,
    start_time: Instant,
    functions: RwLock<HashMap<String, FunctionProfile>>,
    memory: RwLock<MemoryProfile>,
    io: RwLock<IoProfile>,
    queries: RwLock<HashMap<String, QueryProfile>>,
    connections: RwLock<ConnectionProfile>,
    sample_count: AtomicU64,
}

impl ServerProfiler {
    pub fn new(config: ProfilerConfig) -> Self {
        let enabled = config.enabled;
        Self {
            config,
            enabled: AtomicBool::new(enabled),
            start_time: Instant::now(),
            functions: RwLock::new(HashMap::new()),
            memory: RwLock::new(MemoryProfile::new()),
            io: RwLock::new(IoProfile::new()),
            queries: RwLock::new(HashMap::new()),
            connections: RwLock::new(ConnectionProfile::new()),
            sample_count: AtomicU64::new(0),
        }
    }

    /// 프로파일링 활성화
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// 프로파일링 비활성화
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// 프로파일링 활성화 여부
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// 함수 실행 기록
    pub fn record_function(&self, name: &str, duration: Duration) {
        if !self.is_enabled() || !self.config.trace_function_calls {
            return;
        }

        let duration_ns = duration.as_nanos() as u64;
        let mut functions = self.functions.write();
        functions
            .entry(name.to_string())
            .or_insert_with(|| FunctionProfile::new(name))
            .record(duration_ns);
    }

    /// 메모리 할당 기록
    pub fn record_allocation(&self, component: &str, bytes: u64) {
        if !self.is_enabled() || !self.config.trace_memory {
            return;
        }
        self.memory.write().record_allocation(component, bytes);
    }

    /// 메모리 해제 기록
    pub fn record_deallocation(&self, component: &str, bytes: u64) {
        if !self.is_enabled() || !self.config.trace_memory {
            return;
        }
        self.memory.write().record_deallocation(component, bytes);
    }

    /// I/O 읽기 기록
    pub fn record_io_read(&self, bytes: u64, duration: Duration) {
        if !self.is_enabled() || !self.config.trace_io {
            return;
        }
        self.io
            .write()
            .record_read(bytes, duration.as_nanos() as u64);
    }

    /// I/O 쓰기 기록
    pub fn record_io_write(&self, bytes: u64, duration: Duration) {
        if !self.is_enabled() || !self.config.trace_io {
            return;
        }
        self.io
            .write()
            .record_write(bytes, duration.as_nanos() as u64);
    }

    /// 쿼리 실행 기록
    pub fn record_query(&self, pattern: &str, duration: Duration, rows: u64) {
        if !self.is_enabled() || !self.config.trace_queries {
            return;
        }

        let duration_ns = duration.as_nanos() as u64;
        let slow_threshold_ns = self.config.slow_query_threshold_ms * 1_000_000;

        let mut queries = self.queries.write();
        queries
            .entry(pattern.to_string())
            .or_insert_with(|| QueryProfile::new(pattern))
            .record(duration_ns, rows, slow_threshold_ns);
    }

    /// 연결 기록
    pub fn record_connect(&self) {
        if !self.is_enabled() {
            return;
        }
        self.connections.write().record_connect();
    }

    /// 연결 해제 기록
    pub fn record_disconnect(&self, duration: Duration) {
        if !self.is_enabled() {
            return;
        }
        self.connections
            .write()
            .record_disconnect(duration.as_nanos() as u64);
    }

    /// 연결 거부 기록
    pub fn record_connection_rejection(&self) {
        if !self.is_enabled() {
            return;
        }
        self.connections.write().record_rejection();
    }

    /// 연결 타임아웃 기록
    pub fn record_connection_timeout(&self) {
        if !self.is_enabled() {
            return;
        }
        self.connections.write().record_timeout();
    }

    /// 스냅샷 생성
    pub fn snapshot(&self) -> ProfilerSnapshot {
        let functions: Vec<_> = self.functions.read().values().cloned().collect();
        let memory = self.memory.read().clone();
        let io = self.io.read().clone();
        let queries: Vec<_> = self.queries.read().values().cloned().collect();
        let connections = self.connections.read().clone();

        // 병목 분석
        let bottlenecks =
            self.analyze_bottlenecks(&functions, &memory, &io, &queries, &connections);

        ProfilerSnapshot {
            timestamp: Instant::now(),
            duration_ns: self.start_time.elapsed().as_nanos() as u64,
            functions,
            memory,
            io,
            queries,
            connections,
            bottlenecks,
        }
    }

    /// 통계 리셋
    pub fn reset(&self) {
        self.functions.write().clear();
        *self.memory.write() = MemoryProfile::new();
        *self.io.write() = IoProfile::new();
        self.queries.write().clear();
        *self.connections.write() = ConnectionProfile::new();
        self.sample_count.store(0, Ordering::SeqCst);
    }

    /// 병목 분석
    fn analyze_bottlenecks(
        &self,
        functions: &[FunctionProfile],
        memory: &MemoryProfile,
        io: &IoProfile,
        queries: &[QueryProfile],
        connections: &ConnectionProfile,
    ) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        // 느린 함수 탐지
        for func in functions {
            if func.avg_time_ms() > 100.0 {
                let severity = if func.avg_time_ms() > 1000.0 {
                    BottleneckSeverity::Critical
                } else if func.avg_time_ms() > 500.0 {
                    BottleneckSeverity::High
                } else {
                    BottleneckSeverity::Medium
                };

                bottlenecks.push(
                    Bottleneck::new(
                        BottleneckType::Cpu,
                        severity,
                        &func.name,
                        format!(
                            "Function average execution time: {:.2}ms",
                            func.avg_time_ms()
                        ),
                    )
                    .with_impact(func.avg_time_ms() / 1000.0)
                    .with_recommendation("Consider function optimization or caching"),
                );
            }
        }

        // 메모리 사용량 분석
        let memory_gb = memory.allocated_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        if memory_gb > 4.0 {
            let severity = if memory_gb > 8.0 {
                BottleneckSeverity::Critical
            } else {
                BottleneckSeverity::High
            };

            bottlenecks.push(
                Bottleneck::new(
                    BottleneckType::Memory,
                    severity,
                    "memory",
                    format!("High memory usage: {:.2}GB", memory_gb),
                )
                .with_impact(memory_gb / 16.0)
                .with_recommendation("Check for memory leaks and adjust cache size"),
            );
        }

        // I/O 처리량 분석
        let read_mbps = io.read_throughput() / (1024.0 * 1024.0);
        let write_mbps = io.write_throughput() / (1024.0 * 1024.0);

        if io.read_ops > 0 && read_mbps < 10.0 {
            bottlenecks.push(
                Bottleneck::new(
                    BottleneckType::Io,
                    BottleneckSeverity::Medium,
                    "io_read",
                    format!("Low read throughput: {:.2}MB/s", read_mbps),
                )
                .with_recommendation("Increase I/O buffer size or use SSD"),
            );
        }

        if io.write_ops > 0 && write_mbps < 10.0 {
            bottlenecks.push(
                Bottleneck::new(
                    BottleneckType::Io,
                    BottleneckSeverity::Medium,
                    "io_write",
                    format!("Low write throughput: {:.2}MB/s", write_mbps),
                )
                .with_recommendation("Use batch writes or asynchronous I/O"),
            );
        }

        // 느린 쿼리 분석
        for query in queries {
            if query.slow_count > 0 {
                let slow_ratio = query.slow_count as f64 / query.execution_count as f64;
                if slow_ratio > 0.1 {
                    let severity = if slow_ratio > 0.5 {
                        BottleneckSeverity::Critical
                    } else if slow_ratio > 0.3 {
                        BottleneckSeverity::High
                    } else {
                        BottleneckSeverity::Medium
                    };

                    bottlenecks.push(
                        Bottleneck::new(
                            BottleneckType::Query,
                            severity,
                            &query.pattern,
                            format!(
                                "Slow query ratio: {:.1}% (avg: {:.2}ms)",
                                slow_ratio * 100.0,
                                query.avg_time_ms()
                            ),
                        )
                        .with_impact(slow_ratio)
                        .with_recommendation("Optimize queries or add indexes"),
                    );
                }
            }
        }

        // 연결 풀 분석
        if connections.rejected_connections > 0 {
            let rejection_ratio = connections.rejected_connections as f64
                / (connections.total_connections + connections.rejected_connections) as f64;

            if rejection_ratio > 0.01 {
                bottlenecks.push(
                    Bottleneck::new(
                        BottleneckType::ConnectionPool,
                        BottleneckSeverity::High,
                        "connection_pool",
                        format!(
                            "Connection rejection rate: {:.1}% ({} rejected)",
                            rejection_ratio * 100.0,
                            connections.rejected_connections
                        ),
                    )
                    .with_impact(rejection_ratio)
                    .with_recommendation("Increase connection pool size"),
                );
            }
        }

        // 심각도순 정렬
        bottlenecks.sort_by_key(|bottleneck| std::cmp::Reverse(bottleneck.severity));
        bottlenecks
    }
}

impl Default for ServerProfiler {
    fn default() -> Self {
        Self::new(ProfilerConfig::default())
    }
}

// ============================================================================
// ProfileGuard - 프로파일 가드 (RAII)
// ============================================================================

/// 함수 프로파일링을 위한 RAII 가드
pub struct ProfileGuard<'a> {
    profiler: &'a ServerProfiler,
    name: String,
    start: Instant,
}

impl<'a> ProfileGuard<'a> {
    pub fn new(profiler: &'a ServerProfiler, name: impl Into<String>) -> Self {
        Self {
            profiler,
            name: name.into(),
            start: Instant::now(),
        }
    }
}

impl<'a> Drop for ProfileGuard<'a> {
    fn drop(&mut self) {
        self.profiler
            .record_function(&self.name, self.start.elapsed());
    }
}

/// 프로파일링 매크로
#[macro_export]
macro_rules! profile_fn {
    ($profiler:expr, $name:expr) => {
        let _guard = $crate::server::profiler::ProfileGuard::new($profiler, $name);
    };
}

// ============================================================================
// LoadTestConfig - 부하 테스트 설정
// ============================================================================

/// 부하 테스트 설정
#[derive(Debug, Clone)]
pub struct LoadTestConfig {
    /// 동시 연결 수
    pub concurrent_connections: usize,
    /// 테스트 기간 (초)
    pub duration_secs: u64,
    /// 초당 요청 수 (0 = 무제한)
    pub requests_per_second: u64,
    /// 워밍업 기간 (초)
    pub warmup_secs: u64,
    /// 쿨다운 기간 (초)
    pub cooldown_secs: u64,
    /// 연결당 쿼리 수
    pub queries_per_connection: usize,
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        Self {
            concurrent_connections: 100,
            duration_secs: 60,
            requests_per_second: 0,
            warmup_secs: 5,
            cooldown_secs: 5,
            queries_per_connection: 100,
        }
    }
}

/// 부하 테스트 결과
#[derive(Debug, Clone)]
pub struct LoadTestResult {
    /// 총 요청 수
    pub total_requests: u64,
    /// 성공 요청 수
    pub successful_requests: u64,
    /// 실패 요청 수
    pub failed_requests: u64,
    /// 총 테스트 시간 (밀리초)
    pub total_time_ms: u64,
    /// 평균 응답 시간 (밀리초)
    pub avg_response_time_ms: f64,
    /// P50 응답 시간 (밀리초)
    pub p50_response_time_ms: f64,
    /// P95 응답 시간 (밀리초)
    pub p95_response_time_ms: f64,
    /// P99 응답 시간 (밀리초)
    pub p99_response_time_ms: f64,
    /// 최대 응답 시간 (밀리초)
    pub max_response_time_ms: f64,
    /// 초당 요청 처리량
    pub requests_per_second: f64,
    /// 처리량 (bytes/sec)
    pub throughput_bytes_per_sec: f64,
    /// 에러율
    pub error_rate: f64,
}

impl LoadTestResult {
    /// 테스트 성공 여부
    pub fn is_successful(&self) -> bool {
        self.error_rate < 0.01 && self.failed_requests == 0
    }

    /// 성능 등급
    pub fn grade(&self) -> char {
        if self.requests_per_second > 10000.0 && self.p99_response_time_ms < 10.0 {
            'A'
        } else if self.requests_per_second > 5000.0 && self.p99_response_time_ms < 50.0 {
            'B'
        } else if self.requests_per_second > 1000.0 && self.p99_response_time_ms < 100.0 {
            'C'
        } else if self.requests_per_second > 100.0 && self.p99_response_time_ms < 500.0 {
            'D'
        } else {
            'F'
        }
    }
}
