// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bookmark Management
//!
//! Phase 2 Step 5 M5.2: 북마크 관리
//!
//! Bolt 세션의 인과적 일관성을 위한 북마크 관리 시스템
//! - 트랜잭션 커밋 후 북마크 생성
//! - 세션간 인과적 일관성 보장
//! - 북마크 대기 기능

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// 북마크 에러
#[derive(Debug, thiserror::Error)]
pub enum BookmarkError {
    #[error("Bookmark wait timeout after {0:?}")]
    WaitTimeout(Duration),

    #[error("Invalid bookmark format: {0}")]
    InvalidFormat(String),

    #[error("Bookmark not found: {0}")]
    NotFound(String),

    #[error("Database mismatch: expected {expected}, got {actual}")]
    DatabaseMismatch { expected: String, actual: String },
}

/// 북마크 대기 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkWaitResult {
    /// 성공적으로 대기 완료
    Success,
    /// 타임아웃
    Timeout,
    /// 북마크 없음 (대기 불필요)
    NoBookmarks,
}

/// 북마크
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// 북마크 값 (예: "zeta4j:bookmark:v1:tx123")
    value: String,
    /// 데이터베이스 이름
    database: String,
    /// 트랜잭션 ID (추출된)
    tx_id: u64,
    /// 생성 시간
    #[serde(skip, default = "Instant::now")]
    created_at: Instant,
}

impl Bookmark {
    /// 북마크 생성
    pub fn new(database: &str, tx_id: u64) -> Self {
        let value = format!("{}:bookmark:v1:tx{}", database, tx_id);
        Self {
            value,
            database: database.to_string(),
            tx_id,
            created_at: Instant::now(),
        }
    }

    /// 문자열에서 북마크 파싱
    pub fn parse(value: &str) -> Result<Self, BookmarkError> {
        // 형식: {database}:bookmark:v1:tx{id}

        let parts: Vec<&str> = value.split(':').collect();
        if parts.len() < 4 {
            return Err(BookmarkError::InvalidFormat(value.to_string()));
        }

        let database = parts[0].to_string();

        if parts[1] != "bookmark" || parts[2] != "v1" {
            return Err(BookmarkError::InvalidFormat(value.to_string()));
        }

        let tx_part = parts[3];
        if !tx_part.starts_with("tx") {
            return Err(BookmarkError::InvalidFormat(value.to_string()));
        }

        let tx_id: u64 = tx_part[2..]
            .parse()
            .map_err(|_| BookmarkError::InvalidFormat(value.to_string()))?;

        Ok(Self {
            value: value.to_string(),
            database,
            tx_id,
            created_at: Instant::now(),
        })
    }

    /// 북마크 값
    pub fn value(&self) -> &str {
        &self.value
    }

    /// 데이터베이스 이름
    pub fn database(&self) -> &str {
        &self.database
    }

    /// 트랜잭션 ID
    pub fn tx_id(&self) -> u64 {
        self.tx_id
    }

    /// 생성 시간
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// 수명
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

impl std::fmt::Display for Bookmark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl PartialEq for Bookmark {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Bookmark {}

impl std::hash::Hash for Bookmark {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

/// 북마크 관리자
///
/// 세션의 북마크를 관리하고 인과적 일관성을 보장
#[derive(Debug)]
pub struct BookmarkManager {
    /// 데이터베이스별 북마크 (마지막 북마크만 유지)
    bookmarks: HashMap<String, Bookmark>,
    /// 마지막 갱신 시간
    last_updated: Option<Instant>,
    /// 최대 저장 북마크 수 (데이터베이스당)
    #[expect(dead_code)]
    max_bookmarks: usize,
}

impl BookmarkManager {
    /// 새 북마크 관리자 생성
    pub fn new() -> Self {
        Self {
            bookmarks: HashMap::new(),
            last_updated: None,
            max_bookmarks: 10,
        }
    }

    /// 최대 북마크 수 설정
    pub fn with_max_bookmarks(max: usize) -> Self {
        Self {
            bookmarks: HashMap::new(),
            last_updated: None,
            max_bookmarks: max,
        }
    }

    /// 북마크 추가/업데이트
    pub fn add_bookmark(&mut self, bookmark: Bookmark) {
        let db = bookmark.database.clone();

        // 기존 북마크가 있으면 더 최신인 경우에만 업데이트
        if let Some(existing) = self.bookmarks.get(&db) {
            if bookmark.tx_id > existing.tx_id {
                self.bookmarks.insert(db, bookmark);
            }
        } else {
            self.bookmarks.insert(db, bookmark);
        }

        self.last_updated = Some(Instant::now());
    }

    /// 트랜잭션 커밋 후 북마크 생성
    pub fn create_bookmark(&mut self, database: &str, tx_id: u64) -> Bookmark {
        let bookmark = Bookmark::new(database, tx_id);
        self.add_bookmark(bookmark.clone());
        bookmark
    }

    /// 특정 데이터베이스의 북마크 조회
    pub fn get_bookmark(&self, database: &str) -> Option<&Bookmark> {
        self.bookmarks.get(database)
    }

