//! 스트리밍 알고리즘 설정
//!
//! 청크 크기, 진행 콜백, 메모리 제한 등 공통 설정

use std::sync::Arc;

/// 진행 상황 콜백 함수 타입
///
/// 매개변수: (진행률 0.0-1.0, 현재 단계, 처리된 항목 수)
pub type ProgressCallback = Arc<dyn Fn(f64, AlgorithmPhase, usize) + Send + Sync>;

/// 알고리즘 실행 단계
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlgorithmPhase {
    /// 그래프 구조 수집
    CollectingGraph,
    /// 노드 처리 중
    ProcessingNodes,
    /// 엣지 처리 중
    ProcessingEdges,
    /// 결과 계산 중
    Computing,
    /// 결과 정규화/후처리
    Finalizing,
}

/// 메모리 초과 시 대응 전략
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryFallback {
    /// 에러 반환 (기본값)
    #[default]
    Error,
    /// 샘플링으로 근사 계산
    Approximate,
    /// 청크 기반 처리
    Chunked,
}

impl std::fmt::Display for AlgorithmPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlgorithmPhase::CollectingGraph => write!(f, "Collecting graph"),
            AlgorithmPhase::ProcessingNodes => write!(f, "Processing nodes"),
            AlgorithmPhase::ProcessingEdges => write!(f, "Processing edges"),
            AlgorithmPhase::Computing => write!(f, "Computing"),
            AlgorithmPhase::Finalizing => write!(f, "Finalizing"),
        }
    }
}

/// 청크 진행 정보
#[derive(Debug, Clone)]
pub struct ChunkProgress {
    /// 현재 청크 번호 (0-based)
    pub current_chunk: usize,
    /// 전체 청크 수
    pub total_chunks: usize,
    /// 현재 청크에서 처리된 항목 수
    pub items_in_chunk: usize,
    /// 전체 처리된 항목 수
    pub total_items_processed: usize,
    /// 전체 항목 수
    pub total_items: usize,
}

impl ChunkProgress {
    /// 진행률 (0.0 - 1.0)
    pub fn progress(&self) -> f64 {
        if self.total_items == 0 {
            1.0
        } else {
            self.total_items_processed as f64 / self.total_items as f64
        }
    }
}

/// 스트리밍 통계
#[derive(Debug, Clone, Default)]
pub struct StreamingStats {
    /// 처리된 노드 수
    pub nodes_processed: usize,
    /// 처리된 엣지 수
    pub edges_processed: usize,
    /// 사용된 청크 수
    pub chunks_used: usize,
    /// 총 실행 시간 (밀리초)
    pub elapsed_ms: u64,
    /// 최대 메모리 사용량 (바이트, 추정)
    pub peak_memory_bytes: usize,
}

/// 스트리밍 알고리즘 설정
#[derive(Clone)]
pub struct StreamingConfig {
    /// 청크 크기 (한 번에 처리할 노드/항목 수)
    pub chunk_size: usize,
    /// 진행 상황 콜백
    pub progress_callback: Option<ProgressCallback>,
    /// 메모리 제한 (바이트, None이면 무제한)
    pub memory_limit: Option<usize>,
    /// 조기 종료 조건 (true 반환 시 중단)
    pub early_stop: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    /// 결과 버퍼 크기 (Iterator용)
    pub result_buffer_size: usize,
    /// 메모리 제한 강제 여부
    pub enforce_memory_limit: bool,
    /// 메모리 초과 시 대응 전략
    pub memory_fallback: MemoryFallback,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            chunk_size: 10000,
            progress_callback: None,
            memory_limit: None,
            early_stop: None,
            result_buffer_size: 1000,
            enforce_memory_limit: false,
            memory_fallback: MemoryFallback::Error,
        }
    }
}

