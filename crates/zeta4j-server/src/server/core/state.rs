// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Connection State Machine
//!
//! Phase 2 Step 1 M1.3: 연결 상태 머신
//!
//! 연결의 상태 전이를 관리하고 유효성을 검증합니다.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// 연결 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ConnState {
    /// 새 연결 (accept 직후)
    New = 0,
    /// TLS 핸드셰이크 중
    TlsHandshake = 1,
    /// 프로토콜 핸드셰이크 중 (Bolt/HTTP)
    ProtocolHandshake = 2,
    /// 인증 중
    Authenticating = 3,
    /// 준비 완료 (요청 대기)
    Ready = 4,
    /// 요청 처리 중
    Processing = 5,
    /// 트랜잭션 진행 중
    InTransaction = 6,
    /// 결과 스트리밍 중
    Streaming = 7,
    /// 종료 중 (Graceful)
    Closing = 8,
    /// 오류 상태
    Error = 9,
    /// 종료됨
    Closed = 10,
}

impl ConnState {
    /// 숫자에서 상태로 변환
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::New),
            1 => Some(Self::TlsHandshake),
            2 => Some(Self::ProtocolHandshake),
            3 => Some(Self::Authenticating),
            4 => Some(Self::Ready),
            5 => Some(Self::Processing),
            6 => Some(Self::InTransaction),
            7 => Some(Self::Streaming),
            8 => Some(Self::Closing),
            9 => Some(Self::Error),
            10 => Some(Self::Closed),
            _ => None,
        }
    }

    /// 활성 상태인지 확인 (종료 아닌 상태)
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Closed | Self::Error)
    }

    /// 요청 처리 가능한 상태인지 확인
    pub fn can_process_request(&self) -> bool {
        matches!(self, Self::Ready | Self::InTransaction)
    }

    /// 종료 가능한 상태인지 확인
    pub fn can_close(&self) -> bool {
        !matches!(self, Self::Closed | Self::Closing)
    }
}

impl std::fmt::Display for ConnState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::New => write!(f, "New"),
            Self::TlsHandshake => write!(f, "TlsHandshake"),
            Self::ProtocolHandshake => write!(f, "ProtocolHandshake"),
            Self::Authenticating => write!(f, "Authenticating"),
            Self::Ready => write!(f, "Ready"),
            Self::Processing => write!(f, "Processing"),
            Self::InTransaction => write!(f, "InTransaction"),
            Self::Streaming => write!(f, "Streaming"),
            Self::Closing => write!(f, "Closing"),
            Self::Error => write!(f, "Error"),
            Self::Closed => write!(f, "Closed"),
        }
    }
}

/// 상태 전이 결과
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionResult {
    /// 성공
    Ok,
    /// 잘못된 전이
    InvalidTransition,
    /// 이미 해당 상태
    AlreadyInState,
}

/// 유효한 상태 전이 정의
const VALID_TRANSITIONS: &[(ConnState, ConnState)] = &[
    // New에서의 전이
    (ConnState::New, ConnState::TlsHandshake),
    (ConnState::New, ConnState::ProtocolHandshake),
    (ConnState::New, ConnState::Closing),
    (ConnState::New, ConnState::Error),
    // TlsHandshake에서의 전이
    (ConnState::TlsHandshake, ConnState::ProtocolHandshake),
    (ConnState::TlsHandshake, ConnState::Closing),
    (ConnState::TlsHandshake, ConnState::Error),
    // ProtocolHandshake에서의 전이
    (ConnState::ProtocolHandshake, ConnState::Authenticating),
    (ConnState::ProtocolHandshake, ConnState::Ready), // 인증 없이 Ready
    (ConnState::ProtocolHandshake, ConnState::Closing),
    (ConnState::ProtocolHandshake, ConnState::Error),
    // Authenticating에서의 전이
    (ConnState::Authenticating, ConnState::Ready),
    (ConnState::Authenticating, ConnState::Closing),
    (ConnState::Authenticating, ConnState::Error),
    // Ready에서의 전이
    (ConnState::Ready, ConnState::Processing),
    (ConnState::Ready, ConnState::InTransaction),
    (ConnState::Ready, ConnState::Closing),
    (ConnState::Ready, ConnState::Error),
    // Processing에서의 전이
    (ConnState::Processing, ConnState::Ready),
    (ConnState::Processing, ConnState::Streaming),
    (ConnState::Processing, ConnState::InTransaction),
    (ConnState::Processing, ConnState::Closing),
    (ConnState::Processing, ConnState::Error),
    // InTransaction에서의 전이
    (ConnState::InTransaction, ConnState::Processing),
    (ConnState::InTransaction, ConnState::Ready), // COMMIT/ROLLBACK
    (ConnState::InTransaction, ConnState::Closing),
    (ConnState::InTransaction, ConnState::Error),
    // Streaming에서의 전이
    (ConnState::Streaming, ConnState::Ready),
    (ConnState::Streaming, ConnState::Processing),
    (ConnState::Streaming, ConnState::Closing),
    (ConnState::Streaming, ConnState::Error),
    // Closing에서의 전이
    (ConnState::Closing, ConnState::Closed),
    (ConnState::Closing, ConnState::Error),
    // Error에서의 전이
    (ConnState::Error, ConnState::Closing),
    (ConnState::Error, ConnState::Closed),
];

