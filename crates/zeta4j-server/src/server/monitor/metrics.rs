// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! M7.1: Extended Metrics Collection
//!
//! 통합 메트릭 레지스트리 및 확장 메트릭 수집
//!
//! ## Prometheus 메트릭
//!
//! | 메트릭 | 타입 | 설명 |
//! |--------|------|------|
//! | `zeta4j_info` | gauge | 빌드 정보 (버전, 에디션 등) |
//! | `zeta4j_uptime_seconds` | gauge | 서버 가동 시간 |
//! | `zeta4j_queries_total` | counter | 총 쿼리 수 |
//! | `zeta4j_query_duration_seconds` | histogram | 쿼리 실행 시간 분포 |
//! | `zeta4j_connections_accepted` | counter | 수락된 연결 수 |
//! | `zeta4j_active_connections` | gauge | 현재 활성 연결 |
//! | `zeta4j_cpu_usage_percent` | gauge | CPU 사용률 |
//! | `zeta4j_memory_used_bytes` | gauge | 메모리 사용량 |

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use super::server_metrics::{Counter, Gauge, Histogram, ServerMetrics};

// ============================================================================
// BuildInfo - 빌드 정보 메트릭
// ============================================================================

/// 빌드 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    /// 버전
    pub version: String,
    /// 에디션 (Base)
    pub edition: String,
    /// Git 커밋 해시
    pub git_commit: Option<String>,
    /// 빌드 시간
    pub build_time: Option<String>,
    /// Rust 버전
    pub rust_version: Option<String>,
    /// 타겟 플랫폼
    pub target: Option<String>,
}

impl BuildInfo {
    /// 기본 빌드 정보 생성
    pub fn new(version: &str, edition: &str) -> Self {
        Self {
            version: version.to_string(),
            edition: edition.to_string(),
            git_commit: option_env!("GIT_COMMIT").map(String::from),
            build_time: option_env!("BUILD_TIME").map(String::from),
            rust_version: option_env!("RUSTC_VERSION").map(String::from),
            target: option_env!("TARGET").map(String::from),
        }
    }

    /// Prometheus INFO 메트릭 출력
    pub fn to_prometheus(&self) -> String {
        let mut labels = vec![
            format!("version=\"{}\"", self.version),
            format!("edition=\"{}\"", self.edition),
        ];

        if let Some(ref commit) = self.git_commit {
            labels.push(format!("git_commit=\"{}\"", commit));
        }
        if let Some(ref target) = self.target {
            labels.push(format!("target=\"{}\"", target));
        }

        format!(
            "# HELP zeta4j_info Build information\n\
             # TYPE zeta4j_info gauge\n\
             zeta4j_info{{{}}} 1\n",
            labels.join(",")
        )
    }
}

impl Default for BuildInfo {
    fn default() -> Self {
        Self::new(env!("CARGO_PKG_VERSION"), "Standard")
    }
}

// ============================================================================
// MetricsRegistry - 통합 메트릭 레지스트리
// ============================================================================

/// 통합 메트릭 레지스트리
pub struct MetricsRegistry {
    /// 빌드 정보
    build_info: BuildInfo,
    /// 서버 메트릭
    server: ServerMetrics,
    /// 쿼리 메트릭
    query: QueryMetrics,
    /// 시스템 메트릭
    system: SystemMetrics,
    /// 캐시 메트릭
    cache: CacheMetrics,
    /// 연결 풀 메트릭
    connection_pool: ConnectionPoolMetrics,
    /// 데이터베이스별 메트릭
    databases: RwLock<HashMap<String, DatabaseMetrics>>,
    /// 시작 시간
    start_time: Instant,
}

impl MetricsRegistry {
    /// 새 레지스트리 생성
    pub fn new() -> Self {
        Self::with_build_info(BuildInfo::default())
    }

    /// 빌드 정보와 함께 레지스트리 생성
    pub fn with_build_info(build_info: BuildInfo) -> Self {
        Self {
            build_info,
            server: ServerMetrics::new(),
            query: QueryMetrics::new(),
            system: SystemMetrics::new(),
            cache: CacheMetrics::new(),
            connection_pool: ConnectionPoolMetrics::new(),
            databases: RwLock::new(HashMap::new()),
            start_time: Instant::now(),
        }
    }

