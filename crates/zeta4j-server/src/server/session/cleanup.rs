// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Session Cleanup Task
//!
//! Phase 5: 세션 타임아웃 시 자동 트랜잭션 롤백
//!
//! 세션이 만료되면 해당 세션의 활성 트랜잭션을 자동으로 롤백합니다.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;
use tracing::{debug, info, warn};

use super::timeout::{SessionTimeoutManager, TimeoutEvent};
use crate::server::http::HttpTransactionManager;

/// 세션 클린업 태스크 설정
#[derive(Debug, Clone)]
pub struct SessionCleanupConfig {
    /// 클린업 체크 주기 (기본: 60초)
    pub check_interval: Duration,
    /// 트랜잭션 롤백 활성화
    pub auto_rollback_enabled: bool,
}

impl Default for SessionCleanupConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(60),
            auto_rollback_enabled: true,
        }
    }
}

impl SessionCleanupConfig {
    /// 빌더 생성
    pub fn builder() -> SessionCleanupConfigBuilder {
        SessionCleanupConfigBuilder::default()
    }
}

/// 설정 빌더
#[derive(Default)]
pub struct SessionCleanupConfigBuilder {
    config: SessionCleanupConfig,
}

impl SessionCleanupConfigBuilder {
    /// 체크 주기 설정
    pub fn check_interval(mut self, interval: Duration) -> Self {
        self.config.check_interval = interval;
        self
    }

    /// 자동 롤백 활성화 여부
    pub fn auto_rollback_enabled(mut self, enabled: bool) -> Self {
        self.config.auto_rollback_enabled = enabled;
        self
    }

    /// 빌드
    pub fn build(self) -> SessionCleanupConfig {
        self.config
    }
}

/// 세션 클린업 태스크
///
/// 세션 타임아웃 매니저와 트랜잭션 매니저를 연동하여
/// 세션 만료 시 활성 트랜잭션을 자동 롤백합니다.
pub struct SessionCleanupTask {
    /// 설정
    config: SessionCleanupConfig,
    /// 세션 타임아웃 매니저
    session_manager: Arc<SessionTimeoutManager>,
    /// HTTP 트랜잭션 매니저
    txn_manager: Arc<HttpTransactionManager>,
    /// 종료 알림
    shutdown_notify: Arc<Notify>,
}

impl SessionCleanupTask {
    /// 새 클린업 태스크 생성
    pub fn new(
        session_manager: Arc<SessionTimeoutManager>,
        txn_manager: Arc<HttpTransactionManager>,
    ) -> Self {
        Self {
            config: SessionCleanupConfig::default(),
            session_manager,
            txn_manager,
            shutdown_notify: Arc::new(Notify::new()),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(
        config: SessionCleanupConfig,
        session_manager: Arc<SessionTimeoutManager>,
        txn_manager: Arc<HttpTransactionManager>,
    ) -> Self {
        Self {
            config,
            session_manager,
            txn_manager,
            shutdown_notify: Arc::new(Notify::new()),
        }
    }

    /// 태스크 시작 (백그라운드)
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let task = Arc::clone(&self);

        // 세션 타임아웃 이벤트 핸들러 등록
        if self.config.auto_rollback_enabled {
            let txn_manager = Arc::clone(&self.txn_manager);
            self.session_manager.on_timeout(Arc::new(move |event| {
                Self::handle_timeout_event(&txn_manager, event);
            }));
        }

        tokio::spawn(async move {
            task.run().await;
        })
    }

    /// 타임아웃 이벤트 처리
    fn handle_timeout_event(txn_manager: &HttpTransactionManager, event: TimeoutEvent) {
        match event {
            TimeoutEvent::SessionIdle {
                session_id,
                idle_duration,
            } => {
                info!(
                    "Session {} idle timeout after {:?}, rolling back active transactions",
                    session_id, idle_duration
                );
                // 세션의 모든 트랜잭션 롤백
                let rolled_back = txn_manager.cleanup_and_rollback_expired();
                if !rolled_back.is_empty() {
                    info!(
                        "Auto-rolled back {} transactions for session {}",
                        rolled_back.len(),
                        session_id
                    );
                }
            }
            TimeoutEvent::SessionExpired {
                session_id,
                lifetime,
            } => {
                info!(
                    "Session {} expired after {:?}, rolling back active transactions",
                    session_id, lifetime
                );
                let rolled_back = txn_manager.cleanup_and_rollback_expired();
                if !rolled_back.is_empty() {
                    info!(
                        "Auto-rolled back {} transactions for expired session {}",
                        rolled_back.len(),
                        session_id
                    );
                }
            }
            TimeoutEvent::TransactionTimeout {
                session_id,
                tx_id,
                duration,
            } => {
                warn!(
                    "Transaction {} in session {} timed out after {:?}",
                    tx_id, session_id, duration
                );
                // 개별 트랜잭션 롤백은 타임아웃 매니저가 tx_id를 알지 못하므로
                // cleanup_and_rollback_expired로 처리
                let _ = txn_manager.cleanup_and_rollback_expired();
            }
            TimeoutEvent::QueryTimeout {
                session_id,
                query_id,
                duration,
            } => {
                debug!(
                    "Query {} in session {} timed out after {:?}",
                    query_id, session_id, duration
                );
                // 쿼리 타임아웃은 트랜잭션에 영향 없음
            }
        }
    }

    /// 클린업 루프 실행
    async fn run(&self) {
        info!("Session cleanup task started");

        let mut ticker = tokio::time::interval(self.config.check_interval);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.perform_cleanup().await;
                }
                _ = self.shutdown_notify.notified() => {
                    info!("Session cleanup task shutting down");
                    break;
                }
            }
        }
    }

    /// 주기적 클린업 수행
    async fn perform_cleanup(&self) {
        // 만료된 트랜잭션 롤백
        let rolled_back = self.txn_manager.cleanup_and_rollback_expired();
        if !rolled_back.is_empty() {
            info!(
                "Periodic cleanup: rolled back {} expired transactions",
                rolled_back.len()
            );
        }

        // 세션 타임아웃 체크 (이벤트 발생)
        let events = self.session_manager.check_timeouts();
        if !events.is_empty() {
            debug!(
                "Periodic cleanup: processed {} timeout events",
                events.len()
            );
        }
    }

    /// 태스크 종료
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_waiters();
    }

    /// 설정 조회
    pub fn config(&self) -> &SessionCleanupConfig {
        &self.config
    }
}

impl std::fmt::Debug for SessionCleanupTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionCleanupTask")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}
