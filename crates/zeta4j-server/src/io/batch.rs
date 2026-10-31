// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 배치 로딩 모듈 (Batch Loading)
//!
//! v0.53.0: 대용량 데이터 배치 처리 지원
//! v1.78.0: 비동기 CSV 로딩 지원
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::{BatchBuilder, BatchLoader, BatchOptions};
//!
//! // 스트리밍 방식
//! let mut batch = BatchBuilder::new(&engine);
//! batch.add_node(vec!["Person".into()], props);
//! batch.flush()?;
//!
//! // 파일 로딩 (동기)
//! let loader = BatchLoader::new(&engine);
//! loader.load_nodes_from_csv("nodes.csv")?;
//!
//! // 파일 로딩 (비동기 - v1.78.0)
//! let stats = load_nodes_from_csv_async(&engine, "nodes.csv", BatchOptions::default()).await?;
//! ```

use indexmap::IndexMap;

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, BufReader as TokioBufReader};

use serde::{Deserialize, Serialize};

use zeta4j_graph::core::PropertyContainer;
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::error::{GraphError, Result};

/// 배치 로딩 옵션
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOptions {
    /// 배치 크기 (한 번에 처리할 항목 수)
    pub batch_size: usize,
    /// 트랜잭션 사용 여부
    pub use_transaction: bool,
    /// 오류 발생 시 계속 진행
    pub continue_on_error: bool,
    /// 진행 콜백 간격 (0이면 비활성화)
    pub progress_interval: usize,
    /// CSV 구분자
    pub csv_delimiter: char,
    /// CSV 헤더 포함 여부
    pub csv_has_header: bool,
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            batch_size: 10_000,
            use_transaction: true,
            continue_on_error: false,
            progress_interval: 0,
            csv_delimiter: ',',
            csv_has_header: true,
        }
    }
}

impl BatchOptions {
    /// 고성능 모드 (대용량 최적화)
    pub fn high_performance() -> Self {
        Self {
            batch_size: 50_000,
            use_transaction: false, // 트랜잭션 오버헤드 제거
            continue_on_error: false,
            progress_interval: 100_000,
            csv_delimiter: ',',
            csv_has_header: true,
        }
    }

    /// 안전 모드 (작은 배치, 트랜잭션 사용)
    pub fn safe() -> Self {
        Self {
            batch_size: 1_000,
            use_transaction: true,
            continue_on_error: true,
            progress_interval: 1_000,
            csv_delimiter: ',',
            csv_has_header: true,
        }
    }

    /// 배치 크기 설정
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// 진행 콜백 간격 설정
    pub fn with_progress_interval(mut self, interval: usize) -> Self {
        self.progress_interval = interval;
        self
    }
}

/// 배치 로딩 통계
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchStats {
    /// 처리된 총 노드 수
    pub total_nodes: usize,
    /// 처리된 총 엣지 수
    pub total_edges: usize,
    /// 성공한 노드 수
    pub success_nodes: usize,
    /// 성공한 엣지 수
    pub success_edges: usize,
    /// 실패한 노드 수
    pub failed_nodes: usize,
    /// 실패한 엣지 수
    pub failed_edges: usize,
    /// 경과 시간 (밀리초)
    pub elapsed_ms: u64,
    /// 초당 처리량 (ops/sec)
    pub throughput: f64,
}

impl BatchStats {
    /// 통계 병합
    pub fn merge(&mut self, other: &BatchStats) {
        self.total_nodes += other.total_nodes;
        self.total_edges += other.total_edges;
        self.success_nodes += other.success_nodes;
        self.success_edges += other.success_edges;
        self.failed_nodes += other.failed_nodes;
        self.failed_edges += other.failed_edges;
    }

    /// 처리량 계산
    pub fn calculate_throughput(&mut self) {
        if self.elapsed_ms > 0 {
            let total_ops = (self.success_nodes + self.success_edges) as f64;
            self.throughput = total_ops / (self.elapsed_ms as f64 / 1000.0);
        }
    }
}

/// 대기 중인 노드 데이터
#[derive(Debug, Clone)]
pub struct PendingNode {
    /// 라벨 목록
    pub labels: Vec<String>,
    /// 프로퍼티 맵
    pub properties: IndexMap<String, Value>,
}

