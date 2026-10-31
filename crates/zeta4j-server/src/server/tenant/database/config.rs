// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 데이터베이스 설정 타입.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 데이터베이스 접근 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccessMode {
    /// 읽기 전용
    Read,
    /// 읽기/쓰기
    #[default]
    Write,
}

/// 테넌트 데이터베이스 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDatabaseConfig {
    /// 읽기 전용 모드
    pub read_only: bool,
    /// 트랜잭션 타임아웃 (밀리초)
    pub tx_timeout_ms: u64,
    /// 쿼리 타임아웃 (밀리초)
    pub query_timeout_ms: u64,
    /// 기본 접근 모드
    pub default_access_mode: AccessMode,
    /// 인덱스 제공자
    pub index_provider: String,
    /// 전문 검색 제공자
    pub fulltext_provider: Option<String>,
    /// 최대 동시 트랜잭션
    pub max_concurrent_transactions: u32,
}

impl TenantDatabaseConfig {
    /// 트랜잭션 타임아웃
    pub fn tx_timeout(&self) -> Duration {
        Duration::from_millis(self.tx_timeout_ms)
    }

    /// 쿼리 타임아웃
    pub fn query_timeout(&self) -> Duration {
        Duration::from_millis(self.query_timeout_ms)
    }
}

impl Default for TenantDatabaseConfig {
    fn default() -> Self {
        Self {
            read_only: false,
            tx_timeout_ms: 30_000,     // 30 seconds
            query_timeout_ms: 120_000, // 120 seconds
            default_access_mode: AccessMode::Write,
            index_provider: "native-btree-1.0".to_string(),
            fulltext_provider: Some("lucene".to_string()),
            max_concurrent_transactions: 1000,
        }
    }
}

/// 설정 빌더
#[derive(Debug, Default)]
pub struct TenantDatabaseConfigBuilder {
    config: TenantDatabaseConfig,
}

impl TenantDatabaseConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.config.read_only = read_only;
        self
    }

    pub fn tx_timeout(mut self, timeout: Duration) -> Self {
        self.config.tx_timeout_ms = timeout.as_millis() as u64;
        self
    }

    pub fn tx_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.config.tx_timeout_ms = timeout_ms;
        self
    }

    pub fn query_timeout(mut self, timeout: Duration) -> Self {
        self.config.query_timeout_ms = timeout.as_millis() as u64;
        self
    }

    pub fn query_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.config.query_timeout_ms = timeout_ms;
        self
    }

    pub fn default_access_mode(mut self, mode: AccessMode) -> Self {
        self.config.default_access_mode = mode;
        self
    }

    pub fn index_provider(mut self, provider: impl Into<String>) -> Self {
        self.config.index_provider = provider.into();
        self
    }

    pub fn fulltext_provider(mut self, provider: impl Into<String>) -> Self {
        self.config.fulltext_provider = Some(provider.into());
        self
    }

    pub fn max_concurrent_transactions(mut self, max: u32) -> Self {
        self.config.max_concurrent_transactions = max;
        self
    }

    pub fn build(self) -> TenantDatabaseConfig {
        self.config
    }
}
