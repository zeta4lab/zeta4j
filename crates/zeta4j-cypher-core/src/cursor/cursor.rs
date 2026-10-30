//! 커서 및 커서 상태 정의 모듈

use std::time::Instant;
use uuid::Uuid;

/// 커서 ID
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CursorId(String);

impl CursorId {
    /// 새 커서 ID 생성
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// 문자열에서 커서 ID 생성
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// 커서 ID 문자열 반환
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CursorId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CursorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 커서 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorState {
    /// 활성 상태 - 더 많은 결과를 가져올 수 있음
    Active,
    /// 완료됨 - 모든 결과를 가져옴
    Exhausted,
    /// 만료됨 - TTL 초과
    Expired,
    /// 닫힘 - 명시적으로 닫음
    Closed,
}

impl CursorState {
    /// 활성 상태인지 확인
    pub fn is_active(&self) -> bool {
        matches!(self, CursorState::Active)
    }

    /// 더 이상 사용할 수 없는 상태인지 확인
    pub fn is_terminal(&self) -> bool {
        !self.is_active()
    }
}

impl std::fmt::Display for CursorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CursorState::Active => write!(f, "Active"),
            CursorState::Exhausted => write!(f, "Exhausted"),
            CursorState::Expired => write!(f, "Expired"),
            CursorState::Closed => write!(f, "Closed"),
        }
    }
}

/// 커서
///
/// 대용량 결과 집합을 효율적으로 순회하기 위한 상태 유지 객체입니다.
#[derive(Debug, Clone)]
pub struct Cursor {
    /// 커서 ID
    id: CursorId,
    /// 쿼리 문자열
    query: String,
    /// 현재 위치 (0-based)
    position: usize,
    /// 총 행 수 (알려진 경우)
    total_rows: Option<usize>,
    /// 상태
    state: CursorState,
    /// 생성 시간
    created_at: Instant,
    /// 마지막 접근 시간
    last_accessed: Instant,
    /// TTL (초)
    ttl_seconds: u64,
}

impl Cursor {
    /// 새 커서 생성
    pub fn new(query: impl Into<String>, ttl_seconds: u64) -> Self {
        let now = Instant::now();
        Self {
            id: CursorId::new(),
            query: query.into(),
            position: 0,
            total_rows: None,
            state: CursorState::Active,
            created_at: now,
            last_accessed: now,
            ttl_seconds,
        }
    }

    /// 총 행 수를 지정하여 커서 생성
    pub fn with_total_rows(query: impl Into<String>, ttl_seconds: u64, total_rows: usize) -> Self {
        let mut cursor = Self::new(query, ttl_seconds);
        cursor.total_rows = Some(total_rows);
        cursor
    }

    /// 커서 ID 반환
    pub fn id(&self) -> &CursorId {
        &self.id
    }

    /// 쿼리 문자열 반환
    pub fn query(&self) -> &str {
        &self.query
    }

    /// 현재 위치 반환
    pub fn position(&self) -> usize {
        self.position
    }

    /// 총 행 수 반환
    pub fn total_rows(&self) -> Option<usize> {
        self.total_rows
    }

    /// 상태 반환
    pub fn state(&self) -> CursorState {
        self.state
    }

    /// 생성 시간 반환
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// 마지막 접근 시간 반환
    pub fn last_accessed(&self) -> Instant {
        self.last_accessed
    }

    /// TTL 반환 (초)
    pub fn ttl_seconds(&self) -> u64 {
        self.ttl_seconds
    }

    /// 활성 상태인지 확인
    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    /// 만료 여부 확인
    pub fn is_expired(&self) -> bool {
        if self.state == CursorState::Expired {
            return true;
        }
        // TTL이 0이면 즉시 만료로 간주 (어떤 시간이 경과해도 만료)
        // 그 외에는 경과 시간이 TTL을 초과하면 만료
        let elapsed = self.last_accessed.elapsed();
        if self.ttl_seconds == 0 {
            // 1ms라도 경과했으면 만료
            elapsed.as_millis() > 0
        } else {
            elapsed.as_secs() >= self.ttl_seconds
        }
    }

    /// 더 많은 결과가 있는지 확인
    pub fn has_more(&self) -> bool {
        if !self.is_active() {
            return false;
        }
        match self.total_rows {
            Some(total) => self.position < total,
            None => true, // 총 행 수를 모르면 있다고 가정
        }
    }

    /// 남은 행 수 반환
    pub fn remaining_rows(&self) -> Option<usize> {
        self.total_rows
            .map(|total| total.saturating_sub(self.position))
    }

    /// 접근 시간 갱신
    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
    }

    /// 위치 이동
    pub fn advance(&mut self, count: usize) {
        self.position += count;
        self.touch();

        // 총 행 수를 알고 있고 위치가 그 이상이면 완료 상태로
        if let Some(total) = self.total_rows
            && self.position >= total
        {
            self.state = CursorState::Exhausted;
        }
    }

    /// 완료 상태로 설정
    pub fn mark_exhausted(&mut self) {
        self.state = CursorState::Exhausted;
        self.touch();
    }

    /// 만료 상태로 설정
    pub fn mark_expired(&mut self) {
        self.state = CursorState::Expired;
    }

    /// 커서 닫기
    pub fn close(&mut self) {
        self.state = CursorState::Closed;
    }

    /// 총 행 수 설정
    pub fn set_total_rows(&mut self, total: usize) {
        self.total_rows = Some(total);
        if self.position >= total {
            self.state = CursorState::Exhausted;
        }
    }

    /// 경과 시간 (밀리초)
    pub fn age_ms(&self) -> u128 {
        self.created_at.elapsed().as_millis()
    }

    /// 마지막 접근 후 경과 시간 (밀리초)
    pub fn idle_ms(&self) -> u128 {
        self.last_accessed.elapsed().as_millis()
    }
}