impl PendingNode {
    /// 새로운 대기 노드 생성
    pub fn new(labels: Vec<String>) -> Self {
        Self {
            labels,
            properties: IndexMap::new(),
        }
    }

    /// 프로퍼티와 함께 생성
    pub fn with_properties(labels: Vec<String>, properties: IndexMap<String, Value>) -> Self {
        Self { labels, properties }
    }
}

/// 대기 중인 엣지 데이터
#[derive(Debug, Clone)]
pub struct PendingEdge {
    /// 관계 타입
    pub rel_type: String,
    /// 시작 노드 ID
    pub from_id: u64,
    /// 끝 노드 ID
    pub to_id: u64,
    /// 프로퍼티 맵
    pub properties: IndexMap<String, Value>,
}

impl PendingEdge {
    /// 새로운 대기 엣지 생성
    pub fn new(rel_type: String, from_id: u64, to_id: u64) -> Self {
        Self {
            rel_type,
            from_id,
            to_id,
            properties: IndexMap::new(),
        }
    }

    /// 프로퍼티와 함께 생성
    pub fn with_properties(
        rel_type: String,
        from_id: u64,
        to_id: u64,
        properties: IndexMap<String, Value>,
    ) -> Self {
        Self {
            rel_type,
            from_id,
            to_id,
            properties,
        }
    }
}

/// 배치 빌더 (스트리밍 방식 배치 처리)
pub struct BatchBuilder<'a> {
    engine: &'a GraphEngine,
    pending_nodes: Vec<PendingNode>,
    pending_edges: Vec<PendingEdge>,
    options: BatchOptions,
    stats: BatchStats,
    start_time: Instant,
    progress_callback: Option<Box<dyn Fn(usize, usize) + 'a>>,
}

