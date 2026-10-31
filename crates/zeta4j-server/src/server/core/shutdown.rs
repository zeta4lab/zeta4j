// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Graceful Shutdown Controller

use std::time::Duration;
use tokio::signal;
use tokio::sync::{broadcast, watch};

/// Shutdown 이유
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// 정상 종료 요청
    Requested,
    /// SIGINT (Ctrl+C)
    Sigint,
    /// SIGTERM
    Sigterm,
    /// 에러로 인한 종료
    Error,
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Requested => write!(f, "Requested"),
            Self::Sigint => write!(f, "SIGINT"),
            Self::Sigterm => write!(f, "SIGTERM"),
            Self::Error => write!(f, "Error"),
        }
    }
}

/// Shutdown 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownState {
    /// 실행 중
    Running,
    /// Shutdown 시작
    ShuttingDown(ShutdownReason),
    /// 드레인 중
    Draining,
    /// 강제 종료 중
    ForceClosing,
    /// 종료됨
    Terminated,
}

impl std::fmt::Display for ShutdownState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "Running"),
            Self::ShuttingDown(reason) => write!(f, "ShuttingDown({})", reason),
            Self::Draining => write!(f, "Draining"),
            Self::ForceClosing => write!(f, "ForceClosing"),
            Self::Terminated => write!(f, "Terminated"),
        }
    }
}

/// Shutdown 컨트롤러
pub struct ShutdownController {
    /// Shutdown 알림 채널
    notify: broadcast::Sender<ShutdownReason>,
    /// Shutdown 상태 (외부 조회용)
    state_tx: watch::Sender<ShutdownState>,
    state_rx: watch::Receiver<ShutdownState>,
    /// 드레인 타임아웃
    drain_timeout: Duration,
    /// 강제 종료 타임아웃
    force_timeout: Duration,
}

impl ShutdownController {
    /// 새 컨트롤러 생성
    pub fn new(drain_timeout: Duration, force_timeout: Duration) -> Self {
        let (notify, _) = broadcast::channel(1);
        let (state_tx, state_rx) = watch::channel(ShutdownState::Running);

        Self {
            notify,
            state_tx,
            state_rx,
            drain_timeout,
            force_timeout,
        }
    }

    /// 기본 타임아웃으로 생성
    pub fn with_defaults() -> Self {
        Self::new(Duration::from_secs(30), Duration::from_secs(60))
    }

    /// Shutdown 시그널 수신자 생성
    pub fn subscribe(&self) -> broadcast::Receiver<ShutdownReason> {
        self.notify.subscribe()
    }

    /// 상태 감시자 생성
    pub fn watch(&self) -> watch::Receiver<ShutdownState> {
        self.state_rx.clone()
    }

    /// 현재 상태
    pub fn state(&self) -> ShutdownState {
        *self.state_rx.borrow()
    }

    /// 실행 중인지 확인
    pub fn is_running(&self) -> bool {
        matches!(self.state(), ShutdownState::Running)
    }

    /// 종료 중인지 확인
    pub fn is_shutting_down(&self) -> bool {
        !matches!(self.state(), ShutdownState::Running)
    }

    /// Shutdown 트리거
    pub fn trigger(&self, reason: ShutdownReason) {
        let _ = self.state_tx.send(ShutdownState::ShuttingDown(reason));
        let _ = self.notify.send(reason);
        tracing::info!("Shutdown triggered: {}", reason);
    }

    /// 시스템 시그널 핸들러 시작
    pub async fn wait_for_signal(&self) {
        let ctrl_c = signal::ctrl_c();

        #[cfg(unix)]
        let terminate = async {
            let mut signal = signal::unix::signal(signal::unix::SignalKind::terminate())?;
            signal.recv().await;
            Ok::<(), std::io::Error>(())
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<Result<(), std::io::Error>>();

        tokio::select! {
            result = ctrl_c => {
                match result {
                    Ok(()) => {
                        tracing::info!("Received SIGINT (Ctrl+C)");
                        self.trigger(ShutdownReason::Sigint);
                    }
                    Err(error) => {
                        tracing::error!(%error, "failed to install Ctrl+C handler");
                        self.trigger(ShutdownReason::Error);
                    }
                }
            }
            result = terminate => {
                match result {
                    Ok(()) => {
                        tracing::info!("Received SIGTERM");
                        self.trigger(ShutdownReason::Sigterm);
                    }
                    Err(error) => {
                        tracing::error!(%error, "failed to install SIGTERM handler");
                        self.trigger(ShutdownReason::Error);
                    }
                }
            }
        }
    }

    /// 드레인 상태로 전환
    pub fn start_draining(&self) {
        let _ = self.state_tx.send(ShutdownState::Draining);
    }

    /// 드레인 대기
    pub async fn wait_for_drain<F, Fut>(&self, check_fn: F) -> bool
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        self.start_draining();

        let result = tokio::time::timeout(self.drain_timeout, async {
            loop {
                if check_fn().await {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        result.unwrap_or(false)
    }

    /// 강제 종료 상태로 전환
    pub fn start_force_closing(&self) {
        let _ = self.state_tx.send(ShutdownState::ForceClosing);
    }

    /// 강제 종료 대기
    pub async fn wait_for_force<F, Fut>(&self, close_fn: F) -> bool
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        self.start_force_closing();

        let remaining = self.force_timeout.saturating_sub(self.drain_timeout);
        tokio::time::timeout(remaining, close_fn()).await.is_ok()
    }

    /// 종료 완료
    pub fn terminate(&self) {
        let _ = self.state_tx.send(ShutdownState::Terminated);
        tracing::info!("Shutdown complete");
    }

    /// 드레인 타임아웃
    pub fn drain_timeout(&self) -> Duration {
        self.drain_timeout
    }

    /// 강제 종료 타임아웃
    pub fn force_timeout(&self) -> Duration {
        self.force_timeout
    }
}

/// Shutdown 가드 (RAII)
///
/// 드롭 시 자동으로 종료 알림
#[derive(Default)]
pub struct ShutdownGuard {
    tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ShutdownGuard {
    /// 새 가드 생성
    pub fn new() -> (Self, tokio::sync::oneshot::Receiver<()>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (Self { tx: Some(tx) }, rx)
    }

    /// 가드 해제 (종료 알림 안 함)
    pub fn disarm(mut self) {
        self.tx.take();
    }

    /// 가드 활성화 여부
    pub fn is_armed(&self) -> bool {
        self.tx.is_some()
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(());
        }
    }
}