    /// 서버 메트릭 접근
    pub fn server(&self) -> &ServerMetrics {
        &self.server
    }

    /// 쿼리 메트릭 접근
    pub fn query(&self) -> &QueryMetrics {
        &self.query
    }

    /// 시스템 메트릭 접근
    pub fn system(&self) -> &SystemMetrics {
        &self.system
    }

    /// 캐시 메트릭 접근
    pub fn cache(&self) -> &CacheMetrics {
        &self.cache
    }

    /// 연결 풀 메트릭 접근
    pub fn connection_pool(&self) -> &ConnectionPoolMetrics {
        &self.connection_pool
    }

    /// 빌드 정보 접근
    pub fn build_info(&self) -> &BuildInfo {
        &self.build_info
    }

    /// 데이터베이스 메트릭 가져오기 또는 생성
    pub fn database(&self, name: &str) -> DatabaseMetrics {
        {
            let dbs = self.databases.read().unwrap_or_else(|e| e.into_inner());
            if let Some(metrics) = dbs.get(name) {
                return metrics.clone();
            }
        }

        let mut dbs = self.databases.write().unwrap_or_else(|e| e.into_inner());
        dbs.entry(name.to_string())
            .or_insert_with(|| DatabaseMetrics::new(name.to_string()))
            .clone()
    }

    /// Uptime 업데이트
    pub fn update_uptime(&self) {
        let uptime = self.start_time.elapsed().as_secs() as i64;
        self.server.uptime_seconds.set(uptime);
    }

    /// Prometheus 포맷 출력
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        // 빌드 정보
        output.push_str(&self.build_info.to_prometheus());

        // 서버 메트릭
        output.push_str(&self.server.to_prometheus());

        // 쿼리 메트릭
        output.push_str(&self.query.to_prometheus());

        // 시스템 메트릭
        output.push_str(&self.system.to_prometheus());

        // 캐시 메트릭
        output.push_str(&self.cache.to_prometheus());

        // 연결 풀 메트릭
        output.push_str(&self.connection_pool.to_prometheus());

        // 데이터베이스 메트릭
        let dbs = self.databases.read().unwrap_or_else(|e| e.into_inner());
        for db_metrics in dbs.values() {
            output.push_str(&db_metrics.to_prometheus());
        }

        output
    }

    /// 스냅샷 생성
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            timestamp: chrono::Utc::now(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            server: self.server.snapshot(),
            query: self.query.snapshot(),
            system: self.system.snapshot(),
        }
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// QueryMetrics - 쿼리 메트릭
// ============================================================================

/// 쿼리 메트릭
pub struct QueryMetrics {
    /// 총 쿼리 수
    pub total: Counter,
    /// 성공 쿼리 수
    pub success: Counter,
    /// 실패 쿼리 수
    pub failed: Counter,
    /// 쿼리 실행 시간
    pub duration: Histogram,
    /// 슬로우 쿼리 수
    pub slow_queries: Counter,
    /// 슬로우 쿼리 임계값 (밀리초)
    pub slow_threshold_ms: AtomicU64,
    /// 쿼리 타입별 카운트
    query_types: RwLock<HashMap<String, Counter>>,
    /// 읽기 쿼리 수
    pub reads: Counter,
    /// 쓰기 쿼리 수
    pub writes: Counter,
}

impl QueryMetrics {
    /// 새 쿼리 메트릭 생성
    pub fn new() -> Self {
        Self {
            total: Counter::new(),
            success: Counter::new(),
            failed: Counter::new(),
            duration: Histogram::with_buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            slow_queries: Counter::new(),
            slow_threshold_ms: AtomicU64::new(1000), // 기본 1초
            query_types: RwLock::new(HashMap::new()),
            reads: Counter::new(),
            writes: Counter::new(),
        }
    }

