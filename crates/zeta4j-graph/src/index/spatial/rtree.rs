// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! R*-Tree 구현
//!
//! Beckmann et al.의 R*-Tree 알고리즘을 기반으로 구현합니다.
//! - 삽입: R*-Tree ChooseSubtree (overlap 최소화) + Forced Reinsert
//! - 분할: R*-Tree Split (축별 정렬, margin/overlap 최적화)
//! - 삭제: CondenseTree
//! - 검색: 범위 검색, 점 검색, K-최근접 이웃

use super::node::{MAX_ENTRIES, MIN_ENTRIES, RTreeEntry, RTreeNode};
use super::types::{BoundingBox, Point2D};
use crate::core::NodeId;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

/// R*-Tree Forced Reinsert 비율 (30%)
const REINSERT_P: usize = (MAX_ENTRIES as f64 * 0.3) as usize;

/// R-Tree 자료구조
#[derive(Debug)]
pub struct RTree {
    /// 모든 노드 저장 (인덱스로 접근)
    nodes: Vec<RTreeNode>,
    /// 루트 노드 인덱스
    root: usize,
    /// 트리 높이
    height: usize,
    /// 엔트리 수
    size: usize,
    /// 재사용 가능한 빈 노드 슬롯 (Free List)
    free_slots: Vec<usize>,
}

impl Default for RTree {
    fn default() -> Self {
        Self::new()
    }
}

impl RTree {
    /// 새 R-Tree 생성
    pub fn new() -> Self {
        let root_node = RTreeNode::new_leaf();
        Self {
            nodes: vec![root_node],
            root: 0,
            height: 1,
            size: 0,
            free_slots: Vec::new(),
        }
    }

    /// 새 노드 할당 (Free List 활용)
    fn allocate_node(&mut self, node: RTreeNode) -> usize {
        if let Some(slot) = self.free_slots.pop() {
            // 재사용 가능한 슬롯 사용
            self.nodes[slot] = node;
            slot
        } else {
            // 새 슬롯 할당
            let idx = self.nodes.len();
            self.nodes.push(node);
            idx
        }
    }

    /// 노드 해제 (Free List에 추가)
    fn deallocate_node(&mut self, node_idx: usize) {
        // 노드 초기화 (메모리 해제)
        self.nodes[node_idx] = RTreeNode::new_leaf();
        self.nodes[node_idx].entries.clear();
        self.nodes[node_idx].entries.shrink_to_fit();
        self.free_slots.push(node_idx);
    }

    /// 트리가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// 엔트리 수 반환
    pub fn len(&self) -> usize {
        self.size
    }

    /// 트리 높이 반환
    pub fn height(&self) -> usize {
        self.height
    }

    /// 총 노드 슬롯 수 (할당된 + 해제된)
    pub fn total_node_slots(&self) -> usize {
        self.nodes.len()
    }

    /// 활성 노드 수 (실제 사용 중인 노드)
    pub fn active_node_count(&self) -> usize {
        self.nodes.len() - self.free_slots.len()
    }

    /// 재사용 가능한 빈 슬롯 수
    pub fn free_slot_count(&self) -> usize {
        self.free_slots.len()
    }

    /// 메모리 사용량 추정 (바이트)
    pub fn memory_usage(&self) -> usize {
        use std::mem::size_of;

        // RTree 구조체 자체
        let struct_size = size_of::<Self>();

        // nodes Vec
        let nodes_vec_overhead = size_of::<Vec<RTreeNode>>();
        let nodes_content: usize = self
            .nodes
            .iter()
            .map(|node| size_of::<RTreeNode>() + node.entries.capacity() * size_of::<RTreeEntry>())
            .sum();

        // free_slots Vec
        let free_slots_overhead = size_of::<Vec<usize>>();
        let free_slots_content = self.free_slots.capacity() * size_of::<usize>();

        struct_size + nodes_vec_overhead + nodes_content + free_slots_overhead + free_slots_content
    }

    /// 메모리 효율성 비율 (활성 노드 / 총 슬롯)
    pub fn memory_efficiency(&self) -> f64 {
        if self.nodes.is_empty() {
            1.0
        } else {
            self.active_node_count() as f64 / self.nodes.len() as f64
        }
    }

