// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! ARC (Adaptive Replacement Cache) Replacer
//!
//! IBM에서 개발한 자가 적응형 캐시 교체 알고리즘입니다.
//! LRU와 LFU의 장점을 결합하여 다양한 워크로드에 자동 적응합니다.
//!
//! ## 알고리즘 개요
//!
//! ARC는 4개의 리스트를 유지합니다:
//! - **T1**: 최근 접근 캐시 (1번 접근된 항목)
//! - **T2**: 빈번 접근 캐시 (2번 이상 접근된 항목)
//! - **B1**: T1에서 제거된 고스트 항목
//! - **B2**: T2에서 제거된 고스트 항목
//!
//! 적응 파라미터 `p`는 T1의 목표 크기를 나타내며,
//! 히트/미스 패턴에 따라 동적으로 조정됩니다.
//!
//! ## 시간 복잡도
//! - 모든 연산: O(1)
//!
//! ## 참고
//! - Megiddo, N., & Modha, D. S. (2003). ARC: A Self-Tuning, Low Overhead Replacement Cache.
//!   Proceedings of FAST'03.

use std::collections::{HashMap, VecDeque};

/// 프레임 ID (버퍼 풀 내의 인덱스)
pub type FrameId = usize;

/// ARC 리스트 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListType {
    /// 최근 접근 캐시 (1번 접근)
    T1,
    /// 빈번 접근 캐시 (2번+ 접근)
    T2,
    /// T1 고스트
    B1,
    /// T2 고스트
    B2,
}

/// ARC Replacer
///
/// 적응형 교체 캐시로, 접근 패턴에 따라 자동으로 최적화됩니다.
#[derive(Debug)]
pub struct ARCReplacer {
    /// 캐시 용량 (T1 + T2의 최대 크기)
    capacity: usize,

    /// 적응 파라미터 (T1의 목표 크기, 0 <= p <= capacity)
    p: usize,

    /// T1: 최근 접근 리스트 (LRU 순서, front=LRU, back=MRU)
    t1: VecDeque<FrameId>,

    /// T2: 빈번 접근 리스트 (LRU 순서)
    t2: VecDeque<FrameId>,

    /// B1: T1 고스트 리스트
    b1: VecDeque<FrameId>,

    /// B2: T2 고스트 리스트
    b2: VecDeque<FrameId>,

    /// 프레임 위치 매핑 (O(1) 조회)
    location: HashMap<FrameId, ListType>,

    /// Pin된 프레임 집합 (교체 불가)
    pinned: HashMap<FrameId, ListType>,

    /// 통계: T1 히트
    stats_t1_hits: u64,

    /// 통계: T2 히트
    stats_t2_hits: u64,

    /// 통계: B1 히트 (적응 트리거)
    stats_b1_hits: u64,

    /// 통계: B2 히트 (적응 트리거)
    stats_b2_hits: u64,

    /// 통계: 미스
    stats_misses: u64,
}