/// 상태 전이가 유효한지 확인
fn is_valid_transition(from: ConnState, to: ConnState) -> bool {
    VALID_TRANSITIONS.contains(&(from, to))
}

/// 연결 상태 머신
///
/// 연결의 상태를 추적하고 유효한 전이만 허용합니다.
pub struct ConnectionStateMachine {
    state: AtomicU8,
    last_transition: parking_lot::Mutex<Instant>,
    transition_count: std::sync::atomic::AtomicU64,
}

impl ConnectionStateMachine {
    /// 새 상태 머신 생성
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(ConnState::New as u8),
            last_transition: parking_lot::Mutex::new(Instant::now()),
            transition_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 지정된 상태로 시작
    pub fn with_state(state: ConnState) -> Self {
        Self {
            state: AtomicU8::new(state as u8),
            last_transition: parking_lot::Mutex::new(Instant::now()),
            transition_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 현재 상태
    pub fn state(&self) -> ConnState {
        ConnState::from_u8(self.state.load(Ordering::Acquire)).unwrap_or(ConnState::Error)
    }

    /// 상태 전이 시도
    pub fn transition(&self, to: ConnState) -> TransitionResult {
        loop {
            let current = self.state.load(Ordering::Acquire);
            let from = ConnState::from_u8(current).unwrap_or(ConnState::Error);

            if from == to {
                return TransitionResult::AlreadyInState;
            }

            if !is_valid_transition(from, to) {
                return TransitionResult::InvalidTransition;
            }

            // CAS로 상태 변경
            if self
                .state
                .compare_exchange(current, to as u8, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                *self.last_transition.lock() = Instant::now();
                self.transition_count.fetch_add(1, Ordering::Relaxed);
                return TransitionResult::Ok;
            }
        }
    }

    /// 강제 상태 설정 (검증 없음)
    pub fn force_state(&self, state: ConnState) {
        self.state.store(state as u8, Ordering::Release);
        *self.last_transition.lock() = Instant::now();
        self.transition_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 특정 상태에서 다른 상태로 전이 (CAS)
    pub fn transition_from(&self, from: ConnState, to: ConnState) -> TransitionResult {
        if from == to {
            return TransitionResult::AlreadyInState;
        }

        if !is_valid_transition(from, to) {
            return TransitionResult::InvalidTransition;
        }

        if self
            .state
            .compare_exchange(from as u8, to as u8, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            *self.last_transition.lock() = Instant::now();
            self.transition_count.fetch_add(1, Ordering::Relaxed);
            TransitionResult::Ok
        } else {
            TransitionResult::InvalidTransition
        }
    }

    /// 마지막 전이 이후 경과 시간
    pub fn time_in_state(&self) -> Duration {
        self.last_transition.lock().elapsed()
    }

    /// 총 전이 횟수
    pub fn transition_count(&self) -> u64 {
        self.transition_count.load(Ordering::Relaxed)
    }

    /// 활성 상태인지 확인
    pub fn is_active(&self) -> bool {
        self.state().is_active()
    }

    /// 요청 처리 가능한지 확인
    pub fn can_process_request(&self) -> bool {
        self.state().can_process_request()
    }

    /// 종료 상태인지 확인
    pub fn is_closed(&self) -> bool {
        matches!(self.state(), ConnState::Closed)
    }

    /// 오류 상태인지 확인
    pub fn is_error(&self) -> bool {
        matches!(self.state(), ConnState::Error)
    }
}

impl Default for ConnectionStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ConnectionStateMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionStateMachine")
            .field("state", &self.state())
            .field("transition_count", &self.transition_count())
            .finish()
    }
}

/// 상태 전이 가드
///
/// 스코프를 벗어나면 자동으로 이전 상태로 복원 (오류 시)
pub struct StateGuard<'a> {
    machine: &'a ConnectionStateMachine,
    previous_state: ConnState,
    committed: bool,
}

impl<'a> StateGuard<'a> {
    /// 새 가드 생성 (상태 전이 수행)
    pub fn new(machine: &'a ConnectionStateMachine, to: ConnState) -> Option<Self> {
        let previous = machine.state();

        if machine.transition(to) == TransitionResult::Ok {
            Some(Self {
                machine,
                previous_state: previous,
                committed: false,
            })
        } else {
            None
        }
    }

    /// 상태 전이 커밋 (드롭 시 롤백 안 함)
    pub fn commit(mut self) {
        self.committed = true;
    }

    /// 현재 상태
    pub fn state(&self) -> ConnState {
        self.machine.state()
    }

    /// 이전 상태
    pub fn previous_state(&self) -> ConnState {
        self.previous_state
    }
}

impl<'a> Drop for StateGuard<'a> {
    fn drop(&mut self) {
        if !self.committed {
            // 롤백: 이전 상태로 강제 복원
            self.machine.force_state(self.previous_state);
        }
    }
}
