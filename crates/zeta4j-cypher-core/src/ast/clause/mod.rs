//! 절(Clause) AST 모듈
//!
//! Cypher 쿼리의 절을 정의합니다.
//!
//! # 구조
//!
//! - [`reading`]: 읽기 절 (MATCH, OPTIONAL MATCH)
//! - [`updating`]: 업데이트 절 (CREATE, DELETE, SET, MERGE, REMOVE)
//! - [`projection`]: 프로젝션 절 (WITH, RETURN, UNWIND)

mod projection;
mod reading;
mod updating;

pub use projection::{
    DatabaseTarget, LetClause, OrderItem, ReturnClause, ReturnItem, UnwindClause, UseClause,
    WithClause,
};
pub use reading::ReadingClause;
pub use updating::{RemoveItem, SetItem, UpdatingClause};
