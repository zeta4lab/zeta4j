//! Server Metrics

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// 서버 메트릭 스냅샷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerMetricsSnapshot {
    pub connections_accepted: u64,
    pub connections_rejected: u64,
    pub active_connections: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub requests_processed: u64,
    pub accept_latency_p50: f64,
    pub accept_latency_p99: f64,
    pub request_latency_p50: f64,
    pub request_latency_p99: f64,
}

/// 서버 메트릭
pub struct ServerMetrics {
    // === 연결 메트릭 ===
    pub connections_accepted: Counter,
    pub connections_rejected_max: Counter,
    pub connections_rejected_backpressure: Counter,
    pub connections_rejected_per_ip: Counter,
    pub connections_closed_normal: Counter,
    pub connections_closed_error: Counter,
    pub connections_closed_timeout: Counter,
    pub active_connections: Gauge,

    // === Accept 메트릭 ===
    pub accept_errors: Counter,
    pub accept_errors_fd_limit: Counter,
    pub accept_latency: Histogram,

    // === 처리량 메트릭 ===
    pub bytes_read: Counter,
    pub bytes_written: Counter,
    pub requests_processed: Counter,
    pub request_latency: Histogram,

    // === 백프레셔 메트릭 ===
    pub backpressure_triggered: Counter,
    pub backpressure_released: Counter,
    pub rate_limited: Counter,

    // === 시스템 메트릭 ===
    pub uptime_seconds: Gauge,
}

impl ServerMetrics {
    /// 새 메트릭 생성
    pub fn new() -> Self {
        Self {
            connections_accepted: Counter::new(),
            connections_rejected_max: Counter::new(),
            connections_rejected_backpressure: Counter::new(),
            connections_rejected_per_ip: Counter::new(),
            connections_closed_normal: Counter::new(),
            connections_closed_error: Counter::new(),
            connections_closed_timeout: Counter::new(),
            active_connections: Gauge::new(),

            accept_errors: Counter::new(),
            accept_errors_fd_limit: Counter::new(),
            accept_latency: Histogram::new(),

            bytes_read: Counter::new(),
            bytes_written: Counter::new(),
            requests_processed: Counter::new(),
            request_latency: Histogram::new(),

            backpressure_triggered: Counter::new(),
            backpressure_released: Counter::new(),
            rate_limited: Counter::new(),

            uptime_seconds: Gauge::new(),
        }
    }

    /// 스냅샷 생성
    pub fn snapshot(&self) -> ServerMetricsSnapshot {
        ServerMetricsSnapshot {
            connections_accepted: self.connections_accepted.get(),
            connections_rejected: self.connections_rejected_max.get()
                + self.connections_rejected_backpressure.get()
                + self.connections_rejected_per_ip.get(),
            active_connections: self.active_connections.get() as u64,
            bytes_read: self.bytes_read.get(),
            bytes_written: self.bytes_written.get(),
            requests_processed: self.requests_processed.get(),
            accept_latency_p50: self.accept_latency.percentile(50.0),
            accept_latency_p99: self.accept_latency.percentile(99.0),
            request_latency_p50: self.request_latency.percentile(50.0),
            request_latency_p99: self.request_latency.percentile(99.0),
        }
    }

    /// Prometheus 포맷으로 출력
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        // 연결 메트릭
        output.push_str("# HELP zeta4j_connections_accepted Total accepted connections\n");
        output.push_str("# TYPE zeta4j_connections_accepted counter\n");
        output.push_str(&format!(
            "zeta4j_connections_accepted {}\n",
            self.connections_accepted.get()
        ));

        output.push_str("# HELP zeta4j_connections_rejected Total rejected connections\n");
        output.push_str("# TYPE zeta4j_connections_rejected counter\n");
        output.push_str(&format!(
            "zeta4j_connections_rejected{{reason=\"max\"}} {}\n",
            self.connections_rejected_max.get()
        ));
        output.push_str(&format!(
            "zeta4j_connections_rejected{{reason=\"backpressure\"}} {}\n",
            self.connections_rejected_backpressure.get()
        ));
        output.push_str(&format!(
            "zeta4j_connections_rejected{{reason=\"per_ip\"}} {}\n",
            self.connections_rejected_per_ip.get()
        ));

