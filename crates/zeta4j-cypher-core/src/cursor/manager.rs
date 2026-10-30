//! 커서 관리자 모듈

use super::config::PaginationConfig;
use super::cursor::{Cursor, CursorId, CursorState};
use std::collections::HashMap;

/// 커서 관리자
///
/// 여러 커서의 생성, 조회, 정리를 담당합니다.
pub struct CursorManager {
    /// 활성 커서들
    cursors: HashMap<CursorId, Cursor>,
    /// 설정
    config: PaginationConfig,
}

impl CursorManager {
    /// 새 관리자 생성
    pub fn new(config: PaginationConfig) -> Self {
        Self {
            cursors: HashMap::new(),
            config,
        }
    }

    /// 기본 설정으로 관리자 생성
    pub fn with_defaults() -> Self {
        Self::new(PaginationConfig::default())
    }

    /// 커서 생성
    pub fn create_cursor(&mut self, query: &str) -> Result<CursorId, CursorError> {
        self.create_cursor_with_total(query, None)
    }

    /// 총 행 수를 지정하여 커서 생성
    pub fn create_cursor_with_total(
        &mut self,
        query: &str,
        total_rows: Option<usize>,
    ) -> Result<CursorId, CursorError> {
        // 최대 커서 수 확인
        if self.cursors.len() >= self.config.max_cursors {
            // 만료된 커서 정리 시도
            let cleaned = self.cleanup_expired();
            if cleaned == 0 && self.cursors.len() >= self.config.max_cursors {
                return Err(CursorError::TooManyCursors {
                    current: self.cursors.len(),
                    max: self.config.max_cursors,
                });
            }
        }

        let cursor = match total_rows {
            Some(total) => Cursor::with_total_rows(query, self.config.cursor_ttl_seconds, total),
            None => Cursor::new(query, self.config.cursor_ttl_seconds),
        };

        let id = cursor.id().clone();
        self.cursors.insert(id.clone(), cursor);

        Ok(id)
    }

    /// 커서 가져오기
    pub fn get_cursor(&mut self, id: &CursorId) -> Option<&mut Cursor> {
        let cursor = self.cursors.get_mut(id)?;

        // 만료 체크
        if cursor.is_expired() {
            cursor.mark_expired();
            return None;
        }

        cursor.touch();
        Some(cursor)
    }

    /// 커서 조회 (불변 참조)
    pub fn peek_cursor(&self, id: &CursorId) -> Option<&Cursor> {
        let cursor = self.cursors.get(id)?;
        if cursor.is_expired() {
            None
        } else {
            Some(cursor)
        }
    }

    /// 커서 닫기
    pub fn close_cursor(&mut self, id: &CursorId) -> bool {
        if let Some(cursor) = self.cursors.get_mut(id) {
            cursor.close();
            true
        } else {
            false
        }
    }

    /// 커서 제거
    pub fn remove_cursor(&mut self, id: &CursorId) -> Option<Cursor> {
        self.cursors.remove(id)
    }

    /// 만료된 커서 정리
    pub fn cleanup_expired(&mut self) -> usize {
        let expired_ids: Vec<_> = self
            .cursors
            .iter()
            .filter(|(_, cursor)| cursor.is_expired() || cursor.state().is_terminal())
            .map(|(id, _)| id.clone())
            .collect();

        let count = expired_ids.len();
        for id in expired_ids {
            self.cursors.remove(&id);
        }

        count
    }

    /// 모든 커서 정리
    pub fn clear(&mut self) {
        self.cursors.clear();
    }

    /// 활성 커서 수
    pub fn active_count(&self) -> usize {
        self.cursors
            .values()
            .filter(|c| c.state().is_active() && !c.is_expired())
            .count()
    }

    /// 총 커서 수
    pub fn total_count(&self) -> usize {
        self.cursors.len()
    }

    /// 설정 반환
    pub fn config(&self) -> &PaginationConfig {
        &self.config
    }

    /// 설정 변경
    pub fn set_config(&mut self, config: PaginationConfig) {
        self.config = config;
    }

    /// 커서 존재 여부
    pub fn contains(&self, id: &CursorId) -> bool {
        self.cursors.contains_key(id)
    }

