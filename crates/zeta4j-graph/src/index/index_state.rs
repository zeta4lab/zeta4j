// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 인덱스 상태 관리 모듈
//!
//! 인덱스의 상태(Clean, Stale, Rebuilding 등)를 추적하고
//! 메타데이터(버전, 마지막 업데이트 시간 등)를 관리합니다.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime};

/// 인덱스 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IndexState {
    /// 정상 상태 - 데이터와 동기화됨
    Clean,
    /// Stale 상태 - 데이터와 동기화되지 않았을 수 있음
    Stale,
    /// 리빌드 중
    Rebuilding,
    /// 오류 상태
    Error,
    /// 생성 직후 (아직 데이터 없음)
    #[default]
    Empty,
}

impl std::fmt::Display for IndexState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexState::Clean => write!(f, "CLEAN"),
            IndexState::Stale => write!(f, "STALE"),
            IndexState::Rebuilding => write!(f, "REBUILDING"),
            IndexState::Error => write!(f, "ERROR"),
            IndexState::Empty => write!(f, "EMPTY"),
        }
    }
}

/// 인덱스 메타데이터
#[derive(Debug, Clone)]
pub struct IndexMetadata {
    /// 인덱스 버전 (변경될 때마다 증가)
    pub version: u64,
    /// 현재 상태
    pub state: IndexState,
    /// 마지막 업데이트 시간 (Instant - 경과 시간 측정용)
    pub last_updated: Instant,
    /// 마지막 업데이트 시간 (SystemTime - 절대 시간)
    pub last_updated_at: SystemTime,
    /// 인덱스 생성 시간
    pub created_at: SystemTime,
    /// 인덱싱된 항목 수
    pub entry_count: usize,
    /// 마지막 검증 시간 (None이면 검증 안 됨)
    pub last_verified: Option<Instant>,
    /// stale 감지 임계값 (업데이트 없이 이 시간이 지나면 stale 의심)
    pub stale_threshold: Duration,
    /// 마지막 stale 검사에서 발견된 불일치 수
    pub stale_entry_count: usize,
}

impl Default for IndexMetadata {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexMetadata {
    /// 새 메타데이터 생성
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            version: 0,
            state: IndexState::Empty,
            last_updated: now,
            last_updated_at: SystemTime::now(),
            created_at: SystemTime::now(),
            entry_count: 0,
            last_verified: None,
            stale_threshold: Duration::from_secs(3600), // 기본 1시간
            stale_entry_count: 0,
        }
    }

    /// 사용자 정의 stale 임계값으로 생성
    pub fn with_stale_threshold(stale_threshold: Duration) -> Self {
        let mut meta = Self::new();
        meta.stale_threshold = stale_threshold;
        meta
    }

    /// 상태를 Clean으로 설정
    pub fn mark_clean(&mut self) {
        self.state = IndexState::Clean;
        self.stale_entry_count = 0;
        self.last_verified = Some(Instant::now());
    }

    /// 상태를 Stale로 설정
    pub fn mark_stale(&mut self, stale_count: usize) {
        self.state = IndexState::Stale;
        self.stale_entry_count = stale_count;
        self.last_verified = Some(Instant::now());
    }

    /// 상태를 Rebuilding으로 설정
    pub fn mark_rebuilding(&mut self) {
        self.state = IndexState::Rebuilding;
    }

    /// 상태를 Error로 설정
    pub fn mark_error(&mut self) {
        self.state = IndexState::Error;
    }

    /// 업데이트 기록 (항목 추가/수정/삭제 시)
    pub fn record_update(&mut self, entry_count: usize) {
        self.version += 1;
        self.last_updated = Instant::now();
        self.last_updated_at = SystemTime::now();
        self.entry_count = entry_count;

        // Empty 상태에서 업데이트가 있으면 Clean으로 전환
        if self.state == IndexState::Empty && entry_count > 0 {
            self.state = IndexState::Clean;
        }
    }

    /// 검증 없이 시간 경과만으로 stale 여부 확인
    pub fn is_potentially_stale(&self) -> bool {
        self.last_updated.elapsed() > self.stale_threshold
    }

    /// 마지막 검증 이후 경과 시간
    pub fn time_since_verification(&self) -> Option<Duration> {
        self.last_verified.map(|v| v.elapsed())
    }

    /// 인덱스가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }

    /// 인덱스 상태가 정상인지 확인
    pub fn is_healthy(&self) -> bool {
        matches!(self.state, IndexState::Clean | IndexState::Empty)
    }
}