        output.push_str("# HELP zeta4j_active_connections Current active connections\n");
        output.push_str("# TYPE zeta4j_active_connections gauge\n");
        output.push_str(&format!(
            "zeta4j_active_connections {}\n",
            self.active_connections.get()
        ));

        output.push_str("# HELP zeta4j_connections_closed Total closed connections\n");
        output.push_str("# TYPE zeta4j_connections_closed counter\n");
        output.push_str(&format!(
            "zeta4j_connections_closed{{reason=\"normal\"}} {}\n",
            self.connections_closed_normal.get()
        ));
        output.push_str(&format!(
            "zeta4j_connections_closed{{reason=\"error\"}} {}\n",
            self.connections_closed_error.get()
        ));
        output.push_str(&format!(
            "zeta4j_connections_closed{{reason=\"timeout\"}} {}\n",
            self.connections_closed_timeout.get()
        ));

        // 처리량 메트릭
        output.push_str("# HELP zeta4j_bytes_read Total bytes read\n");
        output.push_str("# TYPE zeta4j_bytes_read counter\n");
        output.push_str(&format!("zeta4j_bytes_read {}\n", self.bytes_read.get()));

        output.push_str("# HELP zeta4j_bytes_written Total bytes written\n");
        output.push_str("# TYPE zeta4j_bytes_written counter\n");
        output.push_str(&format!(
            "zeta4j_bytes_written {}\n",
            self.bytes_written.get()
        ));

        output.push_str("# HELP zeta4j_requests_processed Total requests processed\n");
        output.push_str("# TYPE zeta4j_requests_processed counter\n");
        output.push_str(&format!(
            "zeta4j_requests_processed {}\n",
            self.requests_processed.get()
        ));

        // 에러 메트릭
        output.push_str("# HELP zeta4j_accept_errors Total accept errors\n");
        output.push_str("# TYPE zeta4j_accept_errors counter\n");
        output.push_str(&format!(
            "zeta4j_accept_errors {}\n",
            self.accept_errors.get()
        ));

        // 백프레셔 메트릭
        output.push_str("# HELP zeta4j_backpressure_triggered Backpressure triggered count\n");
        output.push_str("# TYPE zeta4j_backpressure_triggered counter\n");
        output.push_str(&format!(
            "zeta4j_backpressure_triggered {}\n",
            self.backpressure_triggered.get()
        ));

        // Uptime
        output.push_str("# HELP zeta4j_uptime_seconds Server uptime in seconds\n");
        output.push_str("# TYPE zeta4j_uptime_seconds gauge\n");
        output.push_str(&format!(
            "zeta4j_uptime_seconds {}\n",
            self.uptime_seconds.get()
        ));

        output
    }

    /// 모든 메트릭 리셋
    pub fn reset(&self) {
        self.connections_accepted.reset();
        self.connections_rejected_max.reset();
        self.connections_rejected_backpressure.reset();
        self.connections_rejected_per_ip.reset();
        self.connections_closed_normal.reset();
        self.connections_closed_error.reset();
        self.connections_closed_timeout.reset();
        self.active_connections.set(0);
        self.accept_errors.reset();
        self.accept_errors_fd_limit.reset();
        self.accept_latency.reset();
        self.bytes_read.reset();
        self.bytes_written.reset();
        self.requests_processed.reset();
        self.request_latency.reset();
        self.backpressure_triggered.reset();
        self.backpressure_released.reset();
        self.rate_limited.reset();
    }
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 카운터 (단조 증가)
pub struct Counter(AtomicU64);

