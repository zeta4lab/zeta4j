//! TX-aware Pin Tracking
//!
//! 트랜잭션별 페이지 pin을 추적하여 장기 실행 트랜잭션이
//! 페이지를 점유하는 문제를 해결합니다.
//!
//! ## 문제점
//!
//! 기존 BufferPoolManager의 pin_count는 단순 카운터로,
//! 어떤 트랜잭션이 페이지를 점유하고 있는지 알 수 없습니다.
//! 장기 실행 트랜잭션이 abort되어도 pin이 해제되지 않아
//! 버퍼 풀 고갈을 유발할 수 있습니다.
//!
//! ## 해결책
//!
//! `PinTracker`는 (frame_id, tx_id) 쌍을 추적하여:
//! - 트랜잭션 종료(commit/abort) 시 해당 TX의 모든 pin을 정리
//! - 데드락 탐지에 필요한 정보 제공
//! - 장기 실행 TX 모니터링 지원

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use parking_lot::RwLock;

use crate::storage::memory::lru::FrameId;
use crate::transaction::TransactionId;

/// 트랜잭션별 Pin 추적자
///
/// thread-safe한 구현으로 BufferPoolManager와 함께 사용됩니다.
pub struct PinTracker {
    /// frame_id → 해당 프레임을 pin한 트랜잭션 집합
    frame_pins: RwLock<HashMap<FrameId, HashSet<TransactionId>>>,
    /// tx_id → 해당 트랜잭션이 pin한 프레임 집합
    tx_pins: RwLock<HashMap<TransactionId, HashSet<FrameId>>>,
    /// 트랜잭션별 첫 pin 시간 (장기 실행 탐지용)
    tx_start_times: RwLock<HashMap<TransactionId, Instant>>,
}

impl PinTracker {
    /// 새 PinTracker 생성
    pub fn new() -> Self {
        Self {
            frame_pins: RwLock::new(HashMap::new()),
            tx_pins: RwLock::new(HashMap::new()),
            tx_start_times: RwLock::new(HashMap::new()),
        }
    }

    /// 프레임을 특정 트랜잭션에 의해 pin
    ///
    /// 동일한 (frame_id, tx_id) 쌍에 대한 중복 호출은 무시됩니다.
    pub fn pin(&self, frame_id: FrameId, tx_id: TransactionId) {
        // frame_pins 업데이트
        {
            let mut frame_pins = self.frame_pins.write();
            frame_pins.entry(frame_id).or_default().insert(tx_id);
        }

        // tx_pins 업데이트
        {
            let mut tx_pins = self.tx_pins.write();
            let is_first_pin = tx_pins.entry(tx_id).or_default().insert(frame_id);

            // 첫 번째 pin이면 시작 시간 기록
            if is_first_pin {
                let mut start_times = self.tx_start_times.write();
                start_times.entry(tx_id).or_insert_with(Instant::now);
            }
        }
    }

    /// 특정 트랜잭션의 프레임 unpin
    ///
    /// # Returns
    ///
    /// 해당 프레임에 더 이상 pin이 없으면 true
    pub fn unpin(&self, frame_id: FrameId, tx_id: TransactionId) -> bool {
        // frame_pins에서 제거
        let no_more_pins = {
            let mut frame_pins = self.frame_pins.write();
            if let Some(txs) = frame_pins.get_mut(&frame_id) {
                txs.remove(&tx_id);
                if txs.is_empty() {
                    frame_pins.remove(&frame_id);
                    true
                } else {
                    false
                }
            } else {
                true
            }
        };

        // tx_pins에서 제거
        {
            let mut tx_pins = self.tx_pins.write();
            if let Some(frames) = tx_pins.get_mut(&tx_id) {
                frames.remove(&frame_id);
                if frames.is_empty() {
                    tx_pins.remove(&tx_id);
                    // 시작 시간도 제거
                    let mut start_times = self.tx_start_times.write();
                    start_times.remove(&tx_id);
                }
            }
        }

        no_more_pins
    }