impl<'a> BatchBuilder<'a> {
    /// 새로운 배치 빌더 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self::with_options(engine, BatchOptions::default())
    }

    /// 옵션과 함께 배치 빌더 생성
    pub fn with_options(engine: &'a GraphEngine, options: BatchOptions) -> Self {
        Self {
            engine,
            pending_nodes: Vec::with_capacity(options.batch_size),
            pending_edges: Vec::with_capacity(options.batch_size),
            options,
            stats: BatchStats::default(),
            start_time: Instant::now(),
            progress_callback: None,
        }
    }

    /// 진행 콜백 설정
    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize, usize) + 'a,
    {
        self.progress_callback = Some(Box::new(callback));
        self
    }

    /// 노드 추가 (버퍼링)
    pub async fn add_node(&mut self, labels: Vec<String>, properties: IndexMap<String, Value>) {
        self.pending_nodes
            .push(PendingNode::with_properties(labels, properties));
        self.stats.total_nodes += 1;

        // 자동 플러시
        if self.pending_nodes.len() >= self.options.batch_size {
            let _ = self.flush().await;
        }
    }

    /// 엣지 추가 (버퍼링)
    pub async fn add_edge(
        &mut self,
        rel_type: String,
        from_id: u64,
        to_id: u64,
        properties: IndexMap<String, Value>,
    ) {
        self.pending_edges.push(PendingEdge::with_properties(
            rel_type, from_id, to_id, properties,
        ));
        self.stats.total_edges += 1;

        // 자동 플러시
        if self.pending_edges.len() >= self.options.batch_size {
            let _ = self.flush().await;
        }
    }

    /// 현재 버퍼 크기
    pub fn size(&self) -> usize {
        self.pending_nodes.len() + self.pending_edges.len()
    }

    /// 버퍼 플러시 (DB 반영)
    pub async fn flush(&mut self) -> Result<BatchStats> {
        let flush_start = Instant::now();
        let mut flush_stats = BatchStats::default();

        // 노드 플러시
        if !self.pending_nodes.is_empty() {
            flush_stats.merge(&self.flush_nodes().await?);
        }

        // 엣지 플러시
        if !self.pending_edges.is_empty() {
            flush_stats.merge(&self.flush_edges().await?);
        }

        flush_stats.elapsed_ms = flush_start.elapsed().as_millis() as u64;

        // 진행 콜백 호출
        if let Some(ref callback) = self.progress_callback {
            let total = self.stats.success_nodes + self.stats.success_edges;
            let target = self.stats.total_nodes + self.stats.total_edges;
            callback(total, target);
        }

        Ok(flush_stats)
    }

    /// 노드 플러시
    async fn flush_nodes(&mut self) -> Result<BatchStats> {
        let mut stats = BatchStats::default();
        let nodes = std::mem::take(&mut self.pending_nodes);
        stats.total_nodes = nodes.len();

        if self.options.use_transaction {
            // 트랜잭션 모드: 하나의 트랜잭션으로 모든 노드 처리
            let tx_id = self.engine.begin_transaction()?;

            let mut created_ids = Vec::with_capacity(nodes.len());
            for node in &nodes {
                match self
                    .engine
                    .create_node_internal(tx_id, node.labels.clone())
                    .await
                {
                    Ok(id) => created_ids.push((id, node)),
                    Err(e) => {
                        if self.options.continue_on_error {
                            stats.failed_nodes += 1;
                        } else {
                            self.engine.rollback_transaction(tx_id)?;
                            return Err(e);
                        }
                    }
                }
            }

            // 프로퍼티 설정
            for (id, node) in &created_ids {
                if !node.properties.is_empty() {
                    let props = node.properties.clone();
                    if let Err(e) = self
                        .engine
                        .update_node_internal(tx_id, *id, |n| {
                            for (key, value) in props {
                                n.set_property(&key, value);
                            }
                        })
                        .await
                        && !self.options.continue_on_error
                    {
                        self.engine.rollback_transaction(tx_id)?;
                        return Err(e);
                    }
                }
            }

            self.engine.commit_transaction(tx_id)?;
            stats.success_nodes = created_ids.len();
        } else {
            // 비트랜잭션 모드: 개별 처리 (고성능)
            // 라벨만으로 배치 생성
            let labels_list: Vec<Vec<String>> = nodes.iter().map(|n| n.labels.clone()).collect();
            let created_ids = self.engine.batch_create_nodes(labels_list).await?;

            // 프로퍼티 설정 (프로퍼티가 있는 노드만)
            for (id, node) in created_ids.iter().zip(nodes.iter()) {
                if !node.properties.is_empty() {
                    let props = node.properties.clone();
                    if let Err(e) = self
                        .engine
                        .update_node(*id, |n| {
                            for (key, value) in props {
                                n.set_property(&key, value);
                            }
                        })
                        .await
                    {
                        if !self.options.continue_on_error {
                            return Err(e);
                        }
                        stats.failed_nodes += 1;
                    }
                }
            }

            stats.success_nodes = created_ids.len();
        }

        self.stats.success_nodes += stats.success_nodes;
        self.stats.failed_nodes += stats.failed_nodes;

        Ok(stats)
    }

    /// 엣지 플러시
    async fn flush_edges(&mut self) -> Result<BatchStats> {
        let mut stats = BatchStats::default();
        let edges = std::mem::take(&mut self.pending_edges);
        stats.total_edges = edges.len();

        for edge in edges {
            match self
                .engine
                .create_edge(edge.rel_type.clone(), edge.from_id, edge.to_id)
                .await
            {
                Ok(edge_id) => {
                    stats.success_edges += 1;

                    // 프로퍼티 설정
                    if !edge.properties.is_empty() {
                        let props = edge.properties.clone();
                        if let Err(e) = self
                            .engine
                            .update_edge(edge_id, |e| {
                                for (key, value) in props {
                                    e.set_property(&key, value);
                                }
                            })
                            .await
                            && !self.options.continue_on_error
                        {
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    if self.options.continue_on_error {
                        stats.failed_edges += 1;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        self.stats.success_edges += stats.success_edges;
        self.stats.failed_edges += stats.failed_edges;

        Ok(stats)
    }

    /// 완료 (최종 플러시)
    pub async fn finish(mut self) -> Result<BatchStats> {
        // 남은 데이터 플러시
        self.flush().await?;

        // 최종 통계 계산
        self.stats.elapsed_ms = self.start_time.elapsed().as_millis() as u64;
        self.stats.calculate_throughput();

        Ok(self.stats)
    }

    /// 현재 통계 조회
    pub fn stats(&self) -> &BatchStats {
        &self.stats
    }
}

/// 배치 로더 (파일 기반 배치 로딩)
pub struct BatchLoader<'a> {
    engine: &'a GraphEngine,
    options: BatchOptions,
}

impl<'a> BatchLoader<'a> {
    /// 새로운 배치 로더 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            options: BatchOptions::default(),
        }
    }

    /// 옵션 설정
    pub fn with_options(mut self, options: BatchOptions) -> Self {
        self.options = options;
        self
    }

    /// 배치 크기 설정
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.options.batch_size = size;
        self
    }

    /// 이터레이터에서 노드 로딩
    pub async fn load_nodes<I>(&self, nodes: I) -> Result<BatchStats>
    where
        I: Iterator<Item = PendingNode>,
    {
        let mut batch = BatchBuilder::with_options(self.engine, self.options.clone());

        for node in nodes {
            batch.add_node(node.labels, node.properties).await;
        }

        batch.finish().await
    }

    /// 이터레이터에서 엣지 로딩
    pub async fn load_edges<I>(&self, edges: I) -> Result<BatchStats>
    where
        I: Iterator<Item = PendingEdge>,
    {
        let mut batch = BatchBuilder::with_options(self.engine, self.options.clone());

        for edge in edges {
            batch
                .add_edge(edge.rel_type, edge.from_id, edge.to_id, edge.properties)
                .await;
        }

        batch.finish().await
    }

    /// CSV 파일에서 노드 로딩
    ///
    /// CSV 형식:
    /// - 첫 번째 열: 라벨 (세미콜론으로 구분된 다중 라벨)
    /// - 나머지 열: 프로퍼티 (헤더에서 키 이름 추출)
    pub async fn load_nodes_from_csv<P: AsRef<Path>>(&self, path: P) -> Result<BatchStats> {
        let file = File::open(path.as_ref())?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        // 헤더 파싱
        let header_line = lines.next().ok_or_else(|| GraphError::Parse {
            message: "Empty CSV file".to_string(),
        })??;
        let headers: Vec<&str> = header_line.split(self.options.csv_delimiter).collect();

        if headers.is_empty() {
            return Err(GraphError::Parse {
                message: "No columns in CSV".to_string(),
            });
        }

        let mut batch = BatchBuilder::with_options(self.engine, self.options.clone());

        for line_result in lines {
            let line = line_result?;
            let values: Vec<&str> = line.split(self.options.csv_delimiter).collect();

            if values.is_empty() {
                continue;
            }

            // 첫 번째 열은 라벨
            let labels: Vec<String> = values[0]
                .split(';')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            // 나머지 열은 프로퍼티
            let mut properties = IndexMap::new();
            for (i, value) in values.iter().skip(1).enumerate() {
                if i + 1 < headers.len() && !value.is_empty() {
                    let key = headers[i + 1].trim().to_string();
                    let parsed_value = parse_csv_value(value);
                    properties.insert(key, parsed_value);
                }
            }

            batch.add_node(labels, properties).await;
        }

        batch.finish().await
    }

    /// CSV 파일에서 엣지 로딩
    ///
    /// CSV 형식:
    /// - 첫 번째 열: 관계 타입
    /// - 두 번째 열: 시작 노드 ID
    /// - 세 번째 열: 끝 노드 ID
    /// - 나머지 열: 프로퍼티
    pub async fn load_edges_from_csv<P: AsRef<Path>>(&self, path: P) -> Result<BatchStats> {
        let file = File::open(path.as_ref())?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        // 헤더 파싱
        let header_line = lines.next().ok_or_else(|| GraphError::Parse {
            message: "Empty CSV file".to_string(),
        })??;
        let headers: Vec<&str> = header_line.split(self.options.csv_delimiter).collect();

        if headers.len() < 3 {
            return Err(GraphError::Parse {
                message: "CSV must have at least 3 columns: type, from_id, to_id".to_string(),
            });
        }

        let mut batch = BatchBuilder::with_options(self.engine, self.options.clone());

        for line_result in lines {
            let line = line_result?;
            let values: Vec<&str> = line.split(self.options.csv_delimiter).collect();

            if values.len() < 3 {
                continue;
            }

            let rel_type = values[0].trim().to_string();
            let from_id: u64 = values[1].trim().parse().map_err(|_| GraphError::Parse {
                message: format!("Invalid from_id: {}", values[1]),
            })?;
            let to_id: u64 = values[2].trim().parse().map_err(|_| GraphError::Parse {
                message: format!("Invalid to_id: {}", values[2]),
            })?;

            // 나머지 열은 프로퍼티
            let mut properties = IndexMap::new();
            for (i, value) in values.iter().skip(3).enumerate() {
                if i + 3 < headers.len() && !value.is_empty() {
                    let key = headers[i + 3].trim().to_string();
                    let parsed_value = parse_csv_value(value);
                    properties.insert(key, parsed_value);
                }
            }

            batch.add_edge(rel_type, from_id, to_id, properties).await;
        }

        batch.finish().await
    }

    /// JSON 파일에서 그래프 로딩 (기존 JsonImporter 활용)
    pub async fn load_from_json<P: AsRef<Path>>(&self, path: P) -> Result<BatchStats> {
        use crate::io::JsonImporter;

        let start = Instant::now();
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| GraphError::Validation {
                message: "JSON import path is not valid UTF-8".to_string(),
            })?;
        let import_stats = JsonImporter::import_graph(self.engine, path).await?;

        Ok(BatchStats {
            total_nodes: import_stats.node_count,
            total_edges: import_stats.edge_count,
            success_nodes: import_stats.node_count,
            success_edges: import_stats.edge_count,
            failed_nodes: 0,
            failed_edges: 0,
            elapsed_ms: start.elapsed().as_millis() as u64,
            throughput: 0.0, // JsonImporter 내부 처리라 정확한 처리량 계산 어려움
        })
    }

    /// GraphML 파일에서 그래프 로딩 (기존 GraphMLImporter 활용)
    pub async fn load_from_graphml<P: AsRef<Path>>(&self, path: P) -> Result<BatchStats> {
        use crate::io::GraphMLImporter;

        let start = Instant::now();
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| GraphError::Validation {
                message: "GraphML import path is not valid UTF-8".to_string(),
            })?;
        let import_stats = GraphMLImporter::import_graph(self.engine, path).await?;

        Ok(BatchStats {
            total_nodes: import_stats.node_count,
            total_edges: import_stats.edge_count,
            success_nodes: import_stats.node_count,
            success_edges: import_stats.edge_count,
            failed_nodes: 0,
            failed_edges: 0,
            elapsed_ms: start.elapsed().as_millis() as u64,
            throughput: 0.0,
        })
    }
}