    /// 쿼리 기록
    pub fn record(&self, query_type: &str, duration: Duration, success: bool) {
        self.total.inc();

        if success {
            self.success.inc();
        } else {
            self.failed.inc();
        }

        let duration_secs = duration.as_secs_f64();
        self.duration.observe(duration_secs);

        // 슬로우 쿼리 체크
        let threshold_ms = self.slow_threshold_ms.load(Ordering::Relaxed);
        if duration.as_millis() as u64 > threshold_ms {
            self.slow_queries.inc();
        }

        // 쿼리 타입별 카운트
        let mut types = self.query_types.write().unwrap_or_else(|e| e.into_inner());
        types.entry(query_type.to_string()).or_default().inc();

        // 읽기/쓰기 분류
        let upper = query_type.to_uppercase();
        if upper.starts_with("MATCH") || upper.starts_with("RETURN") {
            self.reads.inc();
        } else if upper.starts_with("CREATE")
            || upper.starts_with("DELETE")
            || upper.starts_with("SET")
            || upper.starts_with("MERGE")
        {
            self.writes.inc();
        }
    }

    /// 슬로우 쿼리 임계값 설정 (밀리초)
    pub fn set_slow_threshold_ms(&self, threshold: u64) {
        self.slow_threshold_ms.store(threshold, Ordering::Relaxed);
    }

    /// Prometheus 포맷 출력
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        output.push_str("# HELP zeta4j_queries_total Total queries executed\n");
        output.push_str("# TYPE zeta4j_queries_total counter\n");
        output.push_str(&format!("zeta4j_queries_total {}\n", self.total.get()));

        output.push_str("# HELP zeta4j_queries_success Successful queries\n");
        output.push_str("# TYPE zeta4j_queries_success counter\n");
        output.push_str(&format!("zeta4j_queries_success {}\n", self.success.get()));

        output.push_str("# HELP zeta4j_queries_failed Failed queries\n");
        output.push_str("# TYPE zeta4j_queries_failed counter\n");
        output.push_str(&format!("zeta4j_queries_failed {}\n", self.failed.get()));

        output.push_str("# HELP zeta4j_slow_queries Slow queries count\n");
        output.push_str("# TYPE zeta4j_slow_queries counter\n");
        output.push_str(&format!(
            "zeta4j_slow_queries {}\n",
            self.slow_queries.get()
        ));

        output.push_str("# HELP zeta4j_queries_reads Read queries\n");
        output.push_str("# TYPE zeta4j_queries_reads counter\n");
        output.push_str(&format!("zeta4j_queries_reads {}\n", self.reads.get()));

        output.push_str("# HELP zeta4j_queries_writes Write queries\n");
        output.push_str("# TYPE zeta4j_queries_writes counter\n");
        output.push_str(&format!("zeta4j_queries_writes {}\n", self.writes.get()));

        output.push_str("# HELP zeta4j_query_duration_seconds Query execution duration\n");
        output.push_str("# TYPE zeta4j_query_duration_seconds histogram\n");
        output.push_str(&format!(
            "zeta4j_query_duration_seconds_sum {}\n",
            self.duration.sum()
        ));
        output.push_str(&format!(
            "zeta4j_query_duration_seconds_count {}\n",
            self.duration.count()
        ));

        output
    }

    /// 스냅샷 생성
    pub fn snapshot(&self) -> QueryMetricsSnapshot {
        QueryMetricsSnapshot {
            total: self.total.get(),
            success: self.success.get(),
            failed: self.failed.get(),
            slow_queries: self.slow_queries.get(),
            reads: self.reads.get(),
            writes: self.writes.get(),
            avg_duration_ms: (self.duration.mean() * 1000.0) as u64,
            p50_duration_ms: (self.duration.percentile(50.0) * 1000.0) as u64,
            p99_duration_ms: (self.duration.percentile(99.0) * 1000.0) as u64,
        }
    }
}

impl Default for QueryMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SystemMetrics - 시스템 메트릭
// ============================================================================

