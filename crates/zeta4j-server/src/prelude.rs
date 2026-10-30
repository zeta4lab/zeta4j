//! Zeta4j Server Prelude
//!
//! 서버 개발 시 자주 사용되는 타입들을 한 번에 import할 수 있습니다.
//!
//! # 사용 예제
//!
//! ```rust,ignore
//! use zeta4j_server::prelude::*;
//!
//! // GraphEngine, HttpConfig 등을 바로 사용 가능
//! ```

// =========================================================================
// Core Types (from zeta4j-graph)
// =========================================================================

pub use zeta4j_graph::prelude::*;

// =========================================================================
// Server Types
// =========================================================================

pub use crate::server::{
    bolt::{BoltConfig, BoltServer},
    http::HttpConfig,
};

// =========================================================================
// Config Types
// =========================================================================

pub use crate::{BoltServerConfig, HttpServerConfig, UnifiedServerConfig};
