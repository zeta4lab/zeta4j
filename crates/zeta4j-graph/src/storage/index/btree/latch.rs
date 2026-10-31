// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! B+ Tree 페이지 래치 (Crabbing Protocol)
//!
//! Lock Coupling (Crabbing)을 위한 페이지 수준 래치 관리자입니다.
//!
//! # Crabbing Protocol
//!
//! 트리 탐색 시 다음 규칙을 따릅니다:
//! 1. 루트에서 시작하여 자식으로 내려갈 때
//! 2. 자식 노드의 래치를 먼저 획득
//! 3. 부모 노드가 "safe" (분할/병합 불필요)하면 부모 래치 해제
//!
//! # Safe 조건
//! - 읽기: 언제나 safe
//! - 삽입: 노드가 꽉 차지 않음 (분할 불필요)
//! - 삭제: 노드가 반 이상 차 있음 (병합 불필요)

use crate::error::Result;
use crate::storage::disk::page::PageId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

/// 래치 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatchMode {
    /// 읽기 래치 (공유)
    Read,
    /// 쓰기 래치 (독점)
    Write,
}

/// 페이지 래치
///
/// tokio RwLock 기반으로 읽기는 공유, 쓰기는 독점
/// Arc로 감싸서 owned guard (OwnedRwLockGuard) 지원
#[derive(Debug)]
pub struct PageLatch {
    inner: Arc<RwLock<()>>,
}

impl Default for PageLatch {
    fn default() -> Self {
        Self::new()
    }
}

impl PageLatch {
    /// 새 페이지 래치 생성
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(())),
        }
    }

    /// 읽기 래치 획득 (Arc 기반, owned)
    pub async fn read_owned(&self) -> OwnedRwLockReadGuard<()> {
        self.inner.clone().read_owned().await
    }

    /// 쓰기 래치 획득 (Arc 기반, owned)
    pub async fn write_owned(&self) -> OwnedRwLockWriteGuard<()> {
        self.inner.clone().write_owned().await
    }

    /// 읽기 래치 시도 (non-blocking, Arc 기반)
    pub fn try_read_owned(&self) -> Option<OwnedRwLockReadGuard<()>> {
        self.inner.clone().try_read_owned().ok()
    }

    /// 쓰기 래치 시도 (non-blocking, Arc 기반)
    pub fn try_write_owned(&self) -> Option<OwnedRwLockWriteGuard<()>> {
        self.inner.clone().try_write_owned().ok()
    }
}

/// 래치 관리자
///
/// 페이지별 래치를 관리합니다.
/// latches HashMap 접근은 await를 넘기지 않으므로 parking_lot::Mutex 유지
pub struct LatchManager {
    latches: parking_lot::Mutex<HashMap<PageId, Arc<PageLatch>>>,
}

impl Default for LatchManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LatchManager {
    /// 새 래치 관리자 생성
    pub fn new() -> Self {
        Self {
            latches: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// 페이지 래치 가져오기 (없으면 생성)
    pub fn get_latch(&self, page_id: PageId) -> Arc<PageLatch> {
        let mut latches = self.latches.lock();
        latches
            .entry(page_id)
            .or_insert_with(|| Arc::new(PageLatch::new()))
            .clone()
    }

    /// 페이지 래치 제거
    pub fn remove_latch(&self, page_id: PageId) {
        let mut latches = self.latches.lock();
        latches.remove(&page_id);
    }

    /// 래치 개수 조회
    pub fn latch_count(&self) -> usize {
        self.latches.lock().len()
    }

    /// 모든 래치 정리
    pub fn clear(&self) {
        self.latches.lock().clear();
    }
}

/// 보유 중인 래치의 Guard
///
/// tokio의 Owned guard를 저장하여 래치가 실제로 유지되도록 함
pub enum HeldGuard {
    /// 읽기 래치 guard
    Read(OwnedRwLockReadGuard<()>),
    /// 쓰기 래치 guard
    Write(OwnedRwLockWriteGuard<()>),
}

impl std::fmt::Debug for HeldGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeldGuard::Read(_) => write!(f, "HeldGuard::Read"),
            HeldGuard::Write(_) => write!(f, "HeldGuard::Write"),
        }
    }
}

