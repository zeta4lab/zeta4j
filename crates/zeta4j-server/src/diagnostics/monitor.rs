// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 시스템 상태 모니터 모듈

/// 버퍼 풀 상태
#[derive(Debug, Clone, Default)]
pub struct BufferPoolStatus {
    /// 총 크기 (바이트)
    pub size: usize,
    /// 사용 중인 크기 (바이트)
    pub used: usize,
    /// 히트율 (0.0 ~ 1.0)
    pub hit_ratio: f64,
    /// 제거(eviction) 횟수
    pub eviction_count: usize,
}

impl BufferPoolStatus {
    /// 사용률 계산
    pub fn usage_ratio(&self) -> f64 {
        if self.size == 0 {
            0.0
        } else {
            self.used as f64 / self.size as f64
        }
    }
}

/// 트랜잭션 상태
#[derive(Debug, Clone, Default)]
pub struct TransactionStatus {
    /// 활성 트랜잭션 수
    pub active_count: usize,
    /// 커밋된 트랜잭션 수
    pub committed_count: u64,
    /// 중단된 트랜잭션 수
    pub aborted_count: u64,
    /// 평균 지속 시간 (마이크로초)
    pub avg_duration_us: u64,
}

impl TransactionStatus {
    /// 중단 비율 계산
    pub fn abort_ratio(&self) -> f64 {
        let total = self.committed_count + self.aborted_count;
        if total == 0 {
            0.0
        } else {
            self.aborted_count as f64 / total as f64
        }
    }
}

/// 락 상태
#[derive(Debug, Clone, Default)]
pub struct LockStatus {
    /// 보유 중인 락 수
    pub held_count: usize,
    /// 대기 중인 락 수
    pub waiting_count: usize,
    /// 데드락 발생 횟수
    pub deadlock_count: u64,
}

impl LockStatus {
    /// 경합 수준 계산
    pub fn contention_level(&self) -> f64 {
        if self.held_count == 0 {
            0.0
        } else {
            self.waiting_count as f64 / self.held_count as f64
        }
    }
}

/// 인덱스 상태
#[derive(Debug, Clone, Default)]
pub struct IndexStatus {
    /// 총 인덱스 수
    pub total_count: usize,
    /// B-Tree 인덱스 수
    pub btree_count: usize,
    /// 전문 검색 인덱스 수
    pub fulltext_count: usize,
    /// 공간 인덱스 수
    pub spatial_count: usize,
}

/// 시스템 상태 스냅샷
#[derive(Debug, Clone)]
pub struct SystemStatus {
    /// 타임스탬프
    pub timestamp: u64,
    /// 노드 수
    pub node_count: usize,
    /// 엣지 수
    pub edge_count: usize,
    /// 메모리 사용량 (바이트)
    pub memory_used: usize,
    /// 최대 메모리 (바이트)
    pub memory_max: usize,
    /// 버퍼 풀 상태
    pub buffer_pool: BufferPoolStatus,
    /// 트랜잭션 상태
    pub transactions: TransactionStatus,
    /// 락 상태
    pub locks: LockStatus,
    /// 인덱스 상태
    pub indexes: IndexStatus,
}

impl Default for SystemStatus {
    fn default() -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            node_count: 0,
            edge_count: 0,
            memory_used: 0,
            memory_max: 0,
            buffer_pool: BufferPoolStatus::default(),
            transactions: TransactionStatus::default(),
            locks: LockStatus::default(),
            indexes: IndexStatus::default(),
        }
    }
}

impl SystemStatus {
    /// 새 SystemStatus 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 메모리 사용률
    pub fn memory_usage_ratio(&self) -> f64 {
        if self.memory_max == 0 {
            0.0
        } else {
            self.memory_used as f64 / self.memory_max as f64
        }
    }

    /// 빌더: 노드/엣지 수 설정
    pub fn with_graph_stats(mut self, nodes: usize, edges: usize) -> Self {
        self.node_count = nodes;
        self.edge_count = edges;
        self
    }

    /// 빌더: 메모리 설정
    pub fn with_memory(mut self, used: usize, max: usize) -> Self {
        self.memory_used = used;
        self.memory_max = max;
        self
    }

