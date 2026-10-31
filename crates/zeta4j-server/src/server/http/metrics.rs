// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Metrics Collection (M2.3)
//!
//! HTTP 및 데이터베이스 메트릭 수집
//! Prometheus 및 JSON 포맷 지원

use prometheus::{
    Counter, CounterVec, Encoder, Gauge, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry,
    TextEncoder,
};
use std::sync::Arc;
use zeta4j_graph::storage::memory::MemoryStats;

/// HTTP 서버 메트릭
pub struct HttpMetrics {
    registry: Registry,

    // Request metrics
    pub requests_total: CounterVec,
    pub requests_duration: HistogramVec,
    pub requests_in_flight: Gauge,

    // Database metrics
    pub db_queries_total: CounterVec,
    pub db_transactions_total: CounterVec,
    pub db_transaction_duration: HistogramVec,

    // Connection metrics
    pub connections_active: Gauge,
    pub connections_total: Counter,

    // Error metrics
    pub errors_total: CounterVec,

    // Store metrics
    pub store_size_bytes: GaugeVec,
    pub node_count: GaugeVec,
    pub relationship_count: GaugeVec,

    // Database-scoped graph-engine memory ledger snapshots.
    pub engine_memory_bytes: GaugeVec,
    pub engine_memory_category_bytes: GaugeVec,
    pub engine_memory_admission_total: GaugeVec,
}

impl HttpMetrics {
    /// 새 메트릭 인스턴스 생성
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        // Request metrics
        let requests_total = CounterVec::new(
            Opts::new("zeta4j_requests_total", "Total HTTP requests"),
            &["method", "path", "status"],
        )?;

        let requests_duration = HistogramVec::new(
            HistogramOpts::new("zeta4j_requests_duration_seconds", "HTTP request duration")
                .buckets(vec![
                    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                ]),
            &["method", "path"],
        )?;

        let requests_in_flight = Gauge::new(
            "zeta4j_requests_in_flight",
            "Current number of HTTP requests being processed",
        )?;

        // Database metrics
        let db_queries_total = CounterVec::new(
            Opts::new("zeta4j_db_queries_total", "Total Cypher queries executed"),
            &["database", "type"],
        )?;

        let db_transactions_total = CounterVec::new(
            Opts::new(
                "zeta4j_db_transactions_total",
                "Total database transactions",
            ),
            &["database", "status"],
        )?;