    /// 메모리 압축: 빈 슬롯을 제거하고 노드 인덱스 재배치
    ///
    /// 주의: 모든 노드 인덱스가 변경되므로 외부 참조가 없을 때만 사용
    ///
    /// ## 성능 최적화
    ///
    /// - `mem::take` + `into_iter`로 노드 이동 (clone 비용 제거)
    /// - free_slots HashSet 직접 사용으로 O(1) lookup 유지
    pub fn compact(&mut self) {
        if self.free_slots.is_empty() {
            return;
        }

        let active_count = self.active_node_count();
        let old_nodes = std::mem::take(&mut self.nodes);

        // 매핑 테이블 생성: old_idx -> new_idx
        let mut index_map: Vec<Option<usize>> = vec![None; old_nodes.len()];
        let mut new_nodes: Vec<RTreeNode> = Vec::with_capacity(active_count);

        // 노드 이동 (clone 없이)
        for (old_idx, node) in old_nodes.into_iter().enumerate() {
            if !self.free_slots.contains(&old_idx) {
                let new_idx = new_nodes.len();
                index_map[old_idx] = Some(new_idx);
                new_nodes.push(node); // 이동, clone 아님
            }
        }

        // 인덱스 업데이트
        for node in &mut new_nodes {
            // parent 인덱스 업데이트
            if let Some(parent_idx) = node.parent {
                node.parent = index_map[parent_idx];
            }

            // child_index 업데이트
            for entry in &mut node.entries {
                if let RTreeEntry::Internal { child_index, .. } = entry
                    && let Some(new_idx) = index_map[*child_index]
                {
                    *child_index = new_idx;
                }
            }
        }

        // 루트 인덱스 업데이트
        if let Some(new_root) = index_map[self.root] {
            self.root = new_root;
        }

        self.nodes = new_nodes;
        self.free_slots.clear();
    }

    /// 점 삽입
    pub fn insert_point(&mut self, point: Point2D, node_id: NodeId) {
        let bbox = BoundingBox::from_point(point);
        self.insert(bbox, node_id);
    }

    /// 경계 사각형 삽입
    pub fn insert(&mut self, bbox: BoundingBox, node_id: NodeId) {
        let entry = RTreeEntry::Leaf { bbox, node_id };

        // 1. ChooseLeaf: 삽입할 리프 노드 찾기
        let Some(leaf_idx) = self.choose_leaf(self.root, &bbox) else {
            tracing::error!("R-tree contains an internal node without a valid child");
            std::process::abort();
        };

        // 2. 리프에 엔트리 추가
        self.nodes[leaf_idx].add_entry(entry);
        self.size += 1;

        // 3. 오버플로우 처리 (Forced Reinsert 또는 분할)
        if self.nodes[leaf_idx].entries.len() > MAX_ENTRIES {
            let mut reinserted_levels = HashSet::new();
            self.handle_overflow_with_reinsert(leaf_idx, &mut reinserted_levels);
        } else {
            // MBR 업데이트
            self.adjust_tree(leaf_idx);
        }
    }

    /// 내부용 삽입 (재삽입 레벨 추적)
    fn insert_internal(
        &mut self,
        bbox: BoundingBox,
        node_id: NodeId,
        reinserted_levels: &mut HashSet<usize>,
    ) {
        let entry = RTreeEntry::Leaf { bbox, node_id };
        let Some(leaf_idx) = self.choose_leaf(self.root, &bbox) else {
            tracing::error!("R-tree contains an internal node without a valid child");
            std::process::abort();
        };
        self.nodes[leaf_idx].add_entry(entry);
        self.size += 1;

        if self.nodes[leaf_idx].entries.len() > MAX_ENTRIES {
            self.handle_overflow_with_reinsert(leaf_idx, reinserted_levels);
        } else {
            self.adjust_tree(leaf_idx);
        }
    }

    /// 노드 ID로 삭제
    pub fn delete(&mut self, node_id: NodeId) -> bool {
        // 1. 삭제할 엔트리가 있는 리프 찾기
        let found = self.find_leaf(self.root, node_id);

        if let Some((leaf_idx, entry_idx)) = found {
            // 2. 엔트리 삭제
            self.nodes[leaf_idx].remove_entry(entry_idx);
            self.size -= 1;

            // 3. CondenseTree: 언더플로우 처리
            self.condense_tree(leaf_idx);

            true
        } else {
            false
        }
    }

    /// 범위 검색: 주어진 bbox와 교차하는 모든 엔트리 반환
    pub fn search(&self, query: &BoundingBox) -> Vec<NodeId> {
        let mut results = Vec::new();
        self.search_recursive(self.root, query, &mut results);
        results
    }

