// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 캐시 엔트리 모듈

use std::time::Instant;

/// 캐시 엔트리
#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    /// 캐시된 값
    value: T,
    /// 생성 시간
    created_at: Instant,
    /// 마지막 접근 시간
    last_accessed: Instant,
    /// 접근 횟수
    access_count: u64,
}

impl<T> CacheEntry<T> {
    /// 새 캐시 엔트리 생성
    pub fn new(value: T) -> Self {
        let now = Instant::now();
        Self {
            value,
            created_at: now,
            last_accessed: now,
            access_count: 1,
        }
    }

    /// 값 참조 반환
    pub fn value(&self) -> &T {
        &self.value
    }

    /// 값 가변 참조 반환
    pub fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// 값 소비하여 반환
    pub fn into_value(self) -> T {
        self.value
    }

    /// 생성 시간 반환
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// 마지막 접근 시간 반환
    pub fn last_accessed(&self) -> Instant {
        self.last_accessed
    }

    /// 접근 횟수 반환
    pub fn access_count(&self) -> u64 {
        self.access_count
    }

    /// 접근 기록 (LRU 업데이트)
    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
        self.access_count += 1;
    }

    /// 생성 이후 경과 시간 (밀리초)
    pub fn age_ms(&self) -> u128 {
        self.created_at.elapsed().as_millis()
    }

    /// 마지막 접근 이후 경과 시간 (밀리초)
    pub fn idle_ms(&self) -> u128 {
        self.last_accessed.elapsed().as_millis()
    }
}

impl<T: Clone> CacheEntry<T> {
    /// 값 복제하여 반환
    pub fn clone_value(&self) -> T {
        self.value.clone()
    }
}
