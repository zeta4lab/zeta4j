//! # LSN (Log Sequence Number) 모듈
//!
//! Page-level WAL을 위한 LSN 인프라를 제공합니다.
//!
//! ## 핵심 개념
//!
//! - **LSN**: WAL 레코드의 고유 순번
//! - **PageLSN**: 페이지에 마지막으로 적용된 WAL LSN
//! - **flushedLSN**: 디스크에 기록된 최신 WAL LSN
//!
//! ## ARIES 원칙
//!
//! - WAL 규칙: `PageLSN <= flushedLSN` 보장 후에만 dirty page flush 허용
//! - REDO 조건: `page.LSN < record.LSN` 이면 REDO 수행

use std::sync::atomic::{AtomicU64, Ordering};

/// Log Sequence Number 타입
///
/// WAL 레코드의 고유 식별자로, 단조 증가하는 64비트 정수입니다.
pub type LSN = u64;

/// 무효 LSN 상수
///
/// 아직 WAL에 기록되지 않은 상태를 나타냅니다.
pub const INVALID_LSN: LSN = 0;

/// LSN 생성기
///
/// Lock-free로 LSN을 생성합니다. 동시성 환경에서 안전하게 사용할 수 있습니다.
///
/// # Example
///
/// ```rust
/// use zeta4j_graph::transaction::lsn::LSNGenerator;
///
/// let generator = LSNGenerator::new();
/// let lsn1 = generator.next();
/// let lsn2 = generator.next();
/// assert!(lsn2 > lsn1);
/// ```
#[derive(Debug)]
pub struct LSNGenerator {
    current: AtomicU64,
}

impl LSNGenerator {
    /// 새 LSN 생성기 생성
    ///
    /// LSN은 1부터 시작합니다 (0은 INVALID_LSN).
    pub fn new() -> Self {
        Self {
            current: AtomicU64::new(1),
        }
    }

    /// 지정된 LSN으로 초기화
    ///
    /// WAL 복구 후 마지막 LSN부터 이어서 시작할 때 사용합니다.
    pub fn with_start(start_lsn: LSN) -> Self {
        Self {
            current: AtomicU64::new(start_lsn),
        }
    }

    /// 다음 LSN 생성 (Lock-free)
    ///
    /// 원자적으로 현재 값을 반환하고 1 증가시킵니다.
    #[inline]
    pub fn next(&self) -> LSN {
        self.current.fetch_add(1, Ordering::SeqCst)
    }

    /// 현재 LSN 조회 (다음에 발급될 LSN)
    #[inline]
    pub fn current(&self) -> LSN {
        self.current.load(Ordering::SeqCst)
    }

    /// 마지막으로 발급된 LSN 조회
    #[inline]
    pub fn last_issued(&self) -> LSN {
        let curr = self.current.load(Ordering::SeqCst);
        if curr > 1 { curr - 1 } else { INVALID_LSN }
    }

    /// WAL 복구 후 LSN 재설정
    ///
    /// 복구된 마지막 LSN의 다음 값으로 설정합니다.
    pub fn restore(&self, last_lsn: LSN) {
        self.current.store(last_lsn + 1, Ordering::SeqCst);
    }
}

impl Default for LSNGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// flushedLSN 추적기
///
/// 디스크에 기록된 최신 WAL LSN을 추적합니다.
/// WAL 규칙 검사에 사용됩니다.
#[derive(Debug)]
pub struct FlushedLSNTracker {
    flushed_lsn: AtomicU64,
}

impl FlushedLSNTracker {
    /// 새 추적기 생성
    pub fn new() -> Self {
        Self {
            flushed_lsn: AtomicU64::new(INVALID_LSN),
        }
    }

    /// flushedLSN 업데이트
    ///
    /// WAL flush 완료 후 호출합니다.
    pub fn update(&self, lsn: LSN) {
        // 단조 증가만 허용
        loop {
            let current = self.flushed_lsn.load(Ordering::SeqCst);
            if lsn <= current {
                break;
            }
            if self
                .flushed_lsn
                .compare_exchange(current, lsn, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
    }

    /// 현재 flushedLSN 조회
    #[inline]
    pub fn get(&self) -> LSN {
        self.flushed_lsn.load(Ordering::SeqCst)
    }

    /// 페이지 flush 가능 여부 확인 (WAL 규칙)
    ///
    /// `page_lsn <= flushed_lsn` 이면 flush 가능합니다.
    #[inline]
    pub fn can_flush_page(&self, page_lsn: LSN) -> bool {
        page_lsn <= self.get()
    }
}

impl Default for FlushedLSNTracker {
    fn default() -> Self {
        Self::new()
    }
}