    /// 점 포함 검색: 주어진 점을 포함하는 모든 엔트리 반환
    pub fn search_point(&self, point: &Point2D) -> Vec<NodeId> {
        let mut results = Vec::new();
        self.search_point_recursive(self.root, point, &mut results);
        results
    }

    /// K-최근접 이웃 검색
    pub fn nearest(&self, point: &Point2D, k: usize) -> Vec<(NodeId, f64)> {
        if k == 0 || self.is_empty() {
            return Vec::new();
        }

        // 우선순위 큐: (거리, 노드인덱스 또는 NodeId)
        let mut queue: BinaryHeap<NearestEntry> = BinaryHeap::new();
        let mut results: Vec<(NodeId, f64)> = Vec::with_capacity(k);

        // 루트부터 시작
        queue.push(NearestEntry::Node {
            distance: 0.0,
            node_idx: self.root,
        });

        while let Some(entry) = queue.pop() {
            match entry {
                NearestEntry::Node { node_idx, .. } => {
                    let node = &self.nodes[node_idx];

                    for e in &node.entries {
                        let dist = e.bbox().min_distance_to_point(point);

                        // 이미 k개 결과가 있고 현재 거리가 가장 먼 결과보다 크면 스킵
                        if results.len() >= k
                            && let Some((_, max_dist)) = results.last()
                            && dist > *max_dist
                        {
                            continue;
                        }

                        match e {
                            RTreeEntry::Leaf { node_id, bbox } => {
                                let exact_dist = bbox.center().distance(point);
                                queue.push(NearestEntry::Leaf {
                                    distance: exact_dist,
                                    node_id: *node_id,
                                });
                            }
                            RTreeEntry::Internal { child_index, .. } => {
                                queue.push(NearestEntry::Node {
                                    distance: dist,
                                    node_idx: *child_index,
                                });
                            }
                        }
                    }
                }
                NearestEntry::Leaf { distance, node_id } => {
                    // 결과에 추가
                    if results.len() < k {
                        results.push((node_id, distance));
                        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
                    } else if let Some((_, max_dist)) = results.last()
                        && distance < *max_dist
                    {
                        results.pop();
                        results.push((node_id, distance));
                        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
                    }
                }
            }
        }

        results
    }

    /// 거리 내 검색: 주어진 점에서 distance 이내의 모든 엔트리 반환
    pub fn search_within_distance(&self, point: &Point2D, distance: f64) -> Vec<(NodeId, f64)> {
        let mut results = Vec::new();
        self.search_within_distance_recursive(self.root, point, distance, &mut results);
        results
    }

    // ==================== Private Methods ====================

    /// R*-Tree ChooseSubtree: 삽입할 리프 노드 선택
    ///
    /// 리프 바로 위 레벨: overlap 증가량 최소화
    /// 그 외 레벨: 면적 증가량 최소화
    fn choose_leaf(&self, node_idx: usize, bbox: &BoundingBox) -> Option<usize> {
        let node = &self.nodes[node_idx];

        if node.is_leaf {
            return Some(node_idx);
        }

        // 자식이 리프인지 확인 (리프 바로 위 레벨인지)
        let first_child_idx = node.entries.first()?.child_index()?;
        let children_are_leaves = self.nodes[first_child_idx].is_leaf;

        let best_idx = if children_are_leaves {
            // R*-Tree: 리프 바로 위 레벨에서는 overlap 최소화
            self.choose_subtree_least_overlap(node_idx, bbox)
        } else {
            // 내부 레벨: 면적 증가 최소화 (기존 방식)
            self.choose_subtree_least_enlargement(node_idx, bbox)
        };

        let child_idx = node.entries.get(best_idx)?.child_index()?;
        self.choose_leaf(child_idx, bbox)
    }

    /// 면적 증가량이 가장 작은 엔트리 선택 (기존 Guttman 방식)
    fn choose_subtree_least_enlargement(&self, node_idx: usize, bbox: &BoundingBox) -> usize {
        let node = &self.nodes[node_idx];
        let mut best_idx = 0;
        let mut best_enlargement = f64::INFINITY;
        let mut best_area = f64::INFINITY;

        for (i, entry) in node.entries.iter().enumerate() {
            let enlargement = entry.bbox().enlargement(bbox);
            let area = entry.bbox().area();

            if enlargement < best_enlargement
                || (enlargement == best_enlargement && area < best_area)
            {
                best_enlargement = enlargement;
                best_area = area;
                best_idx = i;
            }
        }

        best_idx
    }

