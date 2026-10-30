//! 병렬 처리 설정

/// 병렬 처리 설정
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// 사용할 스레드 수 (0 = Rayon 자동 결정)
    pub num_threads: usize,
    /// 병렬 처리 임계값 (노드 수가 이 값 미만이면 순차 실행)
    pub min_parallel_threshold: usize,
    /// 청크 크기 (Rayon par_chunks 사용 시)
    pub chunk_size: usize,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            num_threads: 0,              // Rayon 자동 결정
            min_parallel_threshold: 100, // 100개 미만은 순차 실행
            chunk_size: 64,
        }
    }
}

impl ParallelConfig {
    /// 새 ParallelConfig 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 스레드 수 설정
    ///
    /// # Arguments
    /// * `num_threads` - 사용할 스레드 수 (0 = 자동)
    pub fn with_num_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = num_threads;
        self
    }

    /// 병렬 처리 임계값 설정
    ///
    /// 노드 수가 이 값 미만이면 순차 실행합니다.
    ///
    /// # Arguments
    /// * `threshold` - 최소 노드 수
    pub fn with_min_parallel_threshold(mut self, threshold: usize) -> Self {
        self.min_parallel_threshold = threshold;
        self
    }

    /// 청크 크기 설정
    ///
    /// # Arguments
    /// * `chunk_size` - 청크 크기
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// 병렬 처리가 필요한지 확인
    pub fn should_parallelize(&self, node_count: usize) -> bool {
        node_count >= self.min_parallel_threshold
    }

    /// 고성능 설정 (대규모 그래프용)
    pub fn high_performance() -> Self {
        Self {
            num_threads: 0,
            min_parallel_threshold: 50,
            chunk_size: 128,
        }
    }

    /// 보수적 설정 (작은 오버헤드)
    pub fn conservative() -> Self {
        Self {
            num_threads: 0,
            min_parallel_threshold: 1000,
            chunk_size: 256,
        }
    }
}