impl StreamingConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 청크 크기 설정
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size.max(1);
        self
    }

    /// 진행 상황 콜백 설정
    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(f64, AlgorithmPhase, usize) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// 메모리 제한 설정 (바이트)
    pub fn with_memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = Some(limit);
        self
    }

    /// 조기 종료 조건 설정
    pub fn with_early_stop<F>(mut self, condition: F) -> Self
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        self.early_stop = Some(Arc::new(condition));
        self
    }

    /// 결과 버퍼 크기 설정
    pub fn with_result_buffer_size(mut self, size: usize) -> Self {
        self.result_buffer_size = size.max(1);
        self
    }

    /// 메모리 제한 강제 설정
    ///
    /// 활성화하면 그래프 구조 로딩 전에 메모리 예측을 수행하고,
    /// 제한 초과 시 fallback 전략에 따라 처리합니다.
    pub fn with_enforce_memory_limit(mut self, enforce: bool) -> Self {
        self.enforce_memory_limit = enforce;
        self
    }

    /// 메모리 초과 시 대응 전략 설정
    pub fn with_memory_fallback(mut self, fallback: MemoryFallback) -> Self {
        self.memory_fallback = fallback;
        self
    }

    /// 진행 상황 보고
    pub(crate) fn report_progress(&self, progress: f64, phase: AlgorithmPhase, items: usize) {
        if let Some(ref callback) = self.progress_callback {
            callback(progress, phase, items);
        }
    }

    /// 조기 종료 체크
    pub(crate) fn should_stop(&self) -> bool {
        self.early_stop.as_ref().is_some_and(|f| f())
    }

    /// 메모리 예측 (그래프 구조 기반)
    ///
    /// 노드/엣지 수를 기반으로 예상 메모리 사용량을 계산합니다.
    /// 반환값은 (예상 메모리 바이트, 제한 초과 여부)입니다.
    pub fn estimate_memory(&self, node_count: usize, edge_count: usize) -> (usize, bool) {
        // HashMap 오버헤드 포함 노드당 약 64바이트 (NodeId + Vec overhead)
        // 엣지당 약 24바이트 (from, to, type ref)
        const BYTES_PER_NODE: usize = 64;
        const BYTES_PER_EDGE: usize = 24;

        let estimated = (node_count * BYTES_PER_NODE) + (edge_count * BYTES_PER_EDGE);

        let exceeds = self
            .memory_limit
            .map(|limit| estimated > limit)
            .unwrap_or(false);

        (estimated, exceeds)
    }

    /// 메모리 제한 체크 및 대응
    ///
    /// # Returns
    ///
    /// - `Ok(None)`: 제한 미초과 또는 미설정
    /// - `Ok(Some(fallback))`: 제한 초과, fallback 전략 반환
    /// - `Err`: 제한 초과 + fallback이 Error인 경우
    pub fn check_memory_limit(
        &self,
        node_count: usize,
        edge_count: usize,
    ) -> Result<Option<MemoryFallback>, MemoryLimitExceeded> {
        if !self.enforce_memory_limit {
            return Ok(None);
        }

        let (estimated, exceeds) = self.estimate_memory(node_count, edge_count);

        if !exceeds {
            return Ok(None);
        }

        let limit = self.memory_limit.unwrap_or(0);

        match self.memory_fallback {
            MemoryFallback::Error => Err(MemoryLimitExceeded {
                estimated,
                limit,
                node_count,
                edge_count,
            }),
            fallback => Ok(Some(fallback)),
        }
    }
}

/// 메모리 제한 초과 에러
#[derive(Debug, Clone)]
pub struct MemoryLimitExceeded {
    /// 예상 메모리 사용량 (바이트)
    pub estimated: usize,
    /// 메모리 제한 (바이트)
    pub limit: usize,
    /// 노드 수
    pub node_count: usize,
    /// 엣지 수
    pub edge_count: usize,
}

impl std::fmt::Display for MemoryLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Memory limit exceeded: estimated {}MB > limit {}MB ({} nodes, {} edges)",
            self.estimated / (1024 * 1024),
            self.limit / (1024 * 1024),
            self.node_count,
            self.edge_count
        )
    }
}

impl std::error::Error for MemoryLimitExceeded {}

impl std::fmt::Debug for StreamingConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingConfig")
            .field("chunk_size", &self.chunk_size)
            .field("has_progress_callback", &self.progress_callback.is_some())
            .field("memory_limit", &self.memory_limit)
            .field("has_early_stop", &self.early_stop.is_some())
            .field("result_buffer_size", &self.result_buffer_size)
            .field("enforce_memory_limit", &self.enforce_memory_limit)
            .field("memory_fallback", &self.memory_fallback)
            .finish()
    }
}