        let db_transaction_duration = HistogramVec::new(
            HistogramOpts::new(
                "zeta4j_db_transaction_duration_seconds",
                "Database transaction duration",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0,
            ]),
            &["database"],
        )?;

        // Connection metrics
        let connections_active = Gauge::new(
            "zeta4j_connections_active",
            "Current number of active connections",
        )?;

        let connections_total = Counter::new(
            "zeta4j_connections_total",
            "Total number of connections accepted",
        )?;

        // Error metrics
        let errors_total = CounterVec::new(
            Opts::new("zeta4j_errors_total", "Total number of errors"),
            &["type", "code"],
        )?;

        // Store metrics
        let store_size_bytes = GaugeVec::new(
            Opts::new("zeta4j_store_size_bytes", "Database store size in bytes"),
            &["database"],
        )?;

        let node_count = GaugeVec::new(
            Opts::new("zeta4j_node_count", "Number of nodes in database"),
            &["database"],
        )?;

        let relationship_count = GaugeVec::new(
            Opts::new(
                "zeta4j_relationship_count",
                "Number of relationships in database",
            ),
            &["database"],
        )?;
        let engine_memory_bytes = GaugeVec::new(
            Opts::new(
                "zeta4j_engine_accounted_memory_bytes",
                "Database-scoped graph-engine accounted memory",
            ),
            &["kind"],
        )?;
        let engine_memory_category_bytes = GaugeVec::new(
            Opts::new(
                "zeta4j_engine_accounted_memory_category_bytes",
                "Graph-engine accounted memory by ownership category",
            ),
            &["category", "kind"],
        )?;
        let engine_memory_admission_total = GaugeVec::new(
            Opts::new(
                "zeta4j_engine_memory_admission_total",
                "Graph-engine memory admission and cleanup counters",
            ),
            &["kind"],
        )?;

        // Register all metrics
        registry.register(Box::new(requests_total.clone()))?;
        registry.register(Box::new(requests_duration.clone()))?;
        registry.register(Box::new(requests_in_flight.clone()))?;
        registry.register(Box::new(db_queries_total.clone()))?;
        registry.register(Box::new(db_transactions_total.clone()))?;
        registry.register(Box::new(db_transaction_duration.clone()))?;
        registry.register(Box::new(connections_active.clone()))?;
        registry.register(Box::new(connections_total.clone()))?;
        registry.register(Box::new(errors_total.clone()))?;
        registry.register(Box::new(store_size_bytes.clone()))?;
        registry.register(Box::new(node_count.clone()))?;
        registry.register(Box::new(relationship_count.clone()))?;
        registry.register(Box::new(engine_memory_bytes.clone()))?;
        registry.register(Box::new(engine_memory_category_bytes.clone()))?;
        registry.register(Box::new(engine_memory_admission_total.clone()))?;

        Ok(Self {
            registry,
            requests_total,
            requests_duration,
            requests_in_flight,
            db_queries_total,
            db_transactions_total,
            db_transaction_duration,
            connections_active,
            connections_total,
            errors_total,
            store_size_bytes,
            node_count,
            relationship_count,
            engine_memory_bytes,
            engine_memory_category_bytes,
            engine_memory_admission_total,
        })
    }

    /// Prometheus 텍스트 포맷으로 출력
    pub fn prometheus_format(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }

    /// JSON 포맷으로 출력
    pub fn json_format(&self) -> serde_json::Value {
        let metric_families = self.registry.gather();
        let mut metrics = serde_json::Map::new();

        for family in metric_families {
            let name = family.name();
            let mut family_data = Vec::new();

            for metric in family.get_metric() {
                let mut metric_data = serde_json::Map::new();

                // Labels
                let labels: serde_json::Map<String, serde_json::Value> = metric
                    .get_label()
                    .iter()
                    .map(|l| {
                        (
                            l.name().to_string(),
                            serde_json::Value::String(l.value().to_string()),
                        )
                    })
                    .collect();
                if !labels.is_empty() {
                    metric_data.insert("labels".to_string(), serde_json::Value::Object(labels));
                }

                // Value
                if let Some(counter) = metric.counter.as_ref() {
                    metric_data.insert("value".to_string(), serde_json::json!(counter.value()));
                } else if let Some(gauge) = metric.gauge.as_ref() {
                    metric_data.insert("value".to_string(), serde_json::json!(gauge.value()));
                } else if let Some(histogram) = metric.histogram.as_ref() {
                    metric_data.insert(
                        "count".to_string(),
                        serde_json::json!(histogram.sample_count()),
                    );
                    metric_data
                        .insert("sum".to_string(), serde_json::json!(histogram.sample_sum()));
                }

                family_data.push(serde_json::Value::Object(metric_data));
            }

            metrics.insert(name.to_string(), serde_json::Value::Array(family_data));
        }

        serde_json::Value::Object(metrics)
    }

    /// 요청 메트릭 기록
    pub fn record_request(&self, method: &str, path: &str, status: u16, duration_secs: f64) {
        self.requests_total
            .with_label_values(&[method, path, &status.to_string()])
            .inc();
        self.requests_duration
            .with_label_values(&[method, path])
            .observe(duration_secs);
    }

    /// 쿼리 메트릭 기록
    pub fn record_query(&self, database: &str, query_type: &str) {
        self.db_queries_total
            .with_label_values(&[database, query_type])
            .inc();
    }

    /// 트랜잭션 메트릭 기록
    pub fn record_transaction(&self, database: &str, status: &str, duration_secs: f64) {
        self.db_transactions_total
            .with_label_values(&[database, status])
            .inc();
        self.db_transaction_duration
            .with_label_values(&[database])
            .observe(duration_secs);
    }

    /// 에러 메트릭 기록
    pub fn record_error(&self, error_type: &str, code: &str) {
        self.errors_total
            .with_label_values(&[error_type, code])
            .inc();
    }

    /// 스토어 메트릭 업데이트
    pub fn update_store_metrics(&self, database: &str, size: u64, nodes: u64, relationships: u64) {
        self.store_size_bytes
            .with_label_values(&[database])
            .set(size as f64);
        self.node_count
            .with_label_values(&[database])
            .set(nodes as f64);
        self.relationship_count
            .with_label_values(&[database])
            .set(relationships as f64);
    }

    pub fn update_engine_memory_metrics(&self, stats: &MemoryStats) {
        self.engine_memory_bytes
            .with_label_values(&["current"])
            .set(stats.current_usage as f64);
        self.engine_memory_bytes
            .with_label_values(&["peak"])
            .set(stats.peak_usage as f64);
        self.engine_memory_bytes
            .with_label_values(&["limit"])
            .set(stats.max_allowed as f64);
        for category in &stats.categories {
            let Some(category) = category.category else {
                continue;
            };
            self.engine_memory_category_bytes
                .with_label_values(&[category.as_str(), "current"])
                .set(
                    stats
                        .category(category)
                        .map_or(0, |stats| stats.current_usage) as f64,
                );
            self.engine_memory_category_bytes
                .with_label_values(&[category.as_str(), "peak"])
                .set(stats.category(category).map_or(0, |stats| stats.peak_usage) as f64);
        }
        let counters = [
            ("rejected_reservations", stats.rejected_allocations as f64),
            ("rejected_bytes", stats.rejected_bytes as f64),
            ("cleanup_attempted", stats.auto_cleanup_count as f64),
            ("cleanup_succeeded", stats.cleanup_succeeded_count as f64),
            (
                "cleanup_reclaimed_bytes",
                stats.cleanup_reclaimed_bytes as f64,
            ),
        ];
        for (kind, value) in counters {
            self.engine_memory_admission_total
                .with_label_values(&[kind])
                .set(value);
        }
    }
}

/// Arc로 감싼 메트릭
pub type SharedMetrics = Arc<HttpMetrics>;

/// 메트릭 생성 헬퍼
pub fn create_metrics() -> Result<SharedMetrics, prometheus::Error> {
    HttpMetrics::new().map(Arc::new)
}
