// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 3D R*-Tree 구현
//!
//! 3D 공간 데이터를 위한 R*-Tree 알고리즘 구현입니다.
//! 2D R*-Tree와 동일한 알고리즘을 3차원으로 확장합니다.

use super::types::{BoundingBox3D, Point3D};
use crate::core::NodeId;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

/// 3D R-Tree 최소 엔트리 수 (m)
pub const MIN_ENTRIES_3D: usize = 2;

/// 3D R-Tree 최대 엔트리 수 (M)
pub const MAX_ENTRIES_3D: usize = 8;

/// R*-Tree Forced Reinsert 비율 (30%)
const REINSERT_P_3D: usize = (MAX_ENTRIES_3D as f64 * 0.3) as usize;

/// 3D R-Tree 엔트리
#[derive(Debug, Clone)]
pub enum RTree3DEntry {
    /// 리프 엔트리: MBB + 실제 노드 ID
    Leaf {
        bbox: BoundingBox3D,
        node_id: NodeId,
    },
    /// 내부 엔트리: MBB + 자식 노드 인덱스
    Internal {
        bbox: BoundingBox3D,
        child_index: usize,
    },
}

impl RTree3DEntry {
    /// 엔트리의 경계 직육면체 반환
    pub fn bbox(&self) -> &BoundingBox3D {
        match self {
            RTree3DEntry::Leaf { bbox, .. } => bbox,
            RTree3DEntry::Internal { bbox, .. } => bbox,
        }
    }

    /// 리프 엔트리의 노드 ID 반환
    pub fn node_id(&self) -> Option<NodeId> {
        match self {
            RTree3DEntry::Leaf { node_id, .. } => Some(*node_id),
            RTree3DEntry::Internal { .. } => None,
        }
    }

    /// 내부 엔트리의 자식 인덱스 반환
    pub fn child_index(&self) -> Option<usize> {
        match self {
            RTree3DEntry::Internal { child_index, .. } => Some(*child_index),
            RTree3DEntry::Leaf { .. } => None,
        }
    }
}

/// 3D R-Tree 노드
#[derive(Debug, Clone)]
pub struct RTree3DNode {
    pub entries: Vec<RTree3DEntry>,
    pub is_leaf: bool,
    pub parent: Option<usize>,
}

impl RTree3DNode {
    pub fn new_leaf() -> Self {
        Self {
            entries: Vec::with_capacity(MAX_ENTRIES_3D + 1),
            is_leaf: true,
            parent: None,
        }
    }

    pub fn new_internal() -> Self {
        Self {
            entries: Vec::with_capacity(MAX_ENTRIES_3D + 1),
            is_leaf: false,
            parent: None,
        }
    }

    pub fn is_underflow(&self) -> bool {
        self.entries.len() < MIN_ENTRIES_3D
    }

    pub fn compute_mbb(&self) -> Option<BoundingBox3D> {
        if self.entries.is_empty() {
            return None;
        }
        let mut mbb = *self.entries[0].bbox();
        for entry in self.entries.iter().skip(1) {
            mbb = mbb.merge(entry.bbox());
        }
        Some(mbb)
    }

    pub fn find_entry_by_node_id(&self, node_id: NodeId) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| matches!(e, RTree3DEntry::Leaf { node_id: id, .. } if *id == node_id))
    }

    pub fn find_entry_by_child_index(&self, child_idx: usize) -> Option<usize> {
        self.entries.iter().position(|e| {
            matches!(e, RTree3DEntry::Internal { child_index, .. } if *child_index == child_idx)
        })
    }
}

/// 3D R*-Tree 자료구조
#[derive(Debug)]
pub struct RTree3D {
    nodes: Vec<RTree3DNode>,
    root: usize,
    height: usize,
    size: usize,
    free_slots: Vec<usize>,
}

impl Default for RTree3D {
    fn default() -> Self {
        Self::new()
    }
}

impl RTree3D {
    /// 새 3D R-Tree 생성
    pub fn new() -> Self {
        let root_node = RTree3DNode::new_leaf();
        Self {
            nodes: vec![root_node],
            root: 0,
            height: 1,
            size: 0,
            free_slots: Vec::new(),
        }
    }

    fn allocate_node(&mut self, node: RTree3DNode) -> usize {
        if let Some(slot) = self.free_slots.pop() {
            self.nodes[slot] = node;
            slot
        } else {
            let idx = self.nodes.len();
            self.nodes.push(node);
            idx
        }
    }