/// 인덱스 검증 결과
#[derive(Debug, Clone)]
pub struct IndexVerificationResult {
    /// 검증 통과 여부
    pub is_valid: bool,
    /// 총 인덱스 항목 수
    pub total_entries: usize,
    /// 유효한 항목 수
    pub valid_entries: usize,
    /// stale 항목 수 (데이터와 불일치)
    pub stale_entries: usize,
    /// 누락된 항목 수 (인덱스에 없는 데이터)
    pub missing_entries: usize,
    /// 검증에 걸린 시간
    pub verification_duration: Duration,
    /// 상세 오류 메시지 (있는 경우)
    pub errors: Vec<String>,
}

impl IndexVerificationResult {
    /// 성공적인 검증 결과 생성
    pub fn valid(total_entries: usize, duration: Duration) -> Self {
        Self {
            is_valid: true,
            total_entries,
            valid_entries: total_entries,
            stale_entries: 0,
            missing_entries: 0,
            verification_duration: duration,
            errors: Vec::new(),
        }
    }

    /// 불일치가 있는 검증 결과 생성
    pub fn with_mismatches(
        total_entries: usize,
        stale_entries: usize,
        missing_entries: usize,
        duration: Duration,
    ) -> Self {
        Self {
            is_valid: stale_entries == 0 && missing_entries == 0,
            total_entries,
            valid_entries: total_entries.saturating_sub(stale_entries),
            stale_entries,
            missing_entries,
            verification_duration: duration,
            errors: Vec::new(),
        }
    }

    /// 오류와 함께 검증 결과 생성
    pub fn with_error(error: String, duration: Duration) -> Self {
        Self {
            is_valid: false,
            total_entries: 0,
            valid_entries: 0,
            stale_entries: 0,
            missing_entries: 0,
            verification_duration: duration,
            errors: vec![error],
        }
    }

    /// stale 비율 계산 (0.0 ~ 1.0)
    pub fn stale_ratio(&self) -> f64 {
        if self.total_entries == 0 {
            0.0
        } else {
            self.stale_entries as f64 / self.total_entries as f64
        }
    }
}

/// 인덱스 상태 요약 (여러 인덱스 통계)
#[derive(Debug, Clone, Default)]
pub struct IndexStatesSummary {
    /// 총 인덱스 수
    pub total_indexes: usize,
    /// Clean 상태 인덱스 수
    pub clean_count: usize,
    /// Stale 상태 인덱스 수
    pub stale_count: usize,
    /// Rebuilding 상태 인덱스 수
    pub rebuilding_count: usize,
    /// Error 상태 인덱스 수
    pub error_count: usize,
    /// Empty 상태 인덱스 수
    pub empty_count: usize,
    /// stale 인덱스 이름 목록
    pub stale_indexes: Vec<String>,
}

impl IndexStatesSummary {
    /// 새 요약 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 인덱스 상태 추가
    pub fn add(&mut self, name: &str, state: IndexState) {
        self.total_indexes += 1;
        match state {
            IndexState::Clean => self.clean_count += 1,
            IndexState::Stale => {
                self.stale_count += 1;
                self.stale_indexes.push(name.to_string());
            }
            IndexState::Rebuilding => self.rebuilding_count += 1,
            IndexState::Error => self.error_count += 1,
            IndexState::Empty => self.empty_count += 1,
        }
    }

    /// 모든 인덱스가 정상인지 확인
    pub fn all_healthy(&self) -> bool {
        self.stale_count == 0 && self.error_count == 0
    }
}
