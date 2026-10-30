//! 복합 인덱스 모듈
//!
//! 여러 속성을 조합하여 인덱싱하는 복합 인덱스를 제공합니다.
//! 다중 조건 쿼리의 성능을 크게 향상시킵니다.
//!
//! # 예시
//!
//! ```ignore
//! use zeta4j_graph::index::composite::{CompositeIndexManager, CompositeIndexDef, CompositeKey};
//! use zeta4j_graph::core::Value;
//! use std::collections::HashMap;
//!
//! // 복합 인덱스 정의 생성
//! let def = CompositeIndexDef::new(
//!     "idx_person_name_age".to_string(),
//!     "Person".to_string(),
//!     vec!["name".to_string(), "age".to_string()],
//! );
//!
//! // 관리자 생성 및 인덱스 추가
//! let mut manager = CompositeIndexManager::new();
//! manager.create_index(def).unwrap();
//!
//! // 노드 삽입 시 자동 인덱싱
//! let mut props = HashMap::new();
//! props.insert("name".to_string(), Value::String("Alice".to_string()));
//! props.insert("age".to_string(), Value::Integer(30));
//! manager.on_node_insert("Person", 1, &props).unwrap();
//!
//! // 복합 조건 검색
//! let found = manager.lookup(
//!     "Person",
//!     &["name".to_string(), "age".to_string()],
//!     &[Value::String("Alice".to_string()), Value::Integer(30)],
//! );
//! ```

pub mod index;
pub mod key;
pub mod manager;

pub use index::{CompositeIndex, CompositeIndexDef, CompositeIndexStats};
pub use key::CompositeKey;
pub use manager::CompositeIndexManager;