    /// R*-Tree: Overlap 증가량이 가장 작은 엔트리 선택
    /// 리프 바로 위 레벨에서만 사용
    fn choose_subtree_least_overlap(&self, node_idx: usize, bbox: &BoundingBox) -> usize {
        let node = &self.nodes[node_idx];
        let entries = &node.entries;

        // 엔트리가 적으면 모두 검사
        // 엔트리가 많으면 면적 증가량 상위 후보만 검사 (성능 최적화)
        let candidates: Vec<usize> = if entries.len() <= 4 {
            (0..entries.len()).collect()
        } else {
            // 면적 증가량이 작은 상위 4개 후보 선택
            let mut enlargements: Vec<(usize, f64)> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| (i, e.bbox().enlargement(bbox)))
                .collect();
            enlargements.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
            enlargements.iter().take(4).map(|(i, _)| *i).collect()
        };

        let mut best_idx = candidates[0];
        let mut best_overlap_delta = f64::INFINITY;
        let mut best_enlargement = f64::INFINITY;
        let mut best_area = f64::INFINITY;

        for &i in &candidates {
            let entry_bbox = entries[i].bbox();
            let merged_bbox = entry_bbox.merge(bbox);

            // 현재 overlap 합계
            let current_overlap: f64 = entries
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, e)| entry_bbox.overlap_area(e.bbox()))
                .sum();

            // 병합 후 overlap 합계
            let new_overlap: f64 = entries
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, e)| merged_bbox.overlap_area(e.bbox()))
                .sum();

            let overlap_delta = new_overlap - current_overlap;
            let enlargement = entry_bbox.enlargement(bbox);
            let area = entry_bbox.area();

            // overlap 증가량 → 면적 증가량 → 면적 순으로 비교
            if overlap_delta < best_overlap_delta
                || (overlap_delta == best_overlap_delta && enlargement < best_enlargement)
                || (overlap_delta == best_overlap_delta
                    && enlargement == best_enlargement
                    && area < best_area)
            {
                best_overlap_delta = overlap_delta;
                best_enlargement = enlargement;
                best_area = area;
                best_idx = i;
            }
        }

        best_idx
    }

    /// R*-Tree 오버플로우 처리: Forced Reinsert 또는 노드 분할
    ///
    /// 각 레벨에서 처음 오버플로우가 발생하면 Forced Reinsert 시도
    /// 이미 재삽입한 레벨에서 다시 오버플로우 발생하면 분할
    fn handle_overflow_with_reinsert(
        &mut self,
        node_idx: usize,
        reinserted_levels: &mut HashSet<usize>,
    ) {
        let level = self.get_node_level(node_idx);
        let _is_leaf = self.nodes[node_idx].is_leaf;

        // 루트가 아니고, 이 레벨에서 아직 재삽입을 안 했다면 Forced Reinsert
        if self.nodes[node_idx].parent.is_some() && !reinserted_levels.contains(&level) {
            reinserted_levels.insert(level);
            self.forced_reinsert(node_idx, reinserted_levels);
        } else {
            // 이미 재삽입했거나 루트면 분할
            self.handle_overflow_split(node_idx, reinserted_levels);
        }
    }

    /// Forced Reinsert: 중심에서 가장 먼 p개 엔트리를 재삽입
    fn forced_reinsert(&mut self, node_idx: usize, reinserted_levels: &mut HashSet<usize>) {
        let is_leaf = self.nodes[node_idx].is_leaf;
        let entries = std::mem::take(&mut self.nodes[node_idx].entries);

        // 노드 중심 계산
        let mut mbr = *entries[0].bbox();
        for e in entries.iter().skip(1) {
            mbr = mbr.merge(e.bbox());
        }
        let center = mbr.center();

        // 중심에서의 거리로 정렬 (먼 것부터)
        let mut sorted: Vec<(usize, f64)> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let entry_center = e.bbox().center();
                let dist = center.distance(&entry_center);
                (i, dist)
            })
            .collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

        // 재삽입할 엔트리 수 (최소 1개, 최대 REINSERT_P개)
        let reinsert_count = REINSERT_P.max(1).min(entries.len() - MIN_ENTRIES);

        // 가까운 엔트리는 유지, 먼 엔트리는 재삽입
        let (to_reinsert, to_keep): (Vec<_>, Vec<_>) = sorted
            .into_iter()
            .enumerate()
            .partition(|(i, _)| *i < reinsert_count);

        // 노드에 가까운 엔트리 유지
        for (_, (entry_idx, _)) in to_keep {
            self.nodes[node_idx]
                .entries
                .push(entries[entry_idx].clone());
        }

        // MBR 업데이트
        self.adjust_tree(node_idx);

        // 먼 엔트리 재삽입 (가까운 것부터 = 역순)
        let mut reinsert_entries: Vec<_> = to_reinsert
            .into_iter()
            .map(|(_, (entry_idx, _))| entries[entry_idx].clone())
            .collect();
        reinsert_entries.reverse();

        for entry in reinsert_entries {
            if is_leaf {
                if let RTreeEntry::Leaf { bbox, node_id } = entry {
                    self.size -= 1; // insert_internal에서 증가하므로
                    self.insert_internal(bbox, node_id, reinserted_levels);
                }
            } else {
                // 내부 노드 재삽입 (복잡하므로 간단히 분할로 처리)
                self.nodes[node_idx].entries.push(entry);
                if self.nodes[node_idx].entries.len() > MAX_ENTRIES {
                    self.handle_overflow_split(node_idx, reinserted_levels);
                }
            }
        }
    }

    /// 노드 레벨 계산 (리프 = 0)
    fn get_node_level(&self, node_idx: usize) -> usize {
        let mut level = 0;
        let mut current = node_idx;

        while !self.nodes[current].is_leaf {
            if let Some(child_idx) = self.nodes[current]
                .entries
                .first()
                .and_then(|e| e.child_index())
            {
                current = child_idx;
                level += 1;
            } else {
                break;
            }
        }

        level
    }

    /// 오버플로우 처리: 노드 분할 (Forced Reinsert 후 또는 직접 호출)
    fn handle_overflow_split(&mut self, node_idx: usize, reinserted_levels: &mut HashSet<usize>) {
        // R*-Tree Split 수행
        let (group1, group2) = self.rstar_split(node_idx);

        // 현재 노드를 group1으로, 새 노드를 group2로
        let node = &mut self.nodes[node_idx];
        let was_leaf = node.is_leaf;
        let parent = node.parent;

        node.entries = group1;

        // 새 노드 생성 (Free List 활용)
        let mut new_node = if was_leaf {
            RTreeNode::new_leaf()
        } else {
            RTreeNode::new_internal()
        };
        new_node.entries = group2;
        new_node.parent = parent;

        let new_node_idx = self.allocate_node(new_node);

        // 새 노드의 자식들의 parent 업데이트 (내부 노드인 경우)
        if !was_leaf {
            for entry in &self.nodes[new_node_idx].entries.clone() {
                if let RTreeEntry::Internal { child_index, .. } = entry {
                    self.nodes[*child_index].parent = Some(new_node_idx);
                }
            }
        }

        // 루트 분할인 경우 새 루트 생성, 아니면 부모에 새 노드 추가
        if let Some(parent_idx) = parent {
            let Some(new_mbr) = self.nodes[new_node_idx].compute_mbr() else {
                tracing::error!("R-tree split produced an empty node");
                std::process::abort();
            };

            self.nodes[parent_idx].add_entry(RTreeEntry::Internal {
                bbox: new_mbr,
                child_index: new_node_idx,
            });

            // 부모도 오버플로우 체크
            if self.nodes[parent_idx].entries.len() > MAX_ENTRIES {
                self.handle_overflow_with_reinsert(parent_idx, reinserted_levels);
            } else {
                self.adjust_tree(parent_idx);
            }
        } else {
            self.create_new_root(node_idx, new_node_idx);
        }
    }

    /// R*-Tree Split: 두 그룹으로 분할
    ///
    /// 1. 각 축(X, Y)별로 엔트리를 정렬
    /// 2. 모든 유효한 분할에 대해 margin 합계 계산
    /// 3. margin 합계가 최소인 축 선택
    /// 4. 해당 축에서 overlap이 최소인 분할 선택 (tie: area 최소)
    fn rstar_split(&self, node_idx: usize) -> (Vec<RTreeEntry>, Vec<RTreeEntry>) {
        let entries = self.nodes[node_idx].entries.clone();
        let n = entries.len();

        // 유효한 분할 범위: MIN_ENTRIES ~ n - MIN_ENTRIES
        let num_distributions = n - 2 * MIN_ENTRIES + 1;

        // 각 축별 정렬 (하한 기준, 상한 기준)
        let mut sorted_x_min: Vec<usize> = (0..n).collect();
        let mut sorted_x_max: Vec<usize> = (0..n).collect();
        let mut sorted_y_min: Vec<usize> = (0..n).collect();
        let mut sorted_y_max: Vec<usize> = (0..n).collect();

        sorted_x_min.sort_by(|&a, &b| {
            entries[a]
                .bbox()
                .min
                .x
                .partial_cmp(&entries[b].bbox().min.x)
                .unwrap_or(Ordering::Equal)
        });
        sorted_x_max.sort_by(|&a, &b| {
            entries[a]
                .bbox()
                .max
                .x
                .partial_cmp(&entries[b].bbox().max.x)
                .unwrap_or(Ordering::Equal)
        });
        sorted_y_min.sort_by(|&a, &b| {
            entries[a]
                .bbox()
                .min
                .y
                .partial_cmp(&entries[b].bbox().min.y)
                .unwrap_or(Ordering::Equal)
        });
        sorted_y_max.sort_by(|&a, &b| {
            entries[a]
                .bbox()
                .max
                .y
                .partial_cmp(&entries[b].bbox().max.y)
                .unwrap_or(Ordering::Equal)
        });

        // X축 margin 합계 계산
        let x_margin = self.compute_margin_sum(&entries, &sorted_x_min, num_distributions)
            + self.compute_margin_sum(&entries, &sorted_x_max, num_distributions);

        // Y축 margin 합계 계산
        let y_margin = self.compute_margin_sum(&entries, &sorted_y_min, num_distributions)
            + self.compute_margin_sum(&entries, &sorted_y_max, num_distributions);

        // 최적 축 선택
        let (best_sorted_min, best_sorted_max) = if x_margin <= y_margin {
            (sorted_x_min, sorted_x_max)
        } else {
            (sorted_y_min, sorted_y_max)
        };

        // 최적 분할 찾기 (overlap 최소, tie: area 최소)
        let mut best_split_idx = MIN_ENTRIES;
        let mut best_overlap = f64::INFINITY;
        let mut best_area = f64::INFINITY;
        let mut use_min_sort = true;

        // min 정렬 기준 분할 검사
        for k in MIN_ENTRIES..=(n - MIN_ENTRIES) {
            let (overlap, area) = self.compute_split_metrics(&entries, &best_sorted_min, k);
            if overlap < best_overlap || (overlap == best_overlap && area < best_area) {
                best_overlap = overlap;
                best_area = area;
                best_split_idx = k;
                use_min_sort = true;
            }
        }

        // max 정렬 기준 분할 검사
        for k in MIN_ENTRIES..=(n - MIN_ENTRIES) {
            let (overlap, area) = self.compute_split_metrics(&entries, &best_sorted_max, k);
            if overlap < best_overlap || (overlap == best_overlap && area < best_area) {
                best_overlap = overlap;
                best_area = area;
                best_split_idx = k;
                use_min_sort = false;
            }
        }

        // 최적 분할 적용
        let sorted = if use_min_sort {
            best_sorted_min
        } else {
            best_sorted_max
        };
        let group1: Vec<RTreeEntry> = sorted[..best_split_idx]
            .iter()
            .map(|&i| entries[i].clone())
            .collect();
        let group2: Vec<RTreeEntry> = sorted[best_split_idx..]
            .iter()
            .map(|&i| entries[i].clone())
            .collect();

        (group1, group2)
    }

    /// 주어진 정렬에 대한 모든 분할의 margin 합계 계산
    fn compute_margin_sum(
        &self,
        entries: &[RTreeEntry],
        sorted: &[usize],
        _num_distributions: usize,
    ) -> f64 {
        let n = entries.len();
        let mut margin_sum = 0.0;

        for k in MIN_ENTRIES..=(n - MIN_ENTRIES) {
            // 첫 번째 그룹 MBR
            let mut mbr1 = *entries[sorted[0]].bbox();
            for &idx in sorted.iter().take(k).skip(1) {
                mbr1 = mbr1.merge(entries[idx].bbox());
            }

            // 두 번째 그룹 MBR
            let mut mbr2 = *entries[sorted[k]].bbox();
            for &idx in sorted.iter().skip(k + 1) {
                mbr2 = mbr2.merge(entries[idx].bbox());
            }

            margin_sum += mbr1.margin() + mbr2.margin();
        }

        margin_sum
    }

    /// 주어진 분할의 overlap과 면적 계산
    fn compute_split_metrics(
        &self,
        entries: &[RTreeEntry],
        sorted: &[usize],
        split_idx: usize,
    ) -> (f64, f64) {
        // 첫 번째 그룹 MBR
        let mut mbr1 = *entries[sorted[0]].bbox();
        for &idx in sorted.iter().take(split_idx).skip(1) {
            mbr1 = mbr1.merge(entries[idx].bbox());
        }

        // 두 번째 그룹 MBR
        let mut mbr2 = *entries[sorted[split_idx]].bbox();
        for &idx in sorted.iter().skip(split_idx + 1) {
            mbr2 = mbr2.merge(entries[idx].bbox());
        }

        let overlap = mbr1.overlap_area(&mbr2);
        let area = mbr1.area() + mbr2.area();

        (overlap, area)
    }

    /// 새 루트 생성 (루트 분할 시)
    fn create_new_root(&mut self, child1_idx: usize, child2_idx: usize) {
        let (Some(mbr1), Some(mbr2)) = (
            self.nodes[child1_idx].compute_mbr(),
            self.nodes[child2_idx].compute_mbr(),
        ) else {
            tracing::error!("R-tree root split produced an empty child");
            std::process::abort();
        };

        let mut new_root = RTreeNode::new_internal();
        new_root.add_entry(RTreeEntry::Internal {
            bbox: mbr1,
            child_index: child1_idx,
        });
        new_root.add_entry(RTreeEntry::Internal {
            bbox: mbr2,
            child_index: child2_idx,
        });

        // Free List 활용
        let new_root_idx = self.allocate_node(new_root);

        self.nodes[child1_idx].parent = Some(new_root_idx);
        self.nodes[child2_idx].parent = Some(new_root_idx);

        self.root = new_root_idx;
        self.height += 1;
    }

    /// AdjustTree: 삽입 후 MBR 업데이트
    fn adjust_tree(&mut self, node_idx: usize) {
        let mut current_idx = node_idx;

        while let Some(parent_idx) = self.nodes[current_idx].parent {
            // 부모의 엔트리에서 현재 노드를 가리키는 엔트리 찾기
            let entry_idx = self.nodes[parent_idx].find_entry_by_child_index(current_idx);

            if let Some(idx) = entry_idx {
                // MBR 업데이트
                if let Some(new_mbr) = self.nodes[current_idx].compute_mbr()
                    && let RTreeEntry::Internal { bbox, .. } =
                        &mut self.nodes[parent_idx].entries[idx]
                {
                    *bbox = new_mbr;
                }
            }

            current_idx = parent_idx;
        }
    }

    /// FindLeaf: 특정 노드 ID를 가진 엔트리가 있는 리프 찾기
    fn find_leaf(&self, node_idx: usize, node_id: NodeId) -> Option<(usize, usize)> {
        let node = &self.nodes[node_idx];

        if node.is_leaf {
            // 리프에서 직접 검색
            if let Some(entry_idx) = node.find_entry_by_node_id(node_id) {
                return Some((node_idx, entry_idx));
            }
        } else {
            // 내부 노드: 모든 자식 검색
            for entry in &node.entries {
                if let RTreeEntry::Internal { child_index, .. } = entry
                    && let Some(result) = self.find_leaf(*child_index, node_id)
                {
                    return Some(result);
                }
            }
        }

        None
    }

    /// CondenseTree: 삭제 후 언더플로우 처리
    fn condense_tree(&mut self, leaf_idx: usize) {
        let mut orphaned_entries: Vec<RTreeEntry> = Vec::new();
        let mut nodes_to_free: Vec<usize> = Vec::new();
        let mut current_idx = leaf_idx;

        while let Some(parent_idx) = self.nodes[current_idx].parent {
            if self.nodes[current_idx].is_underflow() && current_idx != self.root {
                // 언더플로우: 모든 엔트리를 고아로 수집
                orphaned_entries.append(&mut self.nodes[current_idx].entries);

                // 부모에서 이 노드에 대한 엔트리 제거
                if let Some(entry_idx) =
                    self.nodes[parent_idx].find_entry_by_child_index(current_idx)
                {
                    self.nodes[parent_idx].remove_entry(entry_idx);
                }

                // Free List에 추가할 노드 기록
                nodes_to_free.push(current_idx);
            } else {
                // MBR 업데이트
                if let Some(entry_idx) =
                    self.nodes[parent_idx].find_entry_by_child_index(current_idx)
                    && let Some(new_mbr) = self.nodes[current_idx].compute_mbr()
                    && let RTreeEntry::Internal { bbox, .. } =
                        &mut self.nodes[parent_idx].entries[entry_idx]
                {
                    *bbox = new_mbr;
                }
            }

            current_idx = parent_idx;
        }

        // 루트 처리
        let old_root = self.root;
        if self.nodes[self.root].entries.is_empty() && self.height > 1 {
            // 루트가 비었으면 높이 감소
            self.nodes[self.root] = RTreeNode::new_leaf();
            self.height = 1;
        } else if !self.nodes[self.root].is_leaf && self.nodes[self.root].entries.len() == 1 {
            // 루트에 자식이 하나만 남으면 그 자식을 루트로
            if let Some(RTreeEntry::Internal { child_index, .. }) =
                self.nodes[self.root].entries.first()
            {
                let new_root = *child_index;
                self.nodes[new_root].parent = None;
                self.root = new_root;
                self.height -= 1;
                // 이전 루트 해제
                nodes_to_free.push(old_root);
            }
        }

        // 해제할 노드들 Free List에 추가
        for node_idx in nodes_to_free {
            self.deallocate_node(node_idx);
        }

        // 고아 엔트리 재삽입
        for entry in orphaned_entries {
            if let RTreeEntry::Leaf { bbox, node_id } = entry {
                self.size -= 1; // insert에서 다시 증가하므로 미리 감소
                self.insert(bbox, node_id);
            }
        }
    }

    /// 범위 검색 재귀
    fn search_recursive(&self, node_idx: usize, query: &BoundingBox, results: &mut Vec<NodeId>) {
        let node = &self.nodes[node_idx];

        for entry in &node.entries {
            if entry.bbox().intersects(query) {
                match entry {
                    RTreeEntry::Leaf { node_id, .. } => {
                        results.push(*node_id);
                    }
                    RTreeEntry::Internal { child_index, .. } => {
                        self.search_recursive(*child_index, query, results);
                    }
                }
            }
        }
    }

    /// 점 검색 재귀
    fn search_point_recursive(&self, node_idx: usize, point: &Point2D, results: &mut Vec<NodeId>) {
        let node = &self.nodes[node_idx];

        for entry in &node.entries {
            if entry.bbox().contains_point(point) {
                match entry {
                    RTreeEntry::Leaf { node_id, .. } => {
                        results.push(*node_id);
                    }
                    RTreeEntry::Internal { child_index, .. } => {
                        self.search_point_recursive(*child_index, point, results);
                    }
                }
            }
        }
    }

    /// 거리 내 검색 재귀
    fn search_within_distance_recursive(
        &self,
        node_idx: usize,
        point: &Point2D,
        distance: f64,
        results: &mut Vec<(NodeId, f64)>,
    ) {
        let node = &self.nodes[node_idx];
        let dist_sq = distance * distance;

        for entry in &node.entries {
            let min_dist_sq = entry.bbox().min_distance_squared_to_point(point);

            if min_dist_sq <= dist_sq {
                match entry {
                    RTreeEntry::Leaf { node_id, bbox } => {
                        let actual_dist = bbox.center().distance(point);
                        if actual_dist <= distance {
                            results.push((*node_id, actual_dist));
                        }
                    }
                    RTreeEntry::Internal { child_index, .. } => {
                        self.search_within_distance_recursive(
                            *child_index,
                            point,
                            distance,
                            results,
                        );
                    }
                }
            }
        }
    }
}

/// K-최근접 이웃 검색용 우선순위 큐 엔트리
#[derive(Debug)]
enum NearestEntry {
    Node { distance: f64, node_idx: usize },
    Leaf { distance: f64, node_id: NodeId },
}

impl PartialEq for NearestEntry {
    fn eq(&self, other: &Self) -> bool {
        self.distance() == other.distance()
    }
}

impl Eq for NearestEntry {}

impl PartialOrd for NearestEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NearestEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap을 위해 역순
        other
            .distance()
            .partial_cmp(&self.distance())
            .unwrap_or(Ordering::Equal)
    }
}

impl NearestEntry {
    fn distance(&self) -> f64 {
        match self {
            NearestEntry::Node { distance, .. } => *distance,
            NearestEntry::Leaf { distance, .. } => *distance,
        }
    }
}