/// 시스템 메트릭
pub struct SystemMetrics {
    /// CPU 사용률 (퍼센트)
    pub cpu_usage: Gauge,
    /// 메모리 사용량 (바이트)
    pub memory_used: Gauge,
    /// 메모리 총량 (바이트)
    pub memory_total: Gauge,
    /// 디스크 사용량 (바이트)
    pub disk_used: Gauge,
    /// 디스크 총량 (바이트)
    pub disk_total: Gauge,
    /// 열린 파일 수
    pub open_files: Gauge,
    /// 스레드 수
    pub thread_count: Gauge,
    /// 힙 메모리 사용량 (바이트)
    pub heap_used: Gauge,
}

impl SystemMetrics {
    /// 새 시스템 메트릭 생성
    pub fn new() -> Self {
        Self {
            cpu_usage: Gauge::new(),
            memory_used: Gauge::new(),
            memory_total: Gauge::new(),
            disk_used: Gauge::new(),
            disk_total: Gauge::new(),
            open_files: Gauge::new(),
            thread_count: Gauge::new(),
            heap_used: Gauge::new(),
        }
    }

    /// 시스템 상태 업데이트
    pub fn update(&self, stats: &SystemStats) {
        self.cpu_usage.set((stats.cpu_usage * 100.0) as i64);
        self.memory_used.set(stats.memory_used as i64);
        self.memory_total.set(stats.memory_total as i64);
        self.disk_used.set(stats.disk_used as i64);
        self.disk_total.set(stats.disk_total as i64);
        self.open_files.set(stats.open_files as i64);
        self.thread_count.set(stats.thread_count as i64);
        self.heap_used.set(stats.heap_used as i64);
    }

    /// Prometheus 포맷 출력
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        output.push_str("# HELP zeta4j_cpu_usage_percent CPU usage percentage\n");
        output.push_str("# TYPE zeta4j_cpu_usage_percent gauge\n");
        output.push_str(&format!(
            "zeta4j_cpu_usage_percent {}\n",
            self.cpu_usage.get()
        ));

        output.push_str("# HELP zeta4j_memory_used_bytes Memory used in bytes\n");
        output.push_str("# TYPE zeta4j_memory_used_bytes gauge\n");
        output.push_str(&format!(
            "zeta4j_memory_used_bytes {}\n",
            self.memory_used.get()
        ));

        output.push_str("# HELP zeta4j_memory_total_bytes Total memory in bytes\n");
        output.push_str("# TYPE zeta4j_memory_total_bytes gauge\n");
        output.push_str(&format!(
            "zeta4j_memory_total_bytes {}\n",
            self.memory_total.get()
        ));

        output.push_str("# HELP zeta4j_disk_used_bytes Disk used in bytes\n");
        output.push_str("# TYPE zeta4j_disk_used_bytes gauge\n");
        output.push_str(&format!(
            "zeta4j_disk_used_bytes {}\n",
            self.disk_used.get()
        ));

        output.push_str("# HELP zeta4j_open_files Current open file descriptors\n");
        output.push_str("# TYPE zeta4j_open_files gauge\n");
        output.push_str(&format!("zeta4j_open_files {}\n", self.open_files.get()));

        output.push_str("# HELP zeta4j_thread_count Current thread count\n");
        output.push_str("# TYPE zeta4j_thread_count gauge\n");
        output.push_str(&format!(
            "zeta4j_thread_count {}\n",
            self.thread_count.get()
        ));

        output
    }

    /// 스냅샷 생성
    pub fn snapshot(&self) -> SystemMetricsSnapshot {
        SystemMetricsSnapshot {
            cpu_usage_percent: self.cpu_usage.get() as f64,
            memory_used: self.memory_used.get() as u64,
            memory_total: self.memory_total.get() as u64,
            disk_used: self.disk_used.get() as u64,
            disk_total: self.disk_total.get() as u64,
            open_files: self.open_files.get() as u64,
            thread_count: self.thread_count.get() as u64,
        }
    }
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 시스템 상태 정보
#[derive(Debug, Clone, Default)]
pub struct SystemStats {
    pub cpu_usage: f64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub open_files: u64,
    pub thread_count: u64,
    pub heap_used: u64,
}

