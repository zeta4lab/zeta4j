//! # Network Core Layer
//!
//! 저수준 네트워크/연결 관리 로직을 담당합니다.
//!
//! ## 모듈 구성
//!
//! - `connection`: 연결 정보 및 추적
//! - `pool`: 연결 풀링
//! - `backpressure`: 백프레셔 제어
//! - `keepalive`: Keep-Alive 관리
//! - `timeout`: 타임아웃 관리
//! - `shutdown`: Graceful Shutdown
//! - `state`: 연결 상태 머신
//! - `tcp`: TCP 서버 기반

pub mod backpressure;
pub mod connection;
pub mod keepalive;
pub mod pool;
pub mod shutdown;
pub mod state;
pub mod tcp;
pub mod timeout;

// Connection exports
pub use connection::{
    Connection, ConnectionId, ConnectionInfo, ConnectionState, ConnectionTracker,
};

// Backpressure exports
pub use backpressure::{BackpressureController, BackpressureUsage, RateLimiter};

// Shutdown exports
pub use shutdown::{ShutdownController, ShutdownGuard, ShutdownReason, ShutdownState};

// TCP Server exports
pub use tcp::{ServerState, TcpServer};

// Connection Pool exports
pub use pool::{
    ConnectionPool, PoolConfig, PoolConfigBuilder, PoolError, PoolStats, PoolStatsSnapshot,
    PoolStatus, Poolable, PooledConnection,
};

// Timeout exports
pub use timeout::{
    TimeoutConfig, TimeoutConfigBuilder, TimeoutEvent, TimeoutGuard, TimeoutManager, TimeoutType,
};

// Keep-Alive exports
pub use keepalive::{
    KeepAliveConfig, KeepAliveConfigBuilder, KeepAliveEvent, KeepAliveManager, KeepAliveState,
};

// State Machine exports
pub use state::{ConnState, ConnectionStateMachine, StateGuard, TransitionResult};
