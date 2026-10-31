// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 메모리 자동 조정 모듈

/// 메모리 자동 조정 설정
#[derive(Debug, Clone)]
pub struct MemoryTuningConfig {
    /// 버퍼 풀 최소 크기 (바이트)
    pub buffer_pool_min: usize,
    /// 버퍼 풀 최대 크기 (바이트)
    pub buffer_pool_max: usize,
    /// 목표 히트율 (0.0 ~ 1.0)
    pub target_hit_ratio: f64,
    /// 조정 간격 (초)
    pub adjustment_interval_secs: u64,
    /// 한 번에 조정할 최대 비율 (0.0 ~ 1.0)
    pub max_adjustment_ratio: f64,
}

impl Default for MemoryTuningConfig {
    fn default() -> Self {
        Self {
            buffer_pool_min: 64 * 1024 * 1024,   // 64MB
            buffer_pool_max: 1024 * 1024 * 1024, // 1GB
            target_hit_ratio: 0.9,               // 90%
            adjustment_interval_secs: 60,        // 1분
            max_adjustment_ratio: 0.25,          // 25%
        }
    }
}

/// 메모리 통계
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// 현재 버퍼 풀 크기 (바이트)
    pub buffer_pool_size: usize,
    /// 캐시 히트율 (0.0 ~ 1.0)
    pub hit_ratio: f64,
    /// 제거(eviction) 횟수
    pub eviction_count: usize,
    /// 메모리 압력 (0.0 ~ 1.0, 높을수록 압박)
    pub memory_pressure: f64,
    /// 총 요청 수
    pub total_requests: usize,
    /// 캐시 히트 수
    pub cache_hits: usize,
    /// 캐시 미스 수
    pub cache_misses: usize,
}

impl MemoryStats {
    /// 새 MemoryStats 생성
    pub fn new(buffer_pool_size: usize) -> Self {
        Self {
            buffer_pool_size,
            ..Default::default()
        }
    }

    /// 히트율 계산
    pub fn calculate_hit_ratio(&self) -> f64 {
        if self.total_requests == 0 {
            1.0
        } else {
            self.cache_hits as f64 / self.total_requests as f64
        }
    }

    /// 캐시 히트 기록
    pub fn record_hit(&mut self) {
        self.cache_hits += 1;
        self.total_requests += 1;
        self.hit_ratio = self.calculate_hit_ratio();
    }

    /// 캐시 미스 기록
    pub fn record_miss(&mut self) {
        self.cache_misses += 1;
        self.total_requests += 1;
        self.hit_ratio = self.calculate_hit_ratio();
    }

    /// 제거 기록
    pub fn record_eviction(&mut self) {
        self.eviction_count += 1;
    }
}

/// 튜닝 액션 타입
#[derive(Debug, Clone, PartialEq)]
pub enum TuningActionType {
    /// 버퍼 풀 크기 증가 (바이트)
    IncreaseBufferPool(usize),
    /// 버퍼 풀 크기 감소 (바이트)
    DecreaseBufferPool(usize),
    /// 제거 정책 조정
    AdjustEvictionPolicy,
    /// 액션 없음
    NoAction,
}

/// 튜닝 액션
#[derive(Debug, Clone)]
pub struct TuningAction {
    /// 액션 타입
    pub action_type: TuningActionType,
    /// 액션 이유
    pub reason: String,
    /// 예상 개선 효과
    pub expected_improvement: String,
}

impl TuningAction {
    /// NoAction 생성
    pub fn no_action() -> Self {
        Self {
            action_type: TuningActionType::NoAction,
            reason: "Current settings are optimal".to_string(),
            expected_improvement: "N/A".to_string(),
        }
    }

    /// 버퍼 풀 증가 액션 생성
    pub fn increase_buffer_pool(amount: usize, reason: String) -> Self {
        Self {
            action_type: TuningActionType::IncreaseBufferPool(amount),
            reason,
            expected_improvement: format!(
                "Buffer pool increase by {}MB expected to improve hit rate",
                amount / (1024 * 1024)
            ),
        }
    }

    /// 버퍼 풀 감소 액션 생성
    pub fn decrease_buffer_pool(amount: usize, reason: String) -> Self {
        Self {
            action_type: TuningActionType::DecreaseBufferPool(amount),
            reason,
            expected_improvement: format!(
                "Buffer pool decrease by {}MB to save memory",
                amount / (1024 * 1024)
            ),
        }
    }
}

/// 메모리 자동 조정기
pub struct MemoryAutoTuner {
    config: MemoryTuningConfig,
    /// 현재 버퍼 풀 크기
    current_buffer_pool_size: usize,
    /// 최근 히트율 기록 (이동 평균용)
    recent_hit_ratios: Vec<f64>,
    /// 최근 조정 기록
    recent_actions: Vec<TuningAction>,
    /// 마지막 조정 시간 (타임스탬프)
    last_adjustment_time: u64,
}