    /// 빌더: 버퍼 풀 설정
    pub fn with_buffer_pool(mut self, status: BufferPoolStatus) -> Self {
        self.buffer_pool = status;
        self
    }

    /// 빌더: 트랜잭션 설정
    pub fn with_transactions(mut self, status: TransactionStatus) -> Self {
        self.transactions = status;
        self
    }

    /// 빌더: 락 설정
    pub fn with_locks(mut self, status: LockStatus) -> Self {
        self.locks = status;
        self
    }

    /// 빌더: 인덱스 설정
    pub fn with_indexes(mut self, status: IndexStatus) -> Self {
        self.indexes = status;
        self
    }
}

/// 트렌드 방향
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrendDirection {
    /// 개선 중
    Improving,
    /// 안정
    Stable,
    /// 악화 중
    Degrading,
}

/// 시스템 상태 모니터
pub struct SystemMonitor {
    /// 상태 히스토리
    history: Vec<SystemStatus>,
    /// 최대 히스토리 크기
    max_history: usize,
}

impl SystemMonitor {
    /// 새 SystemMonitor 생성
    pub fn new(max_history: usize) -> Self {
        Self {
            history: Vec::new(),
            max_history,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(100)
    }

    /// 상태 스냅샷 기록
    pub fn record(&mut self, status: SystemStatus) {
        self.history.push(status);

        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// 현재 상태 (가장 최근)
    pub fn current(&self) -> Option<&SystemStatus> {
        self.history.last()
    }

    /// 상태 히스토리
    pub fn history(&self) -> &[SystemStatus] {
        &self.history
    }

    /// 히스토리 크기
    pub fn count(&self) -> usize {
        self.history.len()
    }

    /// 메모리 트렌드 분석
    pub fn memory_trend(&self) -> TrendDirection {
        self.analyze_trend(|s| s.memory_usage_ratio())
    }

    /// 버퍼 풀 히트율 트렌드
    pub fn hit_ratio_trend(&self) -> TrendDirection {
        // 히트율은 높을수록 좋으므로 반전
        match self.analyze_trend(|s| s.buffer_pool.hit_ratio) {
            TrendDirection::Improving => TrendDirection::Degrading,
            TrendDirection::Degrading => TrendDirection::Improving,
            TrendDirection::Stable => TrendDirection::Stable,
        }
    }

    /// 락 경합 트렌드
    pub fn lock_contention_trend(&self) -> TrendDirection {
        self.analyze_trend(|s| s.locks.contention_level())
    }

    /// 트렌드 분석 (값이 증가하면 Degrading)
    fn analyze_trend<F>(&self, extractor: F) -> TrendDirection
    where
        F: Fn(&SystemStatus) -> f64,
    {
        if self.history.len() < 3 {
            return TrendDirection::Stable;
        }

        let recent: Vec<f64> = self.history.iter().rev().take(5).map(&extractor).collect();

        if recent.len() < 2 {
            return TrendDirection::Stable;
        }

        // 최근 값들의 변화 계산
        let mut increasing = 0;
        let mut decreasing = 0;

        for i in 0..recent.len() - 1 {
            let diff = recent[i] - recent[i + 1];
            if diff > 0.05 {
                increasing += 1;
            } else if diff < -0.05 {
                decreasing += 1;
            }
        }

        if increasing > decreasing + 1 {
            TrendDirection::Degrading
        } else if decreasing > increasing + 1 {
            TrendDirection::Improving
        } else {
            TrendDirection::Stable
        }
    }

    /// 평균 메모리 사용량
    pub fn average_memory_usage(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.history.iter().map(|s| s.memory_usage_ratio()).sum();
        sum / self.history.len() as f64
    }

    /// 평균 히트율
    pub fn average_hit_ratio(&self) -> f64 {
        if self.history.is_empty() {
            return 1.0;
        }
        let sum: f64 = self.history.iter().map(|s| s.buffer_pool.hit_ratio).sum();
        sum / self.history.len() as f64
    }

    /// 히스토리 클리어
    pub fn clear(&mut self) {
        self.history.clear();
    }
}