impl SystemStats {
    /// sysinfo crate를 사용하여 시스템 상태 수집
    pub fn collect() -> Self {
        use sysinfo::{Disks, System};

        let mut sys = System::new_all();
        sys.refresh_all();

        // CPU 사용률 (평균)
        let cpu_usage = sys
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage() as f64)
            .sum::<f64>()
            / sys.cpus().len().max(1) as f64;

        // 메모리
        let memory_used = sys.used_memory();
        let memory_total = sys.total_memory();

        // 디스크 (첫 번째 디스크 기준)
        let disks = Disks::new_with_refreshed_list();
        let (disk_used, disk_total) = disks
            .list()
            .first()
            .map(|d| {
                let total = d.total_space();
                let free = d.available_space();
                (total.saturating_sub(free), total)
            })
            .unwrap_or((0, 0));

        // 프로세스 정보
        let current_pid = sysinfo::get_current_pid().ok();
        let (thread_count, heap_used) = current_pid
            .and_then(|pid| sys.process(pid))
            .map(|proc| {
                (
                    proc.tasks().map(|t| t.len()).unwrap_or(1) as u64,
                    proc.memory(),
                )
            })
            .unwrap_or((1, 0));

        // 열린 파일 수 (Unix 전용)
        #[cfg(unix)]
        let open_files = {
            use std::fs;
            fs::read_dir("/proc/self/fd")
                .map(|d| d.count() as u64)
                .unwrap_or(0)
        };
        #[cfg(not(unix))]
        let open_files = 0u64;

        Self {
            cpu_usage: cpu_usage / 100.0, // 0.0 ~ 1.0으로 정규화
            memory_used,
            memory_total,
            disk_used,
            disk_total,
            open_files,
            thread_count,
            heap_used,
        }
    }
}

// ============================================================================
// CacheMetrics - 캐시 메트릭
// ============================================================================

/// 캐시 메트릭
pub struct CacheMetrics {
    /// 캐시 히트 수
    pub hits: Counter,
    /// 캐시 미스 수
    pub misses: Counter,
    /// 캐시 항목 수
    pub entries: Gauge,
    /// 캐시 크기 (바이트)
    pub size_bytes: Gauge,
    /// 캐시 제거 수
    pub evictions: Counter,
}

impl CacheMetrics {
    /// 새 캐시 메트릭 생성
    pub fn new() -> Self {
        Self {
            hits: Counter::new(),
            misses: Counter::new(),
            entries: Gauge::new(),
            size_bytes: Gauge::new(),
            evictions: Counter::new(),
        }
    }

    /// 캐시 조회 기록
    pub fn record_lookup(&self, hit: bool) {
        if hit {
            self.hits.inc();
        } else {
            self.misses.inc();
        }
    }

    /// 히트율 계산
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.get();
        let total = hits + self.misses.get();
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Prometheus 포맷 출력
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        output.push_str("# HELP zeta4j_cache_hits_total Cache hit count\n");
        output.push_str("# TYPE zeta4j_cache_hits_total counter\n");
        output.push_str(&format!("zeta4j_cache_hits_total {}\n", self.hits.get()));

        output.push_str("# HELP zeta4j_cache_misses_total Cache miss count\n");
        output.push_str("# TYPE zeta4j_cache_misses_total counter\n");
        output.push_str(&format!(
            "zeta4j_cache_misses_total {}\n",
            self.misses.get()
        ));

        output.push_str("# HELP zeta4j_cache_hit_rate Cache hit rate (0.0-1.0)\n");
        output.push_str("# TYPE zeta4j_cache_hit_rate gauge\n");
        output.push_str(&format!("zeta4j_cache_hit_rate {:.4}\n", self.hit_rate()));

        output.push_str("# HELP zeta4j_cache_entries Current cache entries\n");
        output.push_str("# TYPE zeta4j_cache_entries gauge\n");
        output.push_str(&format!("zeta4j_cache_entries {}\n", self.entries.get()));

