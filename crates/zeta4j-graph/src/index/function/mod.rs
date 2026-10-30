//! 함수 인덱스 (Function Index) 모듈
//!
//! 계산된 속성 값에 대한 인덱싱을 지원합니다.
//! - `lower(name)`: 소문자 변환 후 인덱싱
//! - `year(date)`: 연도 추출 후 인덱싱
//! - `length(content)`: 문자열 길이 인덱싱

mod evaluator;
mod expression;
mod index;
mod manager;

pub use evaluator::FunctionEvaluator;
pub use expression::FunctionExpression;
pub use index::FunctionIndex;
pub use manager::{FunctionIndexInfo, FunctionIndexManager, FunctionIndexStats};
