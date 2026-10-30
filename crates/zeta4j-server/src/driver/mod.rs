//! Driver Module
//!
//! Step 9: 클라이언트 SDK (Rust 드라이버)
//!
//! # Milestones
//!
//! - M9.1: Rust 드라이버 (Driver, Config, AuthToken)
//! - M9.2: 연결 풀링 (ConnectionPool, PoolConfig)
//! - M9.3: 트랜잭션 API (Transaction, TransactionConfig)
//! - M9.4: 세션 관리 (Session, SessionConfig, Bookmark)
//! - M9.5: 라우팅 드라이버 (RoutingDriver, RoutingTable)
//! - M9.6: 리액티브 스트림 (tokio-stream 기반)
//!
//! # Example
//!
//! ```ignore
//! use zeta4j_server::driver::{Driver, AuthToken, SessionConfig};
//!
//! // 단일 서버 드라이버 (bolt://)
//! let driver = Driver::new("bolt://localhost:7687", AuthToken::basic("zeta4j", "password"))?;
//!
//! // 세션 생성
//! let session = driver.session(SessionConfig::default())?;
//!
//! // 쿼리 실행
//! let result = session.run("MATCH (n) RETURN n LIMIT 10", None).await?;
//! for record in result {
//!     println!("{:?}", record);
//! }
//!
//! // 트랜잭션
//! let tx = session.begin_transaction(None).await?;
//! tx.run("CREATE (n:Person {name: $name})", params!{"name" => "Alice"}).await?;
//! tx.commit().await?;
//!
//! // 세션 닫기
//! session.close().await?;
//! driver.close().await?;
//! ```
//!
//! # Routing Driver Example
//!
//! ```ignore
//! use zeta4j_server::driver::routing::{RoutingDriver, RoutingPolicy};
//! use zeta4j_server::driver::{AuthToken, SessionConfig, AccessMode};
//!
//! // 라우팅 드라이버 (zeta4j://) - 클러스터용
//! let driver = RoutingDriver::new(
//!     "zeta4j://server1:7687,server2:7687",
//!     AuthToken::basic("admin", "password")
//! )?;
//!
//! // 읽기 세션 (팔로워로 자동 라우팅)
//! let session = driver.session(
//!     SessionConfig::builder()
//!         .with_default_access_mode(AccessMode::Read)
//!         .build()
//! )?;
//!
//! driver.close().await?;
//! ```

pub mod bolt;
#[allow(clippy::module_inception)]
mod driver;
mod error;
mod pool;
pub mod reactive;
mod record;
pub mod routing;
mod session;
mod transaction;
mod types;

// Re-exports
pub use driver::{
    AuthToken, Driver, DriverConfig, DriverConfigBuilder, DriverMetrics, ServerAddress, ServerInfo,
    TrustStrategy,
};
pub use error::{BoltError, DriverError, DriverResult};
pub use pool::{
    ConnectionPool, ConnectionState, PoolConfig, PoolConfigBuilder, PoolMetrics, PooledConnection,
};
pub use record::{Record, RecordStream};
pub use session::{
    AccessMode, Bookmark, Counters, InputPosition, Notification, Query, QueryResult, QueryType,
    ResultSummary, Session, SessionConfig, SessionConfigBuilder,
};
pub use transaction::{Transaction, TransactionConfig, TransactionState};
pub use types::{Duration, Node, Path, Point, Relationship, Value};

/// 파라미터 맵 생성 매크로
#[macro_export]
macro_rules! params {
    () => {
        std::collections::HashMap::new()
    };
    ($($key:expr => $value:expr),+ $(,)?) => {{
        let mut map = std::collections::HashMap::new();
        $(
            map.insert($key.into(), $crate::driver::Value::from($value));
        )+
        map
    }};
}
