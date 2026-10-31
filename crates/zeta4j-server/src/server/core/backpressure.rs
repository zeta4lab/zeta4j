// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Backpressure Controller

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::SystemTime;
use tokio::sync::Semaphore;

use crate::server::common::config::ServerConfig;

/// 백프레셔 사용량 정보
#[derive(Debug, Clone)]
pub struct BackpressureUsage {
    pub current: usize,
    pub max: usize,
    pub percentage: f64,
    pub is_throttling: bool,
}

/// 백프레셔 컨트롤러
///
/// 서버 과부하 방지를 위한 백프레셔 메커니즘:
/// - 세마포어 기반 동시 연결 제한
/// - 수위(watermark) 기반 조절
/// - 메트릭 기반 동적 조절
pub struct BackpressureController {
    semaphore: Semaphore,
    max_permits: usize,
    high_watermark: usize,
    low_watermark: usize,
    is_throttling: AtomicBool,
    acquired_count: AtomicUsize,
    enabled: bool,
}

impl BackpressureController {
    /// 새 백프레셔 컨트롤러 생성
    pub fn new(config: &ServerConfig) -> Self {
        let max_permits = config.max_connections;
        let high_watermark = (max_permits as f64 * config.backpressure_high_watermark) as usize;
        let low_watermark = (max_permits as f64 * config.backpressure_low_watermark) as usize;

        Self {
            semaphore: Semaphore::new(max_permits),
            max_permits,
            high_watermark,
            low_watermark,
            is_throttling: AtomicBool::new(false),
            acquired_count: AtomicUsize::new(0),
            enabled: config.backpressure_enabled,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults(max_connections: usize) -> Self {
        Self {
            semaphore: Semaphore::new(max_connections),
            max_permits: max_connections,
            high_watermark: (max_connections as f64 * 0.8) as usize,
            low_watermark: (max_connections as f64 * 0.5) as usize,
            is_throttling: AtomicBool::new(false),
            acquired_count: AtomicUsize::new(0),
            enabled: true,
        }
    }

    /// 허가 획득 시도 (논블로킹)
    pub fn try_acquire(&self) -> bool {
        if !self.enabled {
            return true;
        }

        // 스로틀링 중이면 거부
        if self.is_throttling.load(Ordering::Acquire) {
            return false;
        }

        match self.semaphore.try_acquire() {
            Ok(permit) => {
                // permit을 forget하여 수동 관리
                std::mem::forget(permit);

                let count = self.acquired_count.fetch_add(1, Ordering::AcqRel) + 1;

                // 고수위 도달 시 스로틀링 시작
                if count >= self.high_watermark {
                    self.is_throttling.store(true, Ordering::Release);
                    tracing::warn!(
                        "Backpressure: high watermark reached ({}/{})",
                        count,
                        self.max_permits
                    );
                }

                true
            }
            Err(_) => false,
        }
    }

    /// 허가 해제
    pub fn release(&self) {
        if !self.enabled {
            return;
        }

        self.semaphore.add_permits(1);
        let count = self
            .acquired_count
            .fetch_sub(1, Ordering::AcqRel)
            .saturating_sub(1);

        // 저수위 도달 시 스로틀링 해제
        if self.is_throttling.load(Ordering::Acquire) && count <= self.low_watermark {
            self.is_throttling.store(false, Ordering::Release);
            tracing::info!(
                "Backpressure: low watermark reached ({}/{}), resuming",
                count,
                self.max_permits
            );
        }
    }

    /// 스로틀링 상태
    pub fn is_throttling(&self) -> bool {
        self.is_throttling.load(Ordering::Acquire)
    }

    /// 현재 사용량
    pub fn usage(&self) -> BackpressureUsage {
        let acquired = self.acquired_count.load(Ordering::Acquire);
        BackpressureUsage {
            current: acquired,
            max: self.max_permits,
            percentage: (acquired as f64 / self.max_permits as f64) * 100.0,
            is_throttling: self.is_throttling.load(Ordering::Acquire),
        }
    }

    /// 현재 획득된 허가 수
    pub fn acquired(&self) -> usize {
        self.acquired_count.load(Ordering::Acquire)
    }

    /// 가용 허가 수
    pub fn available(&self) -> usize {
        self.max_permits.saturating_sub(self.acquired())
    }

    /// 활성화 여부
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Rate Limiter (토큰 버킷 알고리즘)
pub struct RateLimiter {
    tokens: AtomicU64,
    max_tokens: u64,
    refill_rate: u64,       // tokens per second
    last_refill: AtomicU64, // timestamp in nanos
}

impl RateLimiter {
    /// 새 Rate Limiter 생성
    pub fn new(requests_per_second: u32) -> Self {
        let max_tokens = requests_per_second as u64;
        Self {
            tokens: AtomicU64::new(max_tokens),
            max_tokens,
            refill_rate: max_tokens,
            last_refill: AtomicU64::new(Self::now_nanos()),
        }
    }

    fn now_nanos() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    }

    /// 토큰 리필
    fn refill(&self) {
        let now = Self::now_nanos();
        let last = self.last_refill.load(Ordering::Acquire);
        let elapsed_secs = (now.saturating_sub(last)) as f64 / 1_000_000_000.0;

        if elapsed_secs > 0.001 {
            // 1ms 이상 경과
            let new_tokens = (elapsed_secs * self.refill_rate as f64) as u64;
            if new_tokens > 0
                && self
                    .last_refill
                    .compare_exchange(last, now, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                loop {
                    let current = self.tokens.load(Ordering::Acquire);
                    let new_total = current.saturating_add(new_tokens).min(self.max_tokens);
                    if self
                        .tokens
                        .compare_exchange(current, new_total, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        break;
                    }
                }
            }
        }
    }

    /// 토큰 획득 시도
    pub fn try_acquire(&self) -> bool {
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current == 0 {
                return false;
            }

            if self
                .tokens
                .compare_exchange(current, current - 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }

    /// N개 토큰 획득 시도
    pub fn try_acquire_n(&self, n: u64) -> bool {
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current < n {
                return false;
            }

            if self
                .tokens
                .compare_exchange(current, current - n, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }

    /// 현재 가용 토큰
    pub fn available_tokens(&self) -> u64 {
        self.refill();
        self.tokens.load(Ordering::Acquire)
    }

    /// 최대 토큰 수
    pub fn max_tokens(&self) -> u64 {
        self.max_tokens
    }

    /// 초당 리필 속도
    pub fn refill_rate(&self) -> u64 {
        self.refill_rate
    }
}
