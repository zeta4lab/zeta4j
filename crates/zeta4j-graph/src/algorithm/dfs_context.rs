// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! DFS 탐색 컨텍스트
//!
//! `clippy::too_many_arguments` 경고를 해결하기 위한 DFS 상태 컨텍스트 구조체입니다.
//! Articulation Points, Bridges, Biconnected Components, SCC 등의 알고리즘에서
//! 공통으로 사용되는 DFS 상태를 캡슐화합니다.

use crate::core::NodeId;
use std::collections::{HashMap, HashSet};

/// DFS 탐색 컨텍스트 (무방향 그래프용)
///
/// Tarjan의 단절점/단절선 탐지 알고리즘에서 사용되는 상태를 관리합니다.
///
/// # Fields
///
/// * `visited` - 방문한 노드 집합
/// * `disc` - 발견 시간 (discovery time)
/// * `low` - 도달 가능한 최소 발견 시간 (low-link value)
/// * `parent` - 부모 노드 (None은 루트 노드)
/// * `time` - 현재 타임스탬프
#[derive(Debug, Clone)]
pub struct DfsContext {
    /// 방문한 노드 집합
    pub visited: HashSet<NodeId>,
    /// 발견 시간 맵
    pub disc: HashMap<NodeId, u64>,
    /// Low-link 값 맵
    pub low: HashMap<NodeId, u64>,
    /// 부모 노드 맵
    pub parent: HashMap<NodeId, Option<NodeId>>,
    /// 현재 타임스탬프
    pub time: u64,
}

impl Default for DfsContext {
    fn default() -> Self {
        Self::new()
    }
}

impl DfsContext {
    /// 새로운 DFS 컨텍스트 생성
    pub fn new() -> Self {
        Self {
            visited: HashSet::new(),
            disc: HashMap::new(),
            low: HashMap::new(),
            parent: HashMap::new(),
            time: 0,
        }
    }

    /// 예상 노드 수로 용량을 미리 할당하여 생성
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            visited: HashSet::with_capacity(capacity),
            disc: HashMap::with_capacity(capacity),
            low: HashMap::with_capacity(capacity),
            parent: HashMap::with_capacity(capacity),
            time: 0,
        }
    }

    /// 노드 방문 처리
    ///
    /// 노드를 방문 상태로 표시하고 발견 시간과 low-link 값을 설정합니다.
    pub fn visit(&mut self, node: NodeId, parent: Option<NodeId>) {
        self.visited.insert(node);
        self.time += 1;
        self.disc.insert(node, self.time);
        self.low.insert(node, self.time);
        self.parent.insert(node, parent);
    }

    /// 노드가 방문되었는지 확인
    #[inline]
    pub fn is_visited(&self, node: NodeId) -> bool {
        self.visited.contains(&node)
    }

    /// 노드의 발견 시간 조회
    #[inline]
    pub fn discovery_time(&self, node: NodeId) -> u64 {
        *self.disc.get(&node).unwrap_or(&u64::MAX)
    }

    /// 노드의 low-link 값 조회
    #[inline]
    pub fn low_link(&self, node: NodeId) -> u64 {
        *self.low.get(&node).unwrap_or(&u64::MAX)
    }

    /// 노드의 부모 조회
    #[inline]
    pub fn get_parent(&self, node: NodeId) -> Option<NodeId> {
        self.parent.get(&node).copied().flatten()
    }

    /// 노드의 low-link 값 업데이트
    pub fn update_low(&mut self, node: NodeId, value: u64) {
        if let Some(low) = self.low.get_mut(&node) {
            *low = (*low).min(value);
        }
    }

    /// 자식 노드의 low-link 값으로 부모의 low-link 값 업데이트
    pub fn propagate_low_to_parent(&mut self, child: NodeId) {
        if let Some(parent) = self.get_parent(child) {
            let child_low = self.low_link(child);
            self.update_low(parent, child_low);
        }
    }

    /// 컨텍스트 초기화
    pub fn clear(&mut self) {
        self.visited.clear();
        self.disc.clear();
        self.low.clear();
        self.parent.clear();
        self.time = 0;
    }
}