/// CSV 값 파싱
fn parse_csv_value(value: &str) -> Value {
    let trimmed = value.trim();

    // Boolean 체크
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Boolean(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Boolean(false);
    }

    // Integer 체크
    if let Ok(i) = trimmed.parse::<i64>() {
        return Value::Integer(i);
    }

    // Float 체크
    if let Ok(f) = trimmed.parse::<f64>() {
        return Value::Float(f);
    }

    // 기본: String
    Value::String(trimmed.to_string())
}

// ============================================================================
// 비동기 배치 로딩 함수 (v1.78.0)
// ============================================================================

/// CSV 파일에서 노드를 비동기로 로딩
///
/// 대용량 CSV 파일을 비동기로 읽어 서버 응답성을 유지합니다.
///
/// # Arguments
/// * `engine` - GraphEngine 참조
/// * `path` - CSV 파일 경로
/// * `options` - 배치 옵션
///
/// # CSV 형식
/// - 첫 번째 열: 라벨 (세미콜론으로 구분된 다중 라벨)
/// - 나머지 열: 프로퍼티 (헤더에서 키 이름 추출)
pub async fn load_nodes_from_csv_async<P: AsRef<Path>>(
    engine: Arc<GraphEngine>,
    path: P,
    options: BatchOptions,
) -> Result<BatchStats> {
    let file = tokio::fs::File::open(path.as_ref()).await?;
    let reader = TokioBufReader::new(file);
    let mut lines = reader.lines();

    // 헤더 파싱
    let header_line = lines.next_line().await?.ok_or_else(|| GraphError::Parse {
        message: "Empty CSV file".to_string(),
    })?;
    let headers: Vec<&str> = header_line.split(options.csv_delimiter).collect();

    if headers.is_empty() {
        return Err(GraphError::Parse {
            message: "No columns in CSV".to_string(),
        });
    }

    let start = Instant::now();
    let mut stats = BatchStats::default();
    let mut pending_nodes: Vec<PendingNode> = Vec::with_capacity(options.batch_size);

    // 라인별 처리
    while let Some(line) = lines.next_line().await? {
        let values: Vec<&str> = line.split(options.csv_delimiter).collect();

        if values.is_empty() {
            continue;
        }

        // 첫 번째 열은 라벨
        let labels: Vec<String> = values[0]
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // 나머지 열은 프로퍼티
        let mut properties = IndexMap::new();
        for (i, value) in values.iter().skip(1).enumerate() {
            if i + 1 < headers.len() && !value.is_empty() {
                let key = headers[i + 1].trim().to_string();
                let parsed_value = parse_csv_value(value);
                properties.insert(key, parsed_value);
            }
        }

        pending_nodes.push(PendingNode::with_properties(labels, properties));
        stats.total_nodes += 1;

        // 배치 크기에 도달하면 플러시
        if pending_nodes.len() >= options.batch_size {
            let batch_stats = flush_nodes_batch(&engine, &mut pending_nodes, &options).await?;
            stats.success_nodes += batch_stats.success_nodes;
            stats.failed_nodes += batch_stats.failed_nodes;
        }
    }

    // 남은 노드 플러시
    if !pending_nodes.is_empty() {
        let batch_stats = flush_nodes_batch(&engine, &mut pending_nodes, &options).await?;
        stats.success_nodes += batch_stats.success_nodes;
        stats.failed_nodes += batch_stats.failed_nodes;
    }

    stats.elapsed_ms = start.elapsed().as_millis() as u64;
    stats.calculate_throughput();

    Ok(stats)
}