/// 보유 중인 래치 정보
#[derive(Debug)]
pub struct HeldLatch {
    /// 페이지 ID
    pub page_id: PageId,
    /// 래치 모드
    pub mode: LatchMode,
    /// 래치 참조
    latch: Arc<PageLatch>,
    /// 실제 guard (Drop 시 자동 해제)
    #[expect(dead_code)]
    guard: HeldGuard,
}

impl HeldLatch {
    /// 새 보유 래치 생성
    pub fn new(page_id: PageId, mode: LatchMode, latch: Arc<PageLatch>, guard: HeldGuard) -> Self {
        Self {
            page_id,
            mode,
            latch,
            guard,
        }
    }

    /// 래치 참조 가져오기
    pub fn latch(&self) -> &Arc<PageLatch> {
        &self.latch
    }
}

/// Crabbing 컨텍스트
///
/// 트리 탐색 중 보유한 래치 스택을 관리합니다.
pub struct CrabbingContext {
    /// 래치 관리자 참조
    latch_manager: Arc<LatchManager>,
    /// 보유 중인 래치 스택 (부모 → 자식 순)
    held_latches: Vec<HeldLatch>,
    /// 연산 모드 (읽기/쓰기)
    mode: LatchMode,
}

impl CrabbingContext {
    /// 새 Crabbing 컨텍스트 생성
    pub fn new(latch_manager: Arc<LatchManager>, mode: LatchMode) -> Self {
        Self {
            latch_manager,
            held_latches: Vec::new(),
            mode,
        }
    }

    /// 현재 모드 반환
    pub fn mode(&self) -> LatchMode {
        self.mode
    }

    /// 래치 획득 (스택에 추가)
    ///
    /// tokio의 Owned guard를 사용하여 래치가 실제로 유지됨
    pub async fn acquire(&mut self, page_id: PageId) -> Result<()> {
        let latch = self.latch_manager.get_latch(page_id);

        // 모드에 따라 래치 획득하고 guard 저장
        let guard = match self.mode {
            LatchMode::Read => HeldGuard::Read(latch.read_owned().await),
            LatchMode::Write => HeldGuard::Write(latch.write_owned().await),
        };

        self.held_latches
            .push(HeldLatch::new(page_id, self.mode, latch, guard));
        Ok(())
    }

    /// 부모 래치 해제 (safe 조건 충족 시)
    ///
    /// 현재 노드가 safe하면 부모 래치들을 모두 해제
    pub fn release_ancestors(&mut self) {
        // 마지막 래치(현재 노드)를 제외하고 모두 해제
        if self.held_latches.len() > 1 {
            self.held_latches.truncate(1);
        }
    }

    /// 특정 페이지까지의 조상 래치 해제
    pub fn release_ancestors_until(&mut self, keep_page_id: PageId) {
        let pos = self
            .held_latches
            .iter()
            .position(|h| h.page_id == keep_page_id);

        if let Some(idx) = pos {
            // keep_page_id 이후의 래치만 유지
            self.held_latches = self.held_latches.split_off(idx);
        }
    }

    /// 모든 래치 해제
    pub fn release_all(&mut self) {
        self.held_latches.clear();
    }

    /// 현재 보유 중인 래치 개수
    pub fn held_count(&self) -> usize {
        self.held_latches.len()
    }

    /// 특정 페이지 래치 보유 여부
    pub fn holds_latch(&self, page_id: PageId) -> bool {
        self.held_latches.iter().any(|h| h.page_id == page_id)
    }

    /// 보유 중인 래치 목록 (읽기 전용)
    pub fn held_latches(&self) -> &[HeldLatch] {
        &self.held_latches
    }
}

impl Drop for CrabbingContext {
    fn drop(&mut self) {
        // 모든 래치 자동 해제
        self.release_all();
    }
}

/// Safe 조건 체커
pub struct SafetyChecker;

impl SafetyChecker {
    /// 삽입 시 safe 여부 (분할 불필요)
    pub fn is_safe_for_insert(current_size: usize, max_size: usize) -> bool {
        current_size < max_size - 1
    }

    /// 삭제 시 safe 여부 (병합 불필요)
    pub fn is_safe_for_delete(current_size: usize, max_size: usize) -> bool {
        let min_size = max_size / 2;
        current_size > min_size
    }

    /// 읽기는 항상 safe
    pub fn is_safe_for_read() -> bool {
        true
    }
}