/// SCC 탐색 컨텍스트 (방향 그래프용)
///
/// Tarjan의 SCC 알고리즘에서 사용되는 상태를 관리합니다.
///
/// # Fields
///
/// * `indices` - 노드별 인덱스 (발견 순서)
/// * `low_link` - Low-link 값
/// * `on_stack` - 스택에 있는지 여부
/// * `stack` - DFS 스택
/// * `index_counter` - 인덱스 카운터
#[derive(Debug, Clone)]
pub struct SccContext {
    /// 노드별 인덱스 맵
    pub indices: HashMap<NodeId, usize>,
    /// Low-link 값 맵
    pub low_link: HashMap<NodeId, usize>,
    /// 스택 존재 여부 맵
    pub on_stack: HashMap<NodeId, bool>,
    /// DFS 스택
    pub stack: Vec<NodeId>,
    /// 인덱스 카운터
    pub index_counter: usize,
}

impl Default for SccContext {
    fn default() -> Self {
        Self::new()
    }
}

impl SccContext {
    /// 새로운 SCC 컨텍스트 생성
    pub fn new() -> Self {
        Self {
            indices: HashMap::new(),
            low_link: HashMap::new(),
            on_stack: HashMap::new(),
            stack: Vec::new(),
            index_counter: 0,
        }
    }

    /// 예상 노드 수로 용량을 미리 할당하여 생성
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            indices: HashMap::with_capacity(capacity),
            low_link: HashMap::with_capacity(capacity),
            on_stack: HashMap::with_capacity(capacity),
            stack: Vec::with_capacity(capacity),
            index_counter: 0,
        }
    }

    /// 노드 방문 처리
    pub fn visit(&mut self, node: NodeId) {
        self.indices.insert(node, self.index_counter);
        self.low_link.insert(node, self.index_counter);
        self.index_counter += 1;
        self.stack.push(node);
        self.on_stack.insert(node, true);
    }

    /// 노드가 방문되었는지 확인
    #[inline]
    pub fn is_visited(&self, node: NodeId) -> bool {
        self.indices.contains_key(&node)
    }

    /// 노드가 스택에 있는지 확인
    #[inline]
    pub fn is_on_stack(&self, node: NodeId) -> bool {
        *self.on_stack.get(&node).unwrap_or(&false)
    }

    /// 노드의 인덱스 조회
    #[inline]
    pub fn index(&self, node: NodeId) -> usize {
        *self.indices.get(&node).unwrap_or(&usize::MAX)
    }

    /// 노드의 low-link 값 조회
    #[inline]
    pub fn low_link(&self, node: NodeId) -> usize {
        *self.low_link.get(&node).unwrap_or(&usize::MAX)
    }

    /// 노드의 low-link 값 업데이트
    pub fn update_low(&mut self, node: NodeId, value: usize) {
        if let Some(low) = self.low_link.get_mut(&node) {
            *low = (*low).min(value);
        }
    }

    /// 노드가 SCC의 루트인지 확인
    #[inline]
    pub fn is_root(&self, node: NodeId) -> bool {
        self.low_link(node) == self.index(node)
    }

    /// 스택에서 노드를 팝하고 스택 상태 업데이트
    pub fn pop_from_stack(&mut self) -> Option<NodeId> {
        if let Some(node) = self.stack.pop() {
            self.on_stack.insert(node, false);
            Some(node)
        } else {
            None
        }
    }

    /// SCC 추출 (루트 노드까지의 모든 노드)
    pub fn extract_scc(&mut self, root: NodeId) -> Vec<NodeId> {
        let mut component = Vec::new();
        while let Some(w) = self.pop_from_stack() {
            component.push(w);
            if w == root {
                break;
            }
        }
        component
    }

    /// 컨텍스트 초기화
    pub fn clear(&mut self) {
        self.indices.clear();
        self.low_link.clear();
        self.on_stack.clear();
        self.stack.clear();
        self.index_counter = 0;
    }
}