    /// 트랜잭션 종료 시 해당 TX의 모든 pin 정리
    ///
    /// commit 또는 abort 시 호출하여 해당 트랜잭션이
    /// 점유한 모든 프레임을 해제합니다.
    ///
    /// # Returns
    ///
    /// 해제된 frame_id 목록 (더 이상 pin이 없는 프레임만)
    pub fn cleanup_tx(&self, tx_id: TransactionId) -> Vec<FrameId> {
        let mut freed_frames = Vec::new();

        // tx_pins에서 해당 TX가 pin한 프레임 목록 가져오기
        let frames_to_unpin: Vec<FrameId> = {
            let mut tx_pins = self.tx_pins.write();
            tx_pins
                .remove(&tx_id)
                .map(|s| s.into_iter().collect())
                .unwrap_or_default()
        };

        // 시작 시간 제거
        {
            let mut start_times = self.tx_start_times.write();
            start_times.remove(&tx_id);
        }

        // 각 프레임에서 해당 TX의 pin 제거
        {
            let mut frame_pins = self.frame_pins.write();
            for frame_id in frames_to_unpin {
                if let Some(txs) = frame_pins.get_mut(&frame_id) {
                    txs.remove(&tx_id);
                    if txs.is_empty() {
                        frame_pins.remove(&frame_id);
                        freed_frames.push(frame_id);
                    }
                }
            }
        }

        freed_frames
    }

    /// 프레임을 pin하고 있는 트랜잭션 목록 조회
    pub fn get_pinning_transactions(&self, frame_id: FrameId) -> Vec<TransactionId> {
        let frame_pins = self.frame_pins.read();
        frame_pins
            .get(&frame_id)
            .map(|txs| txs.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// 트랜잭션이 pin하고 있는 프레임 목록 조회
    pub fn get_pinned_frames(&self, tx_id: TransactionId) -> Vec<FrameId> {
        let tx_pins = self.tx_pins.read();
        tx_pins
            .get(&tx_id)
            .map(|frames| frames.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// 프레임에 pin이 있는지 확인
    pub fn is_pinned(&self, frame_id: FrameId) -> bool {
        let frame_pins = self.frame_pins.read();
        frame_pins
            .get(&frame_id)
            .map(|txs| !txs.is_empty())
            .unwrap_or(false)
    }

    /// 프레임의 pin 카운트 조회
    pub fn pin_count(&self, frame_id: FrameId) -> usize {
        let frame_pins = self.frame_pins.read();
        frame_pins.get(&frame_id).map(|txs| txs.len()).unwrap_or(0)
    }

    /// 장기 실행 트랜잭션 탐지
    ///
    /// 지정된 시간보다 오래 실행 중인 트랜잭션 목록 반환
    ///
    /// # Arguments
    ///
    /// * `threshold_secs` - 장기 실행 임계값 (초)
    pub fn find_long_running_transactions(&self, threshold_secs: u64) -> Vec<(TransactionId, u64)> {
        let start_times = self.tx_start_times.read();
        let threshold = std::time::Duration::from_secs(threshold_secs);
        let now = Instant::now();

        start_times
            .iter()
            .filter_map(|(&tx_id, &start)| {
                let elapsed = now.duration_since(start);
                if elapsed >= threshold {
                    Some((tx_id, elapsed.as_secs()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// 전체 통계 조회
    pub fn stats(&self) -> PinTrackerStats {
        let frame_pins = self.frame_pins.read();
        let tx_pins = self.tx_pins.read();

        PinTrackerStats {
            total_pinned_frames: frame_pins.len(),
            total_active_transactions: tx_pins.len(),
            max_pins_per_frame: frame_pins.values().map(|txs| txs.len()).max().unwrap_or(0),
            max_pins_per_transaction: tx_pins
                .values()
                .map(|frames| frames.len())
                .max()
                .unwrap_or(0),
        }
    }

    /// 모든 상태 초기화
    pub fn clear(&self) {
        self.frame_pins.write().clear();
        self.tx_pins.write().clear();
        self.tx_start_times.write().clear();
    }
}

impl Default for PinTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// PinTracker 통계
#[derive(Debug, Clone)]
pub struct PinTrackerStats {
    /// 현재 pin된 프레임 수
    pub total_pinned_frames: usize,
    /// 활성 트랜잭션 수 (하나 이상의 프레임을 pin)
    pub total_active_transactions: usize,
    /// 단일 프레임에 대한 최대 pin 수
    pub max_pins_per_frame: usize,
    /// 단일 트랜잭션의 최대 pin 수
    pub max_pins_per_transaction: usize,
}
