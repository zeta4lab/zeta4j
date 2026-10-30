//! 쿼리 힌트 시스템
//!
//! openCypher 호환 USING 절을 지원합니다:
//! - `USING INDEX var:Label(property)` - 특정 인덱스 강제 사용
//! - `USING SCAN var:Label` - 레이블 스캔 강제 (인덱스 무시)
//! - `USING JOIN ON var` - 조인 전략 힌트
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use crate::hint::{HintManager, HintParser, QueryHint};
//!
//! // 힌트 파싱
//! let hint = HintParser::parse("USING INDEX p:Person(name)")?;
//!
//! // 힌트 매니저로 전체 프로세스
//! let manager = HintManager::new();
//! let result = manager.process(
//!     "USING INDEX p:Person(name)",
//!     &mut plan,
//!     &query_variables,
//!     &available_indexes,
//! )?;
//! ```

mod applier;
mod manager;
mod parser;
mod types;
mod validator;

pub use applier::HintApplier;
pub use manager::HintManager;
pub use parser::HintParser;
pub use types::{HintApplication, HintProcessResult, HintValidation, IndexInfo, QueryHint};
pub use validator::HintValidator;