/// CSV 파일에서 엣지를 비동기로 로딩
///
/// # CSV 형식
/// - 첫 번째 열: 관계 타입
/// - 두 번째 열: 시작 노드 ID
/// - 세 번째 열: 끝 노드 ID
/// - 나머지 열: 프로퍼티
pub async fn load_edges_from_csv_async<P: AsRef<Path>>(
    engine: Arc<GraphEngine>,
    path: P,
    options: BatchOptions,
) -> Result<BatchStats> {
    let file = tokio::fs::File::open(path.as_ref()).await?;
    let reader = TokioBufReader::new(file);
    let mut lines = reader.lines();

    // 헤더 파싱
    let header_line = lines.next_line().await?.ok_or_else(|| GraphError::Parse {
        message: "Empty CSV file".to_string(),
    })?;
    let headers: Vec<&str> = header_line.split(options.csv_delimiter).collect();

    if headers.len() < 3 {
        return Err(GraphError::Parse {
            message: "CSV must have at least 3 columns: type, from_id, to_id".to_string(),
        });
    }

    let start = Instant::now();
    let mut stats = BatchStats::default();
    let mut pending_edges: Vec<PendingEdge> = Vec::with_capacity(options.batch_size);

    while let Some(line) = lines.next_line().await? {
        let values: Vec<&str> = line.split(options.csv_delimiter).collect();

        if values.len() < 3 {
            continue;
        }

        let rel_type = values[0].trim().to_string();
        let from_id: u64 = match values[1].trim().parse() {
            Ok(id) => id,
            Err(_) => {
                if options.continue_on_error {
                    stats.failed_edges += 1;
                    continue;
                }
                return Err(GraphError::Parse {
                    message: format!("Invalid from_id: {}", values[1]),
                });
            }
        };
        let to_id: u64 = match values[2].trim().parse() {
            Ok(id) => id,
            Err(_) => {
                if options.continue_on_error {
                    stats.failed_edges += 1;
                    continue;
                }
                return Err(GraphError::Parse {
                    message: format!("Invalid to_id: {}", values[2]),
                });
            }
        };

        // 나머지 열은 프로퍼티
        let mut properties = IndexMap::new();
        for (i, value) in values.iter().skip(3).enumerate() {
            if i + 3 < headers.len() && !value.is_empty() {
                let key = headers[i + 3].trim().to_string();
                let parsed_value = parse_csv_value(value);
                properties.insert(key, parsed_value);
            }
        }

        pending_edges.push(PendingEdge::with_properties(
            rel_type, from_id, to_id, properties,
        ));
        stats.total_edges += 1;

        // 배치 크기에 도달하면 플러시
        if pending_edges.len() >= options.batch_size {
            let batch_stats = flush_edges_batch(&engine, &mut pending_edges, &options).await?;
            stats.success_edges += batch_stats.success_edges;
            stats.failed_edges += batch_stats.failed_edges;
        }
    }

    // 남은 엣지 플러시
    if !pending_edges.is_empty() {
        let batch_stats = flush_edges_batch(&engine, &mut pending_edges, &options).await?;
        stats.success_edges += batch_stats.success_edges;
        stats.failed_edges += batch_stats.failed_edges;
    }

    stats.elapsed_ms = start.elapsed().as_millis() as u64;
    stats.calculate_throughput();

    Ok(stats)
}