        output.push_str("# HELP zeta4j_cache_size_bytes Cache size in bytes\n");
        output.push_str("# TYPE zeta4j_cache_size_bytes gauge\n");
        output.push_str(&format!(
            "zeta4j_cache_size_bytes {}\n",
            self.size_bytes.get()
        ));

        output.push_str("# HELP zeta4j_cache_evictions_total Cache eviction count\n");
        output.push_str("# TYPE zeta4j_cache_evictions_total counter\n");
        output.push_str(&format!(
            "zeta4j_cache_evictions_total {}\n",
            self.evictions.get()
        ));

        output
    }
}

impl Default for CacheMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ConnectionPoolMetrics - 연결 풀 메트릭
// ============================================================================

/// 연결 풀 메트릭
pub struct ConnectionPoolMetrics {
    /// 활성 연결 수
    pub active: Gauge,
    /// 유휴 연결 수
    pub idle: Gauge,
    /// 대기 중인 요청 수
    pub pending: Gauge,
    /// 최대 연결 수
    pub max_connections: Gauge,
    /// 연결 획득 대기 시간
    pub acquire_latency: Histogram,
    /// 연결 타임아웃 수
    pub timeouts: Counter,
    /// 연결 생성 수
    pub connections_created: Counter,
    /// 연결 종료 수
    pub connections_closed: Counter,
}

impl ConnectionPoolMetrics {
    /// 새 연결 풀 메트릭 생성
    pub fn new() -> Self {
        Self {
            active: Gauge::new(),
            idle: Gauge::new(),
            pending: Gauge::new(),
            max_connections: Gauge::new(),
            acquire_latency: Histogram::with_buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
            ]),
            timeouts: Counter::new(),
            connections_created: Counter::new(),
            connections_closed: Counter::new(),
        }
    }

    /// 연결 풀 상태 업데이트
    pub fn update_pool_state(&self, active: i64, idle: i64, pending: i64) {
        self.active.set(active);
        self.idle.set(idle);
        self.pending.set(pending);
    }

    /// 연결 획득 시간 기록
    pub fn record_acquire(&self, duration: Duration) {
        self.acquire_latency.observe(duration.as_secs_f64());
    }

    /// 사용률 계산
    pub fn utilization(&self) -> f64 {
        let max = self.max_connections.get();
        if max == 0 {
            0.0
        } else {
            self.active.get() as f64 / max as f64
        }
    }

    /// Prometheus 포맷 출력
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        output.push_str("# HELP zeta4j_pool_active_connections Active connections in pool\n");
        output.push_str("# TYPE zeta4j_pool_active_connections gauge\n");
        output.push_str(&format!(
            "zeta4j_pool_active_connections {}\n",
            self.active.get()
        ));

        output.push_str("# HELP zeta4j_pool_idle_connections Idle connections in pool\n");
        output.push_str("# TYPE zeta4j_pool_idle_connections gauge\n");
        output.push_str(&format!(
            "zeta4j_pool_idle_connections {}\n",
            self.idle.get()
        ));

        output.push_str("# HELP zeta4j_pool_pending_requests Pending connection requests\n");
        output.push_str("# TYPE zeta4j_pool_pending_requests gauge\n");
        output.push_str(&format!(
            "zeta4j_pool_pending_requests {}\n",
            self.pending.get()
        ));

        output.push_str("# HELP zeta4j_pool_max_connections Maximum pool size\n");
        output.push_str("# TYPE zeta4j_pool_max_connections gauge\n");
        output.push_str(&format!(
            "zeta4j_pool_max_connections {}\n",
            self.max_connections.get()
        ));

        output.push_str("# HELP zeta4j_pool_utilization Pool utilization (0.0-1.0)\n");
        output.push_str("# TYPE zeta4j_pool_utilization gauge\n");
        output.push_str(&format!(
            "zeta4j_pool_utilization {:.4}\n",
            self.utilization()
        ));

        output.push_str("# HELP zeta4j_pool_timeouts_total Connection timeout count\n");
        output.push_str("# TYPE zeta4j_pool_timeouts_total counter\n");
        output.push_str(&format!(
            "zeta4j_pool_timeouts_total {}\n",
            self.timeouts.get()
        ));

        output.push_str(&self.acquire_latency.to_prometheus(
            "zeta4j_pool_acquire_duration_seconds",
            "Connection acquire duration in seconds",
        ));

        output
    }
}