impl MemoryAutoTuner {
    /// 새 MemoryAutoTuner 생성
    pub fn new(config: MemoryTuningConfig) -> Self {
        let initial_size = config.buffer_pool_min;
        Self {
            config,
            current_buffer_pool_size: initial_size,
            recent_hit_ratios: Vec::new(),
            recent_actions: Vec::new(),
            last_adjustment_time: 0,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(MemoryTuningConfig::default())
    }

    /// 현재 버퍼 풀 크기 반환
    pub fn current_buffer_pool_size(&self) -> usize {
        self.current_buffer_pool_size
    }

    /// 설정 반환
    pub fn config(&self) -> &MemoryTuningConfig {
        &self.config
    }

    /// 히트율 기록
    pub fn record_hit_ratio(&mut self, hit_ratio: f64) {
        self.recent_hit_ratios.push(hit_ratio);
        // 최근 10개만 유지
        if self.recent_hit_ratios.len() > 10 {
            self.recent_hit_ratios.remove(0);
        }
    }

    /// 평균 히트율 계산
    fn average_hit_ratio(&self) -> f64 {
        if self.recent_hit_ratios.is_empty() {
            1.0
        } else {
            self.recent_hit_ratios.iter().sum::<f64>() / self.recent_hit_ratios.len() as f64
        }
    }

    /// 현재 상태 분석
    pub fn analyze(&self, stats: &MemoryStats) -> TuningAction {
        let avg_hit_ratio = if self.recent_hit_ratios.is_empty() {
            stats.hit_ratio
        } else {
            self.average_hit_ratio()
        };

        // 히트율이 목표보다 낮은 경우
        if avg_hit_ratio < self.config.target_hit_ratio {
            // 버퍼 풀 증가 가능한지 확인
            if self.current_buffer_pool_size < self.config.buffer_pool_max {
                let increase_amount = self.calculate_increase_amount(avg_hit_ratio);
                return TuningAction::increase_buffer_pool(
                    increase_amount,
                    format!(
                        "Hit rate {:.1}% below target {:.1}%",
                        avg_hit_ratio * 100.0,
                        self.config.target_hit_ratio * 100.0
                    ),
                );
            }
        }

        // 히트율이 매우 높고 메모리 압력이 있는 경우
        if avg_hit_ratio > 0.98
            && stats.memory_pressure > 0.7
            && self.current_buffer_pool_size > self.config.buffer_pool_min
        {
            let decrease_amount = self.calculate_decrease_amount();
            return TuningAction::decrease_buffer_pool(
                decrease_amount,
                format!(
                    "Hit rate {:.1}% is sufficient and memory pressure {:.1}%",
                    avg_hit_ratio * 100.0,
                    stats.memory_pressure * 100.0
                ),
            );
        }

        // 제거가 빈번한 경우
        if stats.eviction_count > 1000
            && self.current_buffer_pool_size < self.config.buffer_pool_max
        {
            let increase_amount = self.calculate_increase_amount(avg_hit_ratio);
            return TuningAction::increase_buffer_pool(
                increase_amount,
                format!(
                    "Eviction count {} suggests cache thrashing",
                    stats.eviction_count
                ),
            );
        }

        TuningAction::no_action()
    }

    /// 증가량 계산
    fn calculate_increase_amount(&self, current_hit_ratio: f64) -> usize {
        let gap = self.config.target_hit_ratio - current_hit_ratio;
        let base_increase =
            (self.current_buffer_pool_size as f64 * self.config.max_adjustment_ratio) as usize;

        // 히트율 갭에 비례하여 증가량 조정
        let adjusted_increase = (base_increase as f64 * (gap / 0.1).min(1.0)) as usize;

        // 최대 크기 제한
        let max_possible = self.config.buffer_pool_max - self.current_buffer_pool_size;
        adjusted_increase.min(max_possible).max(1024 * 1024) // 최소 1MB
    }

    /// 감소량 계산
    fn calculate_decrease_amount(&self) -> usize {
        let base_decrease = (self.current_buffer_pool_size as f64
            * self.config.max_adjustment_ratio
            * 0.5) as usize;

        // 최소 크기 제한
        let max_possible = self.current_buffer_pool_size - self.config.buffer_pool_min;
        base_decrease.min(max_possible).max(1024 * 1024) // 최소 1MB
    }

    /// 튜닝 적용
    pub fn apply(&mut self, action: TuningAction) -> Result<(), String> {
        match &action.action_type {
            TuningActionType::IncreaseBufferPool(amount) => {
                let new_size = self.current_buffer_pool_size + amount;
                if new_size > self.config.buffer_pool_max {
                    return Err("Buffer pool max size exceeded".to_string());
                }
                self.current_buffer_pool_size = new_size;
            }
            TuningActionType::DecreaseBufferPool(amount) => {
                if self.current_buffer_pool_size < *amount {
                    return Err("Buffer pool size insufficient".to_string());
                }
                let new_size = self.current_buffer_pool_size - amount;
                if new_size < self.config.buffer_pool_min {
                    return Err("Buffer pool below minimum size".to_string());
                }
                self.current_buffer_pool_size = new_size;
            }
            TuningActionType::AdjustEvictionPolicy => {
                // 현재는 구현하지 않음
            }
            TuningActionType::NoAction => {
                // 아무것도 하지 않음
            }
        }

        self.recent_actions.push(action);
        self.last_adjustment_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(())
    }

    /// 자동 조정 실행
    pub fn auto_tune(&mut self, stats: &MemoryStats) -> Result<Vec<TuningAction>, String> {
        let mut actions = Vec::new();

        // 히트율 기록
        self.record_hit_ratio(stats.hit_ratio);

        // 분석 및 액션 결정
        let action = self.analyze(stats);

        if action.action_type != TuningActionType::NoAction {
            self.apply(action.clone())?;
            actions.push(action);
        }

        Ok(actions)
    }

    /// 최근 액션 반환
    pub fn recent_actions(&self) -> &[TuningAction] {
        &self.recent_actions
    }
}