    /// 모든 커서 ID 반환
    pub fn cursor_ids(&self) -> Vec<CursorId> {
        self.cursors.keys().cloned().collect()
    }

    /// 통계 정보
    pub fn stats(&self) -> CursorStats {
        let mut stats = CursorStats::default();

        for cursor in self.cursors.values() {
            stats.total += 1;
            match cursor.state() {
                CursorState::Active if !cursor.is_expired() => stats.active += 1,
                CursorState::Exhausted => stats.exhausted += 1,
                CursorState::Expired => stats.expired += 1,
                CursorState::Closed => stats.closed += 1,
                _ => stats.expired += 1, // 만료된 Active도 expired로
            }
        }

        stats.max_allowed = self.config.max_cursors;
        stats
    }
}

impl Default for CursorManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// 커서 오류
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorError {
    /// 커서를 찾을 수 없음
    NotFound(CursorId),
    /// 커서가 만료됨
    Expired(CursorId),
    /// 커서가 이미 닫힘
    Closed(CursorId),
    /// 커서가 완료됨
    Exhausted(CursorId),
    /// 너무 많은 커서
    TooManyCursors { current: usize, max: usize },
}

impl std::fmt::Display for CursorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CursorError::NotFound(id) => write!(f, "Cursor not found: {}", id),
            CursorError::Expired(id) => write!(f, "Cursor expired: {}", id),
            CursorError::Closed(id) => write!(f, "Cursor closed: {}", id),
            CursorError::Exhausted(id) => write!(f, "Cursor exhausted: {}", id),
            CursorError::TooManyCursors { current, max } => {
                write!(f, "Too many cursors: {} (max: {})", current, max)
            }
        }
    }
}

impl std::error::Error for CursorError {}

impl From<CursorError> for zeta4j_common::ZetaError {
    fn from(err: CursorError) -> Self {
        use zeta4j_common::codes;

        match err {
            CursorError::NotFound(id) => zeta4j_common::ZetaError::NotFound {
                code: codes::cypher::CURSOR_NOT_FOUND,
                resource: format!("Cursor: {}", id),
            },
            CursorError::Expired(id) => zeta4j_common::ZetaError::QueryExecution {
                code: codes::cypher::CURSOR_EXPIRED,
                message: format!("Cursor expired: {}", id),
            },
            CursorError::Closed(id) => zeta4j_common::ZetaError::QueryExecution {
                code: codes::cypher::CURSOR_CLOSED,
                message: format!("Cursor already closed: {}", id),
            },
            CursorError::Exhausted(id) => zeta4j_common::ZetaError::QueryExecution {
                code: codes::cypher::CURSOR_EXHAUSTED,
                message: format!("Cursor exhausted: {}", id),
            },
            CursorError::TooManyCursors { current, max } => {
                zeta4j_common::ZetaError::QueryExecution {
                    code: codes::cypher::RESOURCE_EXHAUSTED,
                    message: format!("Too many cursors: {} (max: {})", current, max),
                }
            }
        }
    }
}

/// 커서 통계
#[derive(Debug, Clone, Default)]
pub struct CursorStats {
    /// 총 커서 수
    pub total: usize,
    /// 활성 커서 수
    pub active: usize,
    /// 완료된 커서 수
    pub exhausted: usize,
    /// 만료된 커서 수
    pub expired: usize,
    /// 닫힌 커서 수
    pub closed: usize,
    /// 최대 허용 커서 수
    pub max_allowed: usize,
}

impl CursorStats {
    /// 사용률 (0.0 ~ 1.0)
    pub fn usage_ratio(&self) -> f64 {
        if self.max_allowed == 0 {
            0.0
        } else {
            self.total as f64 / self.max_allowed as f64
        }
    }

    /// 활성 비율 (0.0 ~ 1.0)
    pub fn active_ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.active as f64 / self.total as f64
        }
    }
}

impl std::fmt::Display for CursorStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CursorStats {{ total: {}, active: {}, exhausted: {}, expired: {}, closed: {}, max: {} }}",
            self.total, self.active, self.exhausted, self.expired, self.closed, self.max_allowed
        )
    }
}
