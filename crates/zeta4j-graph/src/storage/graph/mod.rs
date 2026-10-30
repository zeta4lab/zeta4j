//! 그래프 저장소 계층
//!
//! 노드와 엣지를 저장하는 추상화 계층입니다.
//! 메모리/디스크 듀얼 백엔드를 지원합니다.

pub mod common;
pub mod edge_storage;
pub mod node_storage;

pub use common::ElementStorageStats;
pub use edge_storage::{EdgeStorage, EdgeStorageStats};
pub use node_storage::{NodeStorage, NodeStorageStats};