impl Default for ConnectionPoolMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// DatabaseMetrics - 데이터베이스별 메트릭
// ============================================================================

/// 데이터베이스 메트릭
#[derive(Clone)]
pub struct DatabaseMetrics {
    name: String,
    /// 노드 수
    node_count: Arc<Gauge>,
    /// 관계 수
    relationship_count: Arc<Gauge>,
    /// 저장 크기 (바이트)
    store_size: Arc<Gauge>,
    /// 트랜잭션 수
    transactions: Arc<Counter>,
    /// 커밋된 트랜잭션
    commits: Arc<Counter>,
    /// 롤백된 트랜잭션
    rollbacks: Arc<Counter>,
}

impl DatabaseMetrics {
    /// 새 데이터베이스 메트릭 생성
    pub fn new(name: String) -> Self {
        Self {
            name,
            node_count: Arc::new(Gauge::new()),
            relationship_count: Arc::new(Gauge::new()),
            store_size: Arc::new(Gauge::new()),
            transactions: Arc::new(Counter::new()),
            commits: Arc::new(Counter::new()),
            rollbacks: Arc::new(Counter::new()),
        }
    }

    /// 노드 수 설정
    pub fn set_node_count(&self, count: u64) {
        self.node_count.set(count as i64);
    }

    /// 관계 수 설정
    pub fn set_relationship_count(&self, count: u64) {
        self.relationship_count.set(count as i64);
    }

    /// 저장 크기 설정
    pub fn set_store_size(&self, size: u64) {
        self.store_size.set(size as i64);
    }

    /// 트랜잭션 기록
    pub fn record_transaction(&self, committed: bool) {
        self.transactions.inc();
        if committed {
            self.commits.inc();
        } else {
            self.rollbacks.inc();
        }
    }

    /// Prometheus 포맷 출력
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        output.push_str(&format!(
            "zeta4j_db_node_count{{database=\"{}\"}} {}\n",
            self.name,
            self.node_count.get()
        ));

        output.push_str(&format!(
            "zeta4j_db_relationship_count{{database=\"{}\"}} {}\n",
            self.name,
            self.relationship_count.get()
        ));

        output.push_str(&format!(
            "zeta4j_db_store_size_bytes{{database=\"{}\"}} {}\n",
            self.name,
            self.store_size.get()
        ));

        output.push_str(&format!(
            "zeta4j_db_transactions_total{{database=\"{}\"}} {}\n",
            self.name,
            self.transactions.get()
        ));

        output.push_str(&format!(
            "zeta4j_db_commits_total{{database=\"{}\"}} {}\n",
            self.name,
            self.commits.get()
        ));

        output.push_str(&format!(
            "zeta4j_db_rollbacks_total{{database=\"{}\"}} {}\n",
            self.name,
            self.rollbacks.get()
        ));

        output
    }
}

// ============================================================================
// Snapshots - 스냅샷 타입들
// ============================================================================

/// 전체 메트릭 스냅샷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub uptime_seconds: u64,
    pub server: super::server_metrics::ServerMetricsSnapshot,
    pub query: QueryMetricsSnapshot,
    pub system: SystemMetricsSnapshot,
}

/// 쿼리 메트릭 스냅샷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetricsSnapshot {
    pub total: u64,
    pub success: u64,
    pub failed: u64,
    pub slow_queries: u64,
    pub reads: u64,
    pub writes: u64,
    pub avg_duration_ms: u64,
    pub p50_duration_ms: u64,
    pub p99_duration_ms: u64,
}

/// 시스템 메트릭 스냅샷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetricsSnapshot {
    pub cpu_usage_percent: f64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub open_files: u64,
    pub thread_count: u64,
}
