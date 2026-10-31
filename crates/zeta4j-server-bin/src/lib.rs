// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Zeta4j Base Edition
//!
//! Core + Server
//!
//! # 포함 기능
//!
//! - **Core**: 그래프 엔진, Cypher 파서, 스토리지
//! - **Server**: HTTP/Bolt 서버, 멀티테넌시
//! - **CLI**: 공통 CLI 모듈

// =============================================================================
// Re-exports
// =============================================================================

/// Phase 1: Core Graph Engine
pub use zeta4j_graph as core;

/// Phase 2: HTTP/Bolt Server
pub use zeta4j_server as server;