impl Counter {
    /// 새 카운터 생성
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// 값 증가
    pub fn increment(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    /// 1 증가
    pub fn inc(&self) {
        self.increment(1);
    }

    /// 현재 값
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// 리셋
    pub fn reset(&self) {
        self.0.store(0, Ordering::Relaxed);
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

/// 게이지 (증감 가능)
pub struct Gauge(AtomicI64);

impl Gauge {
    /// 새 게이지 생성
    pub fn new() -> Self {
        Self(AtomicI64::new(0))
    }

    /// 값 설정
    pub fn set(&self, value: i64) {
        self.0.store(value, Ordering::Relaxed);
    }

    /// 값 증가
    pub fn increment(&self, n: i64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    /// 1 증가
    pub fn inc(&self) {
        self.increment(1);
    }

    /// 값 감소
    pub fn decrement(&self, n: i64) {
        self.0.fetch_sub(n, Ordering::Relaxed);
    }

    /// 1 감소
    pub fn dec(&self) {
        self.decrement(1);
    }

    /// 현재 값
    pub fn get(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

impl Default for Gauge {
    fn default() -> Self {
        Self::new()
    }
}

/// 히스토그램 (분포 측정)
pub struct Histogram {
    buckets: Vec<AtomicU64>,
    bucket_bounds: Vec<f64>,
    sum: AtomicU64,
    count: AtomicU64,
}

impl Histogram {
    /// 새 히스토그램 생성
    pub fn new() -> Self {
        // 기본 버킷: 0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
        let bucket_bounds = vec![
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ];
        let buckets = (0..=bucket_bounds.len())
            .map(|_| AtomicU64::new(0))
            .collect();

        Self {
            buckets,
            bucket_bounds,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// 커스텀 버킷으로 생성
    pub fn with_buckets(bounds: Vec<f64>) -> Self {
        let buckets = (0..=bounds.len()).map(|_| AtomicU64::new(0)).collect();

        Self {
            buckets,
            bucket_bounds: bounds,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// 값 관찰
    pub fn observe(&self, value: f64) {
        // 버킷 찾기
        let bucket_idx = self
            .bucket_bounds
            .iter()
            .position(|&b| value <= b)
            .unwrap_or(self.bucket_bounds.len());

        self.buckets[bucket_idx].fetch_add(1, Ordering::Relaxed);
        self.sum
            .fetch_add((value * 1_000_000.0) as u64, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// 백분위수 계산
    pub fn percentile(&self, p: f64) -> f64 {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }

        let target = ((count as f64) * p / 100.0).ceil() as u64;
        let mut cumulative = 0u64;

        for (i, bucket) in self.buckets.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if cumulative >= target {
                if i < self.bucket_bounds.len() {
                    return self.bucket_bounds[i];
                } else {
                    return self.bucket_bounds.last().copied().unwrap_or(10.0);
                }
            }
        }

        self.bucket_bounds.last().copied().unwrap_or(10.0)
    }

    /// 총합
    pub fn sum(&self) -> f64 {
        self.sum.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    /// 관찰 횟수
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// 평균
    pub fn mean(&self) -> f64 {
        let count = self.count();
        if count == 0 {
            return 0.0;
        }
        self.sum() / count as f64
    }

    /// 리셋
    pub fn reset(&self) {
        for bucket in &self.buckets {
            bucket.store(0, Ordering::Relaxed);
        }
        self.sum.store(0, Ordering::Relaxed);
        self.count.store(0, Ordering::Relaxed);
    }

    /// 버킷 경계값 반환
    pub fn bucket_bounds(&self) -> &[f64] {
        &self.bucket_bounds
    }

    /// 버킷별 누적 카운트 반환
    pub fn bucket_counts(&self) -> Vec<u64> {
        let mut cumulative = 0u64;
        self.buckets
            .iter()
            .map(|b| {
                cumulative += b.load(Ordering::Relaxed);
                cumulative
            })
            .collect()
    }

    /// Prometheus 히스토그램 포맷 출력
    pub fn to_prometheus(&self, name: &str, help: &str) -> String {
        let mut output = String::new();

        output.push_str(&format!("# HELP {} {}\n", name, help));
        output.push_str(&format!("# TYPE {} histogram\n", name));

        // 버킷별 누적 카운트
        let counts = self.bucket_counts();
        for (i, bound) in self.bucket_bounds.iter().enumerate() {
            output.push_str(&format!(
                "{}_bucket{{le=\"{}\"}} {}\n",
                name, bound, counts[i]
            ));
        }
        // +Inf 버킷
        output.push_str(&format!(
            "{}_bucket{{le=\"+Inf\"}} {}\n",
            name,
            counts.last().copied().unwrap_or(0)
        ));

        output.push_str(&format!("{}_sum {}\n", name, self.sum()));
        output.push_str(&format!("{}_count {}\n", name, self.count()));

        output
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}
