// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Backpressure Control (M2.4)
//!
//! 스트리밍 백프레셔 및 흐름 제어

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// 백프레셔 에러
#[derive(Debug, Error)]
pub enum BackpressureError {
    #[error("Too many concurrent streams")]
    TooManyConcurrentStreams,

    #[error("Buffer full")]
    BufferFull,

    #[error("Acquire timeout")]
    AcquireTimeout,
}

/// 스트림 퍼밋 (RAII 가드)
pub struct StreamPermit {
    _permit: OwnedSemaphorePermit,
    active_count: Arc<AtomicUsize>,
}

impl StreamPermit {
    fn new(permit: OwnedSemaphorePermit, active_count: Arc<AtomicUsize>) -> Self {
        active_count.fetch_add(1, Ordering::SeqCst);
        Self {
            _permit: permit,
            active_count,
        }
    }
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        self.active_count.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 백프레셔 제어
pub struct StreamBackpressure {
    /// 동시 스트림 제한 세마포어
    semaphore: Arc<Semaphore>,
    /// 최대 동시 스트림 수
    max_concurrent: usize,
    /// 현재 활성 스트림 수
    active_count: Arc<AtomicUsize>,
    /// 버퍼 high water mark
    buffer_high_water: usize,
    /// 버퍼 low water mark
    buffer_low_water: usize,
}

impl StreamBackpressure {
    /// 새 백프레셔 제어 생성
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_concurrent,
            active_count: Arc::new(AtomicUsize::new(0)),
            buffer_high_water: 10000,
            buffer_low_water: 1000,
        }
    }

    /// 버퍼 워터마크 설정
    pub fn with_watermarks(mut self, high: usize, low: usize) -> Self {
        self.buffer_high_water = high;
        self.buffer_low_water = low;
        self
    }

    /// 스트리밍 허가 획득 (블로킹)
    pub async fn acquire(&self) -> Result<StreamPermit, BackpressureError> {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| BackpressureError::TooManyConcurrentStreams)?;

        Ok(StreamPermit::new(permit, self.active_count.clone()))
    }

    /// 스트리밍 허가 시도 (비블로킹)
    pub fn try_acquire(&self) -> Result<StreamPermit, BackpressureError> {
        let permit = self
            .semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| BackpressureError::TooManyConcurrentStreams)?;

        Ok(StreamPermit::new(permit, self.active_count.clone()))
    }

    /// 타임아웃과 함께 허가 획득
    pub async fn acquire_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<StreamPermit, BackpressureError> {
        let permit = tokio::time::timeout(timeout, self.semaphore.clone().acquire_owned())
            .await
            .map_err(|_| BackpressureError::AcquireTimeout)?
            .map_err(|_| BackpressureError::TooManyConcurrentStreams)?;

        Ok(StreamPermit::new(permit, self.active_count.clone()))
    }

    /// 버퍼 일시 중지 필요 여부
    pub fn should_pause(&self, buffer_size: usize) -> bool {
        buffer_size >= self.buffer_high_water
    }

    /// 버퍼 재개 가능 여부
    pub fn should_resume(&self, buffer_size: usize) -> bool {
        buffer_size <= self.buffer_low_water
    }

    /// 현재 활성 스트림 수
    pub fn active_streams(&self) -> usize {
        self.active_count.load(Ordering::SeqCst)
    }

    /// 사용 가능한 슬롯 수
    pub fn available_slots(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// 최대 동시 스트림 수
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// 포화 여부 (모든 슬롯 사용 중)
    pub fn is_saturated(&self) -> bool {
        self.available_slots() == 0
    }
}

impl Default for StreamBackpressure {
    fn default() -> Self {
        Self::new(100) // 기본 100개 동시 스트림
    }
}

/// 공유 백프레셔
pub type SharedBackpressure = Arc<StreamBackpressure>;

/// 백프레셔 생성 헬퍼
pub fn create_backpressure(max_concurrent: usize) -> SharedBackpressure {
    Arc::new(StreamBackpressure::new(max_concurrent))
}

/// 버퍼 상태 추적
pub struct BufferState {
    current_size: AtomicUsize,
    high_water: usize,
    low_water: usize,
    paused: std::sync::atomic::AtomicBool,
}

impl BufferState {
    /// 새 버퍼 상태 생성
    pub fn new(high_water: usize, low_water: usize) -> Self {
        Self {
            current_size: AtomicUsize::new(0),
            high_water,
            low_water,
            paused: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// 버퍼 크기 증가
    pub fn increase(&self, amount: usize) -> bool {
        let new_size = self.current_size.fetch_add(amount, Ordering::SeqCst) + amount;
        if new_size >= self.high_water {
            self.paused.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    /// 버퍼 크기 감소
    pub fn decrease(&self, amount: usize) -> bool {
        let old_size = self.current_size.fetch_sub(amount, Ordering::SeqCst);
        let new_size = old_size.saturating_sub(amount);
        if new_size <= self.low_water && self.paused.load(Ordering::SeqCst) {
            self.paused.store(false, Ordering::SeqCst);
            true // 재개 가능
        } else {
            false
        }
    }

    /// 현재 버퍼 크기
    pub fn current_size(&self) -> usize {
        self.current_size.load(Ordering::SeqCst)
    }

    /// 일시 중지 상태
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// 수동 일시 중지
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    /// 수동 재개
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// 리셋
    pub fn reset(&self) {
        self.current_size.store(0, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
    }
}
