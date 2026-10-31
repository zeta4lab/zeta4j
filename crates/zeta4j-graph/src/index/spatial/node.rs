// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! R-Tree 노드 정의
//!
//! RTreeNode와 RTreeEntry를 정의합니다.
//!
//! ## MAX_ENTRIES 최적화 지침
//!
//! `MAX_ENTRIES` 값은 R-Tree 성능에 큰 영향을 미칩니다:
//!
//! - **작은 값 (4-8)**: 트리 깊이 증가, 노드 오버랩 감소, 포인트 쿼리 유리
//! - **큰 값 (16-50)**: 트리 깊이 감소, 메모리 효율적, 범위 쿼리 유리
//!
//! ### 현재 설정 (8) 선택 근거
//!
//! 1. **메모리 기반 인덱스**: 디스크 I/O 고려 불필요
//! 2. **캐시 효율성**: RTreeEntry(~40바이트) × 8 = ~320바이트 (L1 캐시 친화적)
//! 3. **균형 잡힌 성능**: 포인트/범위 쿼리 모두 적정 성능
//!
//! ### 조정 권장 사항
//!
//! - 포인트 쿼리 위주: 4-8 권장
//! - 범위 쿼리 위주: 16-32 권장
//! - 대규모 데이터셋: 벤치마크 후 결정

use super::types::BoundingBox;
use crate::core::NodeId;

/// R-Tree 최소 엔트리 수 (m)
///
/// 분할 시 각 노드가 최소한 가져야 하는 엔트리 수.
/// 일반적으로 MAX_ENTRIES / 2 이하로 설정.
pub const MIN_ENTRIES: usize = 2;

/// R-Tree 최대 엔트리 수 (M)
///
/// 노드당 최대 엔트리 수. 이 값을 초과하면 노드 분할 발생.
/// 메모리 기반 인덱스에서 8은 캐시 효율성과 쿼리 성능의 균형점.
///
/// ## 벤치마크 결과 요약 (1M 포인트 기준)
///
/// | MAX_ENTRIES | 삽입 시간 | 포인트 쿼리 | 범위 쿼리 | 메모리 |
/// |-------------|----------|------------|----------|--------|
/// | 4           | 느림      | 빠름        | 보통      | 많음   |
/// | 8 (현재)     | 보통      | 보통        | 보통      | 보통   |
/// | 16          | 빠름      | 보통        | 빠름      | 적음   |
/// | 32          | 빠름      | 느림        | 빠름      | 적음   |
pub const MAX_ENTRIES: usize = 8;

/// R-Tree 엔트리
#[derive(Debug, Clone)]
pub enum RTreeEntry {
    /// 리프 엔트리: MBR + 실제 노드 ID
    Leaf { bbox: BoundingBox, node_id: NodeId },
    /// 내부 엔트리: MBR + 자식 노드 인덱스
    Internal {
        bbox: BoundingBox,
        child_index: usize,
    },
}

impl RTreeEntry {
    /// 엔트리의 경계 사각형 반환
    pub fn bbox(&self) -> &BoundingBox {
        match self {
            RTreeEntry::Leaf { bbox, .. } => bbox,
            RTreeEntry::Internal { bbox, .. } => bbox,
        }
    }

    /// 엔트리의 경계 사각형 가변 참조
    pub fn bbox_mut(&mut self) -> &mut BoundingBox {
        match self {
            RTreeEntry::Leaf { bbox, .. } => bbox,
            RTreeEntry::Internal { bbox, .. } => bbox,
        }
    }

    /// 리프 엔트리인지 확인
    pub fn is_leaf(&self) -> bool {
        matches!(self, RTreeEntry::Leaf { .. })
    }

    /// 리프 엔트리의 노드 ID 반환
    pub fn node_id(&self) -> Option<NodeId> {
        match self {
            RTreeEntry::Leaf { node_id, .. } => Some(*node_id),
            RTreeEntry::Internal { .. } => None,
        }
    }

    /// 내부 엔트리의 자식 인덱스 반환
    pub fn child_index(&self) -> Option<usize> {
        match self {
            RTreeEntry::Internal { child_index, .. } => Some(*child_index),
            RTreeEntry::Leaf { .. } => None,
        }
    }
}

/// R-Tree 노드
#[derive(Debug, Clone)]
pub struct RTreeNode {
    /// 노드의 엔트리들
    pub entries: Vec<RTreeEntry>,
    /// 리프 노드 여부
    pub is_leaf: bool,
    /// 부모 노드 인덱스 (루트는 None)
    pub parent: Option<usize>,
}

impl RTreeNode {
    /// 새 리프 노드 생성
    pub fn new_leaf() -> Self {
        Self {
            entries: Vec::with_capacity(MAX_ENTRIES + 1),
            is_leaf: true,
            parent: None,
        }
    }

    /// 새 내부 노드 생성
    pub fn new_internal() -> Self {
        Self {
            entries: Vec::with_capacity(MAX_ENTRIES + 1),
            is_leaf: false,
            parent: None,
        }
    }

    /// 노드가 가득 찼는지 확인
    pub fn is_full(&self) -> bool {
        self.entries.len() >= MAX_ENTRIES
    }

    /// 노드가 언더플로우인지 확인 (삭제 시)
    pub fn is_underflow(&self) -> bool {
        self.entries.len() < MIN_ENTRIES
    }

    /// 노드의 모든 엔트리를 포함하는 MBR 계산
    pub fn compute_mbr(&self) -> Option<BoundingBox> {
        if self.entries.is_empty() {
            return None;
        }

        let mut mbr = *self.entries[0].bbox();
        for entry in self.entries.iter().skip(1) {
            mbr = mbr.merge(entry.bbox());
        }
        Some(mbr)
    }

    /// 엔트리 추가
    pub fn add_entry(&mut self, entry: RTreeEntry) {
        self.entries.push(entry);
    }

    /// 엔트리 제거 (인덱스로)
    pub fn remove_entry(&mut self, index: usize) -> RTreeEntry {
        self.entries.remove(index)
    }

    /// 특정 노드 ID를 가진 엔트리 찾기 (리프 노드용)
    pub fn find_entry_by_node_id(&self, node_id: NodeId) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| matches!(e, RTreeEntry::Leaf { node_id: id, .. } if *id == node_id))
    }

    /// 특정 자식 인덱스를 가진 엔트리 찾기 (내부 노드용)
    pub fn find_entry_by_child_index(&self, child_idx: usize) -> Option<usize> {
        self.entries.iter().position(
            |e| matches!(e, RTreeEntry::Internal { child_index, .. } if *child_index == child_idx),
        )
    }
}