impl ARCReplacer {
    /// 새 ARC Replacer 생성
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            p: 0,
            t1: VecDeque::with_capacity(capacity),
            t2: VecDeque::with_capacity(capacity),
            b1: VecDeque::with_capacity(capacity),
            b2: VecDeque::with_capacity(capacity),
            location: HashMap::with_capacity(capacity * 2),
            pinned: HashMap::new(),
            stats_t1_hits: 0,
            stats_t2_hits: 0,
            stats_b1_hits: 0,
            stats_b2_hits: 0,
            stats_misses: 0,
        }
    }

    /// 캐시 용량
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 현재 캐시 크기 (T1 + T2)
    pub fn size(&self) -> usize {
        self.t1.len() + self.t2.len()
    }

    /// 교체 가능한 크기 (pinned 제외)
    pub fn evictable_size(&self) -> usize {
        self.size() - self.pinned.len()
    }

    /// 적응 파라미터 p 조회
    pub fn adaptation_parameter(&self) -> usize {
        self.p
    }

    /// 통계 조회
    pub fn stats(&self) -> ARCStats {
        ARCStats {
            t1_size: self.t1.len(),
            t2_size: self.t2.len(),
            b1_size: self.b1.len(),
            b2_size: self.b2.len(),
            p: self.p,
            t1_hits: self.stats_t1_hits,
            t2_hits: self.stats_t2_hits,
            b1_hits: self.stats_b1_hits,
            b2_hits: self.stats_b2_hits,
            misses: self.stats_misses,
            pinned_count: self.pinned.len(),
        }
    }

    /// 프레임 접근 (캐시에 추가/업데이트)
    ///
    /// 접근 패턴에 따라 적응 파라미터를 조정합니다.
    pub fn access(&mut self, frame_id: FrameId) {
        if let Some(&list_type) = self.location.get(&frame_id) {
            match list_type {
                ListType::T1 => {
                    // T1에서 T2로 승격
                    self.stats_t1_hits += 1;
                    self.remove_from_list(frame_id, ListType::T1);
                    self.t2.push_back(frame_id);
                    self.location.insert(frame_id, ListType::T2);

                    // pinned 상태 업데이트
                    if self.pinned.contains_key(&frame_id) {
                        self.pinned.insert(frame_id, ListType::T2);
                    }
                }
                ListType::T2 => {
                    // T2 내에서 MRU로 이동
                    self.stats_t2_hits += 1;
                    self.remove_from_list(frame_id, ListType::T2);
                    self.t2.push_back(frame_id);
                }
                ListType::B1 => {
                    // B1 히트: 더 많은 recency 필요
                    self.stats_b1_hits += 1;
                    self.adapt_increase_p();

                    // B1에서 제거하고 T2에 추가
                    self.remove_from_list(frame_id, ListType::B1);

                    // 공간 확보 후 T2에 추가
                    self.ensure_space();
                    self.t2.push_back(frame_id);
                    self.location.insert(frame_id, ListType::T2);
                }
                ListType::B2 => {
                    // B2 히트: 더 많은 frequency 필요
                    self.stats_b2_hits += 1;
                    self.adapt_decrease_p();

                    // B2에서 제거하고 T2에 추가
                    self.remove_from_list(frame_id, ListType::B2);

                    // 공간 확보 후 T2에 추가
                    self.ensure_space();
                    self.t2.push_back(frame_id);
                    self.location.insert(frame_id, ListType::T2);
                }
            }
        } else {
            // 완전히 새로운 항목 (캐시 미스)
            self.stats_misses += 1;

            // 공간 확보
            self.ensure_space();

            // T1에 추가 (새 항목은 recency 리스트에)
            if self.t1.len() + self.t2.len() < self.capacity {
                self.t1.push_back(frame_id);
                self.location.insert(frame_id, ListType::T1);
            }
        }
    }

    /// 프레임 Pin (교체 대상에서 제외)
    pub fn pin(&mut self, frame_id: FrameId) {
        if let Some(&list_type) = self.location.get(&frame_id)
            && (list_type == ListType::T1 || list_type == ListType::T2)
        {
            self.pinned.insert(frame_id, list_type);
        }
    }

    /// 프레임 Unpin (교체 대상에 추가)
    pub fn unpin(&mut self, frame_id: FrameId) {
        self.pinned.remove(&frame_id);
    }

    /// Victim 선정 (교체할 프레임 반환)
    ///
    /// 교체 대상이 없으면 None 반환.
    pub fn victim(&mut self) -> Option<FrameId> {
        // T1에서 unpinned victim 찾기
        if let Some(victim) = self.find_victim_in_t1() {
            // T1에서 제거하고 B1에 추가
            self.remove_from_list(victim, ListType::T1);
            self.add_to_ghost(victim, ListType::B1);
            return Some(victim);
        }

        // T2에서 unpinned victim 찾기
        if let Some(victim) = self.find_victim_in_t2() {
            // T2에서 제거하고 B2에 추가
            self.remove_from_list(victim, ListType::T2);
            self.add_to_ghost(victim, ListType::B2);
            return Some(victim);
        }

        None
    }

    /// T1에서 교체 대상 찾기 (LRU 순서, pinned 제외)
    fn find_victim_in_t1(&self) -> Option<FrameId> {
        // p 이하일 때만 T1에서 제거
        if self.t1.len() > self.p || self.t2.is_empty() {
            for &frame_id in &self.t1 {
                if !self.pinned.contains_key(&frame_id) {
                    return Some(frame_id);
                }
            }
        }
        None
    }

    /// T2에서 교체 대상 찾기 (LRU 순서, pinned 제외)
    fn find_victim_in_t2(&self) -> Option<FrameId> {
        self.t2
            .iter()
            .find(|&&frame_id| !self.pinned.contains_key(&frame_id))
            .copied()
    }

    /// 공간 확보 (필요시 victim 제거)
    fn ensure_space(&mut self) {
        // T1 + T2가 용량에 도달하면 제거 필요
        if self.t1.len() + self.t2.len() >= self.capacity {
            // replace 수행
            self.replace();
        }

        // 고스트 리스트 정리
        self.cleanup_ghosts();
    }

    /// Replace 연산 (ARC의 핵심)
    fn replace(&mut self) {
        if !self.t1.is_empty() && (self.t1.len() > self.p || self.t2.is_empty()) {
            // T1에서 제거 (더 많은 recency 제거)
            if let Some(victim) = self.find_unpinned_lru(&self.t1) {
                self.remove_from_list(victim, ListType::T1);
                self.add_to_ghost(victim, ListType::B1);
            }
        } else if !self.t2.is_empty() {
            // T2에서 제거 (더 많은 frequency 제거)
            if let Some(victim) = self.find_unpinned_lru(&self.t2) {
                self.remove_from_list(victim, ListType::T2);
                self.add_to_ghost(victim, ListType::B2);
            }
        }
    }

    /// 리스트에서 unpinned LRU 항목 찾기
    fn find_unpinned_lru(&self, list: &VecDeque<FrameId>) -> Option<FrameId> {
        list.iter()
            .find(|&&frame_id| !self.pinned.contains_key(&frame_id))
            .copied()
    }

    /// 리스트에서 항목 제거
    fn remove_from_list(&mut self, frame_id: FrameId, list_type: ListType) {
        let list = match list_type {
            ListType::T1 => &mut self.t1,
            ListType::T2 => &mut self.t2,
            ListType::B1 => &mut self.b1,
            ListType::B2 => &mut self.b2,
        };

        if let Some(pos) = list.iter().position(|&id| id == frame_id) {
            list.remove(pos);
        }

        if list_type == ListType::T1 || list_type == ListType::T2 {
            self.location.remove(&frame_id);
            self.pinned.remove(&frame_id);
        } else {
            self.location.remove(&frame_id);
        }
    }

    /// 고스트 리스트에 추가
    fn add_to_ghost(&mut self, frame_id: FrameId, ghost_type: ListType) {
        let list = match ghost_type {
            ListType::B1 => &mut self.b1,
            ListType::B2 => &mut self.b2,
            _ => return,
        };

        list.push_back(frame_id);
        self.location.insert(frame_id, ghost_type);
    }

    /// 고스트 리스트 정리 (용량 제한)
    fn cleanup_ghosts(&mut self) {
        // B1 + B2는 capacity를 초과하지 않도록
        while self.b1.len() + self.b2.len() > self.capacity {
            // B1이 너무 크면 B1에서 제거
            if self.b1.len() > self.p {
                if let Some(victim) = self.b1.pop_front() {
                    self.location.remove(&victim);
                }
            } else if let Some(victim) = self.b2.pop_front() {
                // B2에서 제거
                self.location.remove(&victim);
            } else if let Some(victim) = self.b1.pop_front() {
                self.location.remove(&victim);
            }
        }
    }

    /// 적응: p 증가 (더 많은 recency)
    fn adapt_increase_p(&mut self) {
        let delta = if self.b1.len() >= self.b2.len() {
            1
        } else {
            self.b2.len() / self.b1.len().max(1)
        };
        self.p = (self.p + delta).min(self.capacity);
    }

    /// 적응: p 감소 (더 많은 frequency)
    fn adapt_decrease_p(&mut self) {
        let delta = if self.b2.len() >= self.b1.len() {
            1
        } else {
            self.b1.len() / self.b2.len().max(1)
        };
        self.p = self.p.saturating_sub(delta);
    }

    /// 프레임이 캐시에 있는지 확인
    pub fn contains(&self, frame_id: FrameId) -> bool {
        if let Some(&list_type) = self.location.get(&frame_id) {
            list_type == ListType::T1 || list_type == ListType::T2
        } else {
            false
        }
    }

    /// 프레임이 고스트에 있는지 확인
    pub fn is_ghost(&self, frame_id: FrameId) -> bool {
        if let Some(&list_type) = self.location.get(&frame_id) {
            list_type == ListType::B1 || list_type == ListType::B2
        } else {
            false
        }
    }

    /// 통계 리셋
    pub fn reset_stats(&mut self) {
        self.stats_t1_hits = 0;
        self.stats_t2_hits = 0;
        self.stats_b1_hits = 0;
        self.stats_b2_hits = 0;
        self.stats_misses = 0;
    }

    /// 캐시 완전 초기화
    pub fn clear(&mut self) {
        self.t1.clear();
        self.t2.clear();
        self.b1.clear();
        self.b2.clear();
        self.location.clear();
        self.pinned.clear();
        self.p = 0;
        self.reset_stats();
    }
}

