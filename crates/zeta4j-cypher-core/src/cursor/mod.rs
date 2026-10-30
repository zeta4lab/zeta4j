//! 커서 및 페이지네이션 시스템
//!
//! 대용량 결과 집합을 효율적으로 처리하기 위한 커서 기반 페이지네이션 시스템을 제공합니다.
//!
//! # 페이지네이션 방식
//!
//! 1. **Offset 기반 (SKIP/LIMIT)**: 간단하지만 큰 offset에서 성능 저하
//! 2. **커서 기반**: 상태를 유지하여 일관된 성능 제공
//! 3. **Keyset 기반**: 정렬 키를 이용한 효율적인 페이지네이션
//!
//! # 사용 예시
//!
//! ## 커서 관리자 사용
//!
//! ```rust,ignore
//! use crate::cursor::{CursorManager, PaginationConfig};
//!
//! // 커서 관리자 생성
//! let mut manager = CursorManager::with_defaults();
//!
//! // 커서 생성
//! let cursor_id = manager.create_cursor("MATCH (n:Person) RETURN n")?;
//!
//! // 커서로 데이터 가져오기
//! if let Some(cursor) = manager.get_cursor(&cursor_id) {
//!     cursor.advance(20); // 20개 처리
//!     println!("Position: {}", cursor.position());
//! }
//!
//! // 커서 닫기
//! manager.close_cursor(&cursor_id);
//! ```
//!
//! ## 페이지 요청
//!
//! ```rust,ignore
//! use crate::cursor::{PageRequest, PagedResult, PageInfo};
//!
//! // 첫 페이지 요청
//! let request = PageRequest::first(20);
//!
//! // 페이지 결과 생성
//! let data = vec![1, 2, 3, 4, 5];
//! let result = PagedResult::first_page(data, 20, true);
//!
//! println!("Page {}/{}",
//!     result.page_info().display_page_number(),
//!     result.page_info().total_pages.unwrap_or(0));
//! ```
//!
//! ## 결과 이터레이터
//!
//! ```rust,ignore
//! use crate::cursor::ResultIterator;
//!
//! let mut iter = ResultIterator::from_vec(vec![1, 2, 3, 4, 5]);
//!
//! // 배치로 가져오기
//! let batch1 = iter.take_next(2); // [1, 2]
//! let batch2 = iter.take_next(2); // [3, 4]
//!
//! // 스킵
//! iter.skip(1);
//! ```

mod config;
#[allow(clippy::module_inception)]
mod cursor;
mod fetch;
mod iterator;
mod manager;
mod page;

pub use config::{ConfigError, PaginationConfig};
pub use cursor::{Cursor, CursorId, CursorState};
pub use fetch::{FetchResult, Row};
pub use iterator::{ChunkIterator, IntoResultIterator, ResultIterator};
pub use manager::{CursorError, CursorManager, CursorStats};
pub use page::{PageInfo, PageRequest, PagedResult};
