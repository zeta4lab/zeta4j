// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Poolable 트레이트 구현 - 메모리 풀 재사용 가능 객체

use crate::core::{Edge, Node, PropertyContainer};
use crate::storage::memory::Poolable;

/// Node를 Poolable하게 만들기
impl Poolable for Node {
    fn reset(&mut self) {
        // 노드 초기화 (속성만 클리어, 레이블은 수동 제거)
        self.properties_mut().clear();
        // 레이블 제거는 하나씩 수동으로 처리
        let labels_to_remove: Vec<String> = self.labels().iter().cloned().collect();
        for label in labels_to_remove {
            self.remove_label(&label);
        }
    }

    fn prepare_for_pool(&mut self) {
        // 풀 반환 전 정리
        self.properties_mut().clear();
        let labels_to_remove: Vec<String> = self.labels().iter().cloned().collect();
        for label in labels_to_remove {
            self.remove_label(&label);
        }
    }

    fn memory_size(&self) -> usize {
        self.memory_size()
    }
}

/// Edge를 Poolable하게 만들기
impl Poolable for Edge {
    fn reset(&mut self) {
        // 엣지 초기화
        self.properties_mut().clear();
    }

    fn prepare_for_pool(&mut self) {
        // 풀 반환 전 정리
        self.properties_mut().clear();
    }

    fn memory_size(&self) -> usize {
        self.memory_size()
    }
}
