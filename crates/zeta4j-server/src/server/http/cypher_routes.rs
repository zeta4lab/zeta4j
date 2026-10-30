//! Cypher Routes (M2.2)
//!
//! Cypher 쿼리 엔드포인트 라우트 정의
//! Neo4j HTTP API 호환

use axum::Router;
use axum::routing::{delete, post};
use std::sync::Arc;

use super::cypher_handlers::{self, CypherAppState};

/// Cypher 라우트 생성
///
/// Neo4j HTTP API 호환 엔드포인트:
/// - POST /db/{db_name}/tx/commit - Auto-commit 쿼리
/// - POST /db/{db_name}/tx - 트랜잭션 시작
/// - POST /db/{db_name}/tx/{tx_id} - 트랜잭션 내 쿼리
/// - POST /db/{db_name}/tx/{tx_id}/commit - 트랜잭션 커밋
/// - DELETE /db/{db_name}/tx/{tx_id} - 트랜잭션 롤백
/// - POST /db/{db_name}/query/v2 - Neo4j 5.x Query API
pub fn cypher_routes(state: Arc<CypherAppState>) -> Router {
    Router::new()
        // Auto-commit 쿼리 (가장 일반적인 사용)
        .route(
            "/db/{db_name}/tx/commit",
            post(cypher_handlers::auto_commit),
        )
        // 트랜잭션 관리
        .route("/db/{db_name}/tx", post(cypher_handlers::begin_transaction))
        .route(
            "/db/{db_name}/tx/{tx_id}",
            post(cypher_handlers::execute_in_transaction),
        )
        .route(
            "/db/{db_name}/tx/{tx_id}/commit",
            post(cypher_handlers::commit_transaction),
        )
        .route(
            "/db/{db_name}/tx/{tx_id}",
            delete(cypher_handlers::rollback_transaction),
        )
        // Neo4j 5.x Query API v2
        .route("/db/{db_name}/query/v2", post(cypher_handlers::query_v2))
        .with_state(state)
}