/// ARC 통계
#[derive(Debug, Clone)]
pub struct ARCStats {
    /// T1 크기
    pub t1_size: usize,
    /// T2 크기
    pub t2_size: usize,
    /// B1 크기 (고스트)
    pub b1_size: usize,
    /// B2 크기 (고스트)
    pub b2_size: usize,
    /// 적응 파라미터
    pub p: usize,
    /// T1 히트 수
    pub t1_hits: u64,
    /// T2 히트 수
    pub t2_hits: u64,
    /// B1 히트 수 (적응 트리거)
    pub b1_hits: u64,
    /// B2 히트 수 (적응 트리거)
    pub b2_hits: u64,
    /// 미스 수
    pub misses: u64,
    /// Pin된 프레임 수
    pub pinned_count: usize,
}

impl ARCStats {
    /// 총 히트 수
    pub fn total_hits(&self) -> u64 {
        self.t1_hits + self.t2_hits
    }

    /// 총 접근 수
    pub fn total_accesses(&self) -> u64 {
        self.total_hits() + self.misses
    }

    /// 히트율 (0.0 ~ 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.total_accesses();
        if total == 0 {
            0.0
        } else {
            self.total_hits() as f64 / total as f64
        }
    }

    /// 적응 이벤트 수 (B1/B2 히트)
    pub fn adaptation_events(&self) -> u64 {
        self.b1_hits + self.b2_hits
    }
}

// ============================================================================
// 테스트
