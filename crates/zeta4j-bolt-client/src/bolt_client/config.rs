// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bolt client reconnect settings.

use std::time::Duration;

/// 재연결 설정
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// 최대 재시도 횟수
    pub max_retries: u32,
    /// 재시도 간격
    pub retry_delay: Duration,
    /// 지수 백오프 팩터
    pub backoff_factor: f32,
    /// 최대 대기 시간
    pub max_delay: Duration,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            backoff_factor: 2.0,
            max_delay: Duration::from_secs(30),
        }
    }
}

impl ReconnectConfig {
    /// 새 재연결 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 최대 재시도 횟수 설정
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// 재시도 간격 설정
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    /// 지수 백오프 팩터 설정
    pub fn with_backoff_factor(mut self, factor: f32) -> Self {
        self.backoff_factor = factor;
        self
    }

    /// 최대 대기 시간 설정
    pub fn with_max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    /// n번째 재시도에 대한 대기 시간 계산
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_ms =
            self.retry_delay.as_millis() as f32 * self.backoff_factor.powi(attempt as i32);
        let delay = Duration::from_millis(delay_ms as u64);
        std::cmp::min(delay, self.max_delay)
    }
}