/// 노드 배치 플러시 (내부 함수)
async fn flush_nodes_batch(
    engine: &GraphEngine,
    nodes: &mut Vec<PendingNode>,
    options: &BatchOptions,
) -> Result<BatchStats> {
    let mut stats = BatchStats::default();
    let batch = std::mem::take(nodes);
    stats.total_nodes = batch.len();

    if options.use_transaction {
        let tx_id = engine.begin_transaction()?;

        let mut created_ids = Vec::with_capacity(batch.len());
        for node in &batch {
            match engine
                .create_node_internal(tx_id, node.labels.clone())
                .await
            {
                Ok(id) => created_ids.push((id, node)),
                Err(e) => {
                    if options.continue_on_error {
                        stats.failed_nodes += 1;
                    } else {
                        engine.rollback_transaction(tx_id)?;
                        return Err(e);
                    }
                }
            }
        }

        // 프로퍼티 설정
        for (id, node) in &created_ids {
            if !node.properties.is_empty() {
                let props = node.properties.clone();
                if let Err(e) = engine
                    .update_node_internal(tx_id, *id, |n| {
                        for (key, value) in props {
                            n.set_property(&key, value);
                        }
                    })
                    .await
                    && !options.continue_on_error
                {
                    engine.rollback_transaction(tx_id)?;
                    return Err(e);
                }
            }
        }

        engine.commit_transaction(tx_id)?;
        stats.success_nodes = created_ids.len();
    } else {
        // 비트랜잭션 모드
        let labels_list: Vec<Vec<String>> = batch.iter().map(|n| n.labels.clone()).collect();
        let created_ids = engine.batch_create_nodes(labels_list).await?;

        for (id, node) in created_ids.iter().zip(batch.iter()) {
            if !node.properties.is_empty() {
                let props = node.properties.clone();
                if let Err(_e) = engine
                    .update_node(*id, |n| {
                        for (key, value) in props {
                            n.set_property(&key, value);
                        }
                    })
                    .await
                    && !options.continue_on_error
                {
                    stats.failed_nodes += 1;
                }
            }
        }

        stats.success_nodes = created_ids.len();
    }

    Ok(stats)
}

/// 엣지 배치 플러시 (내부 함수)
async fn flush_edges_batch(
    engine: &GraphEngine,
    edges: &mut Vec<PendingEdge>,
    options: &BatchOptions,
) -> Result<BatchStats> {
    let mut stats = BatchStats::default();
    let batch = std::mem::take(edges);
    stats.total_edges = batch.len();

    for edge in batch {
        match engine
            .create_edge(edge.rel_type.clone(), edge.from_id, edge.to_id)
            .await
        {
            Ok(edge_id) => {
                stats.success_edges += 1;

                if !edge.properties.is_empty() {
                    let props = edge.properties.clone();
                    if let Err(_e) = engine
                        .update_edge(edge_id, |e| {
                            for (key, value) in props {
                                e.set_property(&key, value);
                            }
                        })
                        .await
                        && !options.continue_on_error
                    {
                        // 프로퍼티 설정 실패는 경고만
                    }
                }
            }
            Err(_e) => {
                stats.failed_edges += 1;
            }
        }
    }

    Ok(stats)
}