    fn deallocate_node(&mut self, node_idx: usize) {
        self.nodes[node_idx] = RTree3DNode::new_leaf();
        self.nodes[node_idx].entries.clear();
        self.free_slots.push(node_idx);
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// 3D 점 삽입
    pub fn insert_point(&mut self, point: Point3D, node_id: NodeId) {
        let bbox = BoundingBox3D::from_point(point);
        self.insert(bbox, node_id);
    }

    /// 경계 직육면체 삽입
    pub fn insert(&mut self, bbox: BoundingBox3D, node_id: NodeId) {
        let entry = RTree3DEntry::Leaf { bbox, node_id };
        let Some(leaf_idx) = self.choose_leaf(self.root, &bbox) else {
            tracing::error!("3D R-tree contains an internal node without a valid child");
            std::process::abort();
        };

        self.nodes[leaf_idx].entries.push(entry);
        self.size += 1;

        if self.nodes[leaf_idx].entries.len() > MAX_ENTRIES_3D {
            let mut reinserted_levels = HashSet::new();
            self.handle_overflow_with_reinsert(leaf_idx, &mut reinserted_levels);
        } else {
            self.adjust_tree(leaf_idx);
        }
    }

    fn insert_internal(
        &mut self,
        bbox: BoundingBox3D,
        node_id: NodeId,
        reinserted_levels: &mut HashSet<usize>,
    ) {
        let entry = RTree3DEntry::Leaf { bbox, node_id };
        let Some(leaf_idx) = self.choose_leaf(self.root, &bbox) else {
            tracing::error!("3D R-tree contains an internal node without a valid child");
            std::process::abort();
        };
        self.nodes[leaf_idx].entries.push(entry);
        self.size += 1;

        if self.nodes[leaf_idx].entries.len() > MAX_ENTRIES_3D {
            self.handle_overflow_with_reinsert(leaf_idx, reinserted_levels);
        } else {
            self.adjust_tree(leaf_idx);
        }
    }

    /// 노드 ID로 삭제
    pub fn delete(&mut self, node_id: NodeId) -> bool {
        let found = self.find_leaf(self.root, node_id);

        if let Some((leaf_idx, entry_idx)) = found {
            self.nodes[leaf_idx].entries.remove(entry_idx);
            self.size -= 1;
            self.condense_tree(leaf_idx);
            true
        } else {
            false
        }
    }

    /// 범위 검색: 주어진 bbox와 교차하는 모든 엔트리 반환
    pub fn search(&self, query: &BoundingBox3D) -> Vec<NodeId> {
        let mut results = Vec::new();
        self.search_recursive(self.root, query, &mut results);
        results
    }

    /// 점 포함 검색
    pub fn search_point(&self, point: &Point3D) -> Vec<NodeId> {
        let mut results = Vec::new();
        self.search_point_recursive(self.root, point, &mut results);
        results
    }

    /// K-최근접 이웃 검색
    pub fn nearest(&self, point: &Point3D, k: usize) -> Vec<(NodeId, f64)> {
        if k == 0 || self.is_empty() {
            return Vec::new();
        }

        let mut queue: BinaryHeap<Nearest3DEntry> = BinaryHeap::new();
        let mut results: Vec<(NodeId, f64)> = Vec::with_capacity(k);

        queue.push(Nearest3DEntry::Node {
            distance: 0.0,
            node_idx: self.root,
        });

        while let Some(entry) = queue.pop() {
            match entry {
                Nearest3DEntry::Node { node_idx, .. } => {
                    let node = &self.nodes[node_idx];

                    for e in &node.entries {
                        let dist = e.bbox().min_distance_to_point(point);

                        if results.len() >= k
                            && let Some((_, max_dist)) = results.last()
                            && dist > *max_dist
                        {
                            continue;
                        }

                        match e {
                            RTree3DEntry::Leaf { node_id, bbox } => {
                                let exact_dist = bbox.center().distance(point);
                                queue.push(Nearest3DEntry::Leaf {
                                    distance: exact_dist,
                                    node_id: *node_id,
                                });
                            }
                            RTree3DEntry::Internal { child_index, .. } => {
                                queue.push(Nearest3DEntry::Node {
                                    distance: dist,
                                    node_idx: *child_index,
                                });
                            }
                        }
                    }
                }
                Nearest3DEntry::Leaf { distance, node_id } => {
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

    /// 거리 내 검색
    pub fn search_within_distance(&self, point: &Point3D, distance: f64) -> Vec<(NodeId, f64)> {
        let mut results = Vec::new();
        self.search_within_distance_recursive(self.root, point, distance, &mut results);
        results
    }

    // ==================== Private Methods ====================

    fn choose_leaf(&self, node_idx: usize, bbox: &BoundingBox3D) -> Option<usize> {
        let node = &self.nodes[node_idx];

        if node.is_leaf {
            return Some(node_idx);
        }

        let first_child_idx = node.entries.first()?.child_index()?;
        let children_are_leaves = self.nodes[first_child_idx].is_leaf;

        let best_idx = if children_are_leaves {
            self.choose_subtree_least_overlap(node_idx, bbox)
        } else {
            self.choose_subtree_least_enlargement(node_idx, bbox)
        };

        let child_idx = node.entries.get(best_idx)?.child_index()?;
        self.choose_leaf(child_idx, bbox)
    }

    fn choose_subtree_least_enlargement(&self, node_idx: usize, bbox: &BoundingBox3D) -> usize {
        let node = &self.nodes[node_idx];
        let mut best_idx = 0;
        let mut best_enlargement = f64::INFINITY;
        let mut best_volume = f64::INFINITY;

        for (i, entry) in node.entries.iter().enumerate() {
            let enlargement = entry.bbox().enlargement(bbox);
            let volume = entry.bbox().volume();

            if enlargement < best_enlargement
                || (enlargement == best_enlargement && volume < best_volume)
            {
                best_enlargement = enlargement;
                best_volume = volume;
                best_idx = i;
            }
        }

        best_idx
    }

    fn choose_subtree_least_overlap(&self, node_idx: usize, bbox: &BoundingBox3D) -> usize {
        let node = &self.nodes[node_idx];
        let entries = &node.entries;

        let candidates: Vec<usize> = if entries.len() <= 4 {
            (0..entries.len()).collect()
        } else {
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
        let mut best_volume = f64::INFINITY;

        for &i in &candidates {
            let entry_bbox = entries[i].bbox();
            let merged_bbox = entry_bbox.merge(bbox);

            let current_overlap: f64 = entries
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, e)| entry_bbox.overlap_volume(e.bbox()))
                .sum();

            let new_overlap: f64 = entries
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, e)| merged_bbox.overlap_volume(e.bbox()))
                .sum();

            let overlap_delta = new_overlap - current_overlap;
            let enlargement = entry_bbox.enlargement(bbox);
            let volume = entry_bbox.volume();

            if overlap_delta < best_overlap_delta
                || (overlap_delta == best_overlap_delta && enlargement < best_enlargement)
                || (overlap_delta == best_overlap_delta
                    && enlargement == best_enlargement
                    && volume < best_volume)
            {
                best_overlap_delta = overlap_delta;
                best_enlargement = enlargement;
                best_volume = volume;
                best_idx = i;
            }
        }

        best_idx
    }

    fn handle_overflow_with_reinsert(
        &mut self,
        node_idx: usize,
        reinserted_levels: &mut HashSet<usize>,
    ) {
        let level = self.get_node_level(node_idx);

        if self.nodes[node_idx].parent.is_some() && !reinserted_levels.contains(&level) {
            reinserted_levels.insert(level);
            self.forced_reinsert(node_idx, reinserted_levels);
        } else {
            self.handle_overflow_split(node_idx, reinserted_levels);
        }
    }

    fn forced_reinsert(&mut self, node_idx: usize, reinserted_levels: &mut HashSet<usize>) {
        let is_leaf = self.nodes[node_idx].is_leaf;
        let entries = std::mem::take(&mut self.nodes[node_idx].entries);

        let mut mbb = *entries[0].bbox();
        for e in entries.iter().skip(1) {
            mbb = mbb.merge(e.bbox());
        }
        let center = mbb.center();

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

        let reinsert_count = REINSERT_P_3D.max(1).min(entries.len() - MIN_ENTRIES_3D);

        let (to_reinsert, to_keep): (Vec<_>, Vec<_>) = sorted
            .into_iter()
            .enumerate()
            .partition(|(i, _)| *i < reinsert_count);

        for (_, (entry_idx, _)) in to_keep {
            self.nodes[node_idx]
                .entries
                .push(entries[entry_idx].clone());
        }

        self.adjust_tree(node_idx);

        let mut reinsert_entries: Vec<_> = to_reinsert
            .into_iter()
            .map(|(_, (entry_idx, _))| entries[entry_idx].clone())
            .collect();
        reinsert_entries.reverse();

        for entry in reinsert_entries {
            if is_leaf {
                if let RTree3DEntry::Leaf { bbox, node_id } = entry {
                    self.size -= 1;
                    self.insert_internal(bbox, node_id, reinserted_levels);
                }
            } else {
                self.nodes[node_idx].entries.push(entry);
                if self.nodes[node_idx].entries.len() > MAX_ENTRIES_3D {
                    self.handle_overflow_split(node_idx, reinserted_levels);
                }
            }
        }
    }

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

    fn handle_overflow_split(&mut self, node_idx: usize, reinserted_levels: &mut HashSet<usize>) {
        let (group1, group2) = self.rstar_split(node_idx);

        let node = &mut self.nodes[node_idx];
        let was_leaf = node.is_leaf;
        let parent = node.parent;

        node.entries = group1;

        let mut new_node = if was_leaf {
            RTree3DNode::new_leaf()
        } else {
            RTree3DNode::new_internal()
        };
        new_node.entries = group2;
        new_node.parent = parent;

        let new_node_idx = self.allocate_node(new_node);

        if !was_leaf {
            for entry in &self.nodes[new_node_idx].entries.clone() {
                if let RTree3DEntry::Internal { child_index, .. } = entry {
                    self.nodes[*child_index].parent = Some(new_node_idx);
                }
            }
        }

        if let Some(parent_idx) = parent {
            let Some(new_mbb) = self.nodes[new_node_idx].compute_mbb() else {
                tracing::error!("3D R-tree split produced an empty node");
                std::process::abort();
            };

            self.nodes[parent_idx].entries.push(RTree3DEntry::Internal {
                bbox: new_mbb,
                child_index: new_node_idx,
            });

            if self.nodes[parent_idx].entries.len() > MAX_ENTRIES_3D {
                self.handle_overflow_with_reinsert(parent_idx, reinserted_levels);
            } else {
                self.adjust_tree(parent_idx);
            }
        } else {
            self.create_new_root(node_idx, new_node_idx);
        }
    }

    /// R*-Tree 3D Split
    fn rstar_split(&self, node_idx: usize) -> (Vec<RTree3DEntry>, Vec<RTree3DEntry>) {
        let entries = self.nodes[node_idx].entries.clone();
        let n = entries.len();

        // 각 축별 정렬
        let mut sorted_x_min: Vec<usize> = (0..n).collect();
        let mut sorted_y_min: Vec<usize> = (0..n).collect();
        let mut sorted_z_min: Vec<usize> = (0..n).collect();

        sorted_x_min.sort_by(|&a, &b| {
            entries[a]
                .bbox()
                .min
                .x
                .partial_cmp(&entries[b].bbox().min.x)
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
        sorted_z_min.sort_by(|&a, &b| {
            entries[a]
                .bbox()
                .min
                .z
                .partial_cmp(&entries[b].bbox().min.z)
                .unwrap_or(Ordering::Equal)
        });

        // 각 축 margin 합계 계산
        let x_margin = self.compute_margin_sum_3d(&entries, &sorted_x_min);
        let y_margin = self.compute_margin_sum_3d(&entries, &sorted_y_min);
        let z_margin = self.compute_margin_sum_3d(&entries, &sorted_z_min);

        // 최적 축 선택
        let best_sorted = if x_margin <= y_margin && x_margin <= z_margin {
            sorted_x_min
        } else if y_margin <= z_margin {
            sorted_y_min
        } else {
            sorted_z_min
        };

        // 최적 분할 찾기
        let mut best_split_idx = MIN_ENTRIES_3D;
        let mut best_overlap = f64::INFINITY;
        let mut best_volume = f64::INFINITY;

        for k in MIN_ENTRIES_3D..=(n - MIN_ENTRIES_3D) {
            let (overlap, volume) = self.compute_split_metrics_3d(&entries, &best_sorted, k);
            if overlap < best_overlap || (overlap == best_overlap && volume < best_volume) {
                best_overlap = overlap;
                best_volume = volume;
                best_split_idx = k;
            }
        }

        let group1: Vec<RTree3DEntry> = best_sorted[..best_split_idx]
            .iter()
            .map(|&i| entries[i].clone())
            .collect();
        let group2: Vec<RTree3DEntry> = best_sorted[best_split_idx..]
            .iter()
            .map(|&i| entries[i].clone())
            .collect();

        (group1, group2)
    }

    fn compute_margin_sum_3d(&self, entries: &[RTree3DEntry], sorted: &[usize]) -> f64 {
        let n = entries.len();
        let mut margin_sum = 0.0;

        for k in MIN_ENTRIES_3D..=(n - MIN_ENTRIES_3D) {
            let mut mbb1 = *entries[sorted[0]].bbox();
            for &idx in sorted.iter().take(k).skip(1) {
                mbb1 = mbb1.merge(entries[idx].bbox());
            }

            let mut mbb2 = *entries[sorted[k]].bbox();
            for &idx in sorted.iter().skip(k + 1) {
                mbb2 = mbb2.merge(entries[idx].bbox());
            }

            margin_sum += mbb1.margin() + mbb2.margin();
        }

        margin_sum
    }

    fn compute_split_metrics_3d(
        &self,
        entries: &[RTree3DEntry],
        sorted: &[usize],
        split_idx: usize,
    ) -> (f64, f64) {
        let mut mbb1 = *entries[sorted[0]].bbox();
        for &idx in sorted.iter().take(split_idx).skip(1) {
            mbb1 = mbb1.merge(entries[idx].bbox());
        }

        let mut mbb2 = *entries[sorted[split_idx]].bbox();
        for &idx in sorted.iter().skip(split_idx + 1) {
            mbb2 = mbb2.merge(entries[idx].bbox());
        }

        let overlap = mbb1.overlap_volume(&mbb2);
        let volume = mbb1.volume() + mbb2.volume();

        (overlap, volume)
    }

    fn create_new_root(&mut self, child1_idx: usize, child2_idx: usize) {
        let (Some(mbb1), Some(mbb2)) = (
            self.nodes[child1_idx].compute_mbb(),
            self.nodes[child2_idx].compute_mbb(),
        ) else {
            tracing::error!("3D R-tree root split produced an empty child");
            std::process::abort();
        };

        let mut new_root = RTree3DNode::new_internal();
        new_root.entries.push(RTree3DEntry::Internal {
            bbox: mbb1,
            child_index: child1_idx,
        });
        new_root.entries.push(RTree3DEntry::Internal {
            bbox: mbb2,
            child_index: child2_idx,
        });

        let new_root_idx = self.allocate_node(new_root);

        self.nodes[child1_idx].parent = Some(new_root_idx);
        self.nodes[child2_idx].parent = Some(new_root_idx);

        self.root = new_root_idx;
        self.height += 1;
    }

    fn adjust_tree(&mut self, node_idx: usize) {
        let mut current_idx = node_idx;

        while let Some(parent_idx) = self.nodes[current_idx].parent {
            let entry_idx = self.nodes[parent_idx].find_entry_by_child_index(current_idx);

            if let Some(idx) = entry_idx
                && let Some(new_mbb) = self.nodes[current_idx].compute_mbb()
                && let RTree3DEntry::Internal { bbox, .. } =
                    &mut self.nodes[parent_idx].entries[idx]
            {
                *bbox = new_mbb;
            }

            current_idx = parent_idx;
        }
    }

    fn find_leaf(&self, node_idx: usize, node_id: NodeId) -> Option<(usize, usize)> {
        let node = &self.nodes[node_idx];

        if node.is_leaf {
            if let Some(entry_idx) = node.find_entry_by_node_id(node_id) {
                return Some((node_idx, entry_idx));
            }
        } else {
            for entry in &node.entries {
                if let RTree3DEntry::Internal { child_index, .. } = entry
                    && let Some(result) = self.find_leaf(*child_index, node_id)
                {
                    return Some(result);
                }
            }
        }

        None
    }

    fn condense_tree(&mut self, leaf_idx: usize) {
        let mut orphaned_entries: Vec<RTree3DEntry> = Vec::new();
        let mut nodes_to_free: Vec<usize> = Vec::new();
        let mut current_idx = leaf_idx;

        while let Some(parent_idx) = self.nodes[current_idx].parent {
            if self.nodes[current_idx].is_underflow() && current_idx != self.root {
                orphaned_entries.append(&mut self.nodes[current_idx].entries);

                if let Some(entry_idx) =
                    self.nodes[parent_idx].find_entry_by_child_index(current_idx)
                {
                    self.nodes[parent_idx].entries.remove(entry_idx);
                }

                nodes_to_free.push(current_idx);
            } else if let Some(entry_idx) =
                self.nodes[parent_idx].find_entry_by_child_index(current_idx)
                && let Some(new_mbb) = self.nodes[current_idx].compute_mbb()
                && let RTree3DEntry::Internal { bbox, .. } =
                    &mut self.nodes[parent_idx].entries[entry_idx]
            {
                *bbox = new_mbb;
            }

            current_idx = parent_idx;
        }

        let old_root = self.root;
        if self.nodes[self.root].entries.is_empty() && self.height > 1 {
            self.nodes[self.root] = RTree3DNode::new_leaf();
            self.height = 1;
        } else if !self.nodes[self.root].is_leaf
            && self.nodes[self.root].entries.len() == 1
            && let Some(RTree3DEntry::Internal { child_index, .. }) =
                self.nodes[self.root].entries.first()
        {
            let new_root = *child_index;
            self.nodes[new_root].parent = None;
            self.root = new_root;
            self.height -= 1;
            nodes_to_free.push(old_root);
        }

        for node_idx in nodes_to_free {
            self.deallocate_node(node_idx);
        }

        for entry in orphaned_entries {
            if let RTree3DEntry::Leaf { bbox, node_id } = entry {
                self.size -= 1;
                self.insert(bbox, node_id);
            }
        }
    }

    fn search_recursive(&self, node_idx: usize, query: &BoundingBox3D, results: &mut Vec<NodeId>) {
        let node = &self.nodes[node_idx];

        for entry in &node.entries {
            if entry.bbox().intersects(query) {
                match entry {
                    RTree3DEntry::Leaf { node_id, .. } => {
                        results.push(*node_id);
                    }
                    RTree3DEntry::Internal { child_index, .. } => {
                        self.search_recursive(*child_index, query, results);
                    }
                }
            }
        }
    }

    fn search_point_recursive(&self, node_idx: usize, point: &Point3D, results: &mut Vec<NodeId>) {
        let node = &self.nodes[node_idx];

        for entry in &node.entries {
            if entry.bbox().contains_point(point) {
                match entry {
                    RTree3DEntry::Leaf { node_id, .. } => {
                        results.push(*node_id);
                    }
                    RTree3DEntry::Internal { child_index, .. } => {
                        self.search_point_recursive(*child_index, point, results);
                    }
                }
            }
        }
    }

    fn search_within_distance_recursive(
        &self,
        node_idx: usize,
        point: &Point3D,
        distance: f64,
        results: &mut Vec<(NodeId, f64)>,
    ) {
        let node = &self.nodes[node_idx];
        let dist_sq = distance * distance;

        for entry in &node.entries {
            let min_dist_sq = entry.bbox().min_distance_squared_to_point(point);

            if min_dist_sq <= dist_sq {
                match entry {
                    RTree3DEntry::Leaf { node_id, bbox } => {
                        let actual_dist = bbox.center().distance(point);
                        if actual_dist <= distance {
                            results.push((*node_id, actual_dist));
                        }
                    }
                    RTree3DEntry::Internal { child_index, .. } => {
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
enum Nearest3DEntry {
    Node { distance: f64, node_idx: usize },
    Leaf { distance: f64, node_id: NodeId },
}

impl PartialEq for Nearest3DEntry {
    fn eq(&self, other: &Self) -> bool {
        self.distance() == other.distance()
    }
}

impl Eq for Nearest3DEntry {}

impl PartialOrd for Nearest3DEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nearest3DEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .distance()
            .partial_cmp(&self.distance())
            .unwrap_or(Ordering::Equal)
    }
}

impl Nearest3DEntry {
    fn distance(&self) -> f64 {
        match self {
            Nearest3DEntry::Node { distance, .. } => *distance,
            Nearest3DEntry::Leaf { distance, .. } => *distance,
        }
    }
}