    /// 특정 데이터베이스의 북마크들 조회 (문자열 배열)
    pub fn get_bookmarks(&self, database: &str) -> Vec<String> {
        self.bookmarks
            .get(database)
            .map(|b| vec![b.value.clone()])
            .unwrap_or_default()
    }

    /// 모든 북마크 조회
    pub fn all_bookmarks(&self) -> Vec<&Bookmark> {
        self.bookmarks.values().collect()
    }

    /// 모든 북마크 문자열 조회
    pub fn all_bookmark_values(&self) -> Vec<String> {
        self.bookmarks.values().map(|b| b.value.clone()).collect()
    }

    /// 북마크 문자열 설정 (클라이언트로부터)
    pub fn set_bookmarks(&mut self, bookmark_values: &[String]) -> Result<(), BookmarkError> {
        for value in bookmark_values {
            let bookmark = Bookmark::parse(value)?;
            self.add_bookmark(bookmark);
        }
        Ok(())
    }

    /// 북마크 대기 (동기화)
    ///
    /// 지정된 북마크의 트랜잭션이 적용될 때까지 대기
    /// 참고: 실제 분산 환경에서는 복제 상태를 체크해야 함
    pub fn wait_for_bookmarks(
        &self,
        bookmarks: &[String],
        _timeout: Duration,
    ) -> Result<BookmarkWaitResult, BookmarkError> {
        if bookmarks.is_empty() {
            return Ok(BookmarkWaitResult::NoBookmarks);
        }

        // 단일 노드에서는 북마크가 즉시 적용된 것으로 간주
        // 분산 환경에서는 복제 완료를 확인해야 함
        for bookmark_str in bookmarks {
            let bookmark = Bookmark::parse(bookmark_str)?;

            // 해당 데이터베이스의 현재 북마크 확인
            if let Some(current) = self.bookmarks.get(&bookmark.database) {
                // 현재 북마크가 요청된 것보다 최신인지 확인
                if current.tx_id >= bookmark.tx_id {
                    continue;
                }
            }

            // 단일 노드에서는 항상 성공으로 처리
            // 실제 구현에서는 복제 상태 확인 필요
        }

        Ok(BookmarkWaitResult::Success)
    }

    /// 북마크 대기 (비동기)
    pub async fn wait_for_bookmarks_async(
        &self,
        bookmarks: &[String],
        timeout: Duration,
    ) -> Result<BookmarkWaitResult, BookmarkError> {
        if bookmarks.is_empty() {
            return Ok(BookmarkWaitResult::NoBookmarks);
        }

        // 타임아웃 적용
        let result = tokio::time::timeout(timeout, async {
            // 단일 노드에서는 즉시 완료
            // 분산 환경에서는 복제 완료 대기 로직 필요
            for bookmark_str in bookmarks {
                let _ = Bookmark::parse(bookmark_str)?;
            }
            Ok::<_, BookmarkError>(BookmarkWaitResult::Success)
        })
        .await;

        match result {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(e),
            Err(_) => Ok(BookmarkWaitResult::Timeout),
        }
    }

    /// 북마크 삭제
    pub fn remove_bookmark(&mut self, database: &str) -> Option<Bookmark> {
        self.bookmarks.remove(database)
    }

    /// 모든 북마크 삭제
    pub fn clear(&mut self) {
        self.bookmarks.clear();
        self.last_updated = None;
    }

    /// 북마크 수
    pub fn count(&self) -> usize {
        self.bookmarks.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.bookmarks.is_empty()
    }

    /// 마지막 갱신 시간
    pub fn last_updated(&self) -> Option<Instant> {
        self.last_updated
    }
}

impl Default for BookmarkManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 글로벌 북마크 트래커
///
/// 서버 전체의 커밋된 트랜잭션을 추적하여 북마크 검증에 사용
#[derive(Debug)]
pub struct GlobalBookmarkTracker {
    /// 데이터베이스별 마지막 커밋 트랜잭션 ID
    last_committed: HashMap<String, AtomicU64>,
}

impl GlobalBookmarkTracker {
    /// 새 트래커 생성
    pub fn new() -> Self {
        Self {
            last_committed: HashMap::new(),
        }
    }

    /// 트랜잭션 커밋 기록
    pub fn record_commit(&mut self, database: &str, tx_id: u64) {
        if let Some(counter) = self.last_committed.get(database) {
            let current = counter.load(Ordering::Acquire);
            if tx_id > current {
                counter.store(tx_id, Ordering::Release);
            }
        } else {
            self.last_committed
                .insert(database.to_string(), AtomicU64::new(tx_id));
        }
    }

    /// 마지막 커밋 ID 조회
    pub fn last_committed(&self, database: &str) -> Option<u64> {
        self.last_committed
            .get(database)
            .map(|c| c.load(Ordering::Acquire))
    }

    /// 북마크가 적용되었는지 확인
    pub fn is_bookmark_applied(&self, bookmark: &Bookmark) -> bool {
        if let Some(last) = self.last_committed(&bookmark.database) {
            last >= bookmark.tx_id
        } else {
            false
        }
    }
}

impl Default for GlobalBookmarkTracker {
    fn default() -> Self {
        Self::new()
    }
}
