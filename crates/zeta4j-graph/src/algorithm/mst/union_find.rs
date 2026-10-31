// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Union-Find (Disjoint Set Union) 자료구조
//!
//! 분리 집합을 효율적으로 관리합니다.
//! 경로 압축(Path Compression)과 랭크 기반 합집합(Union by Rank)을 사용하여
//! 거의 O(1) 시간에 연산을 수행합니다.

/// Union-Find 자료구조
#[derive(Debug, Clone)]
pub struct UnionFind {
    /// 각 원소의 부모 (루트면 자기 자신)
    parent: Vec<usize>,
    /// 각 트리의 랭크 (높이의 상한)
    rank: Vec<usize>,
    /// 집합의 개수
    set_count: usize,
}

impl UnionFind {
    /// 새로운 Union-Find 생성
    ///
    /// 초기에 각 원소는 자기 자신만을 포함하는 집합
    pub fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
            set_count: size,
        }
    }

    /// 원소 x가 속한 집합의 대표(루트) 찾기
    ///
    /// 경로 압축을 적용하여 이후 연산 가속
    pub fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    /// 두 원소가 같은 집합에 속하는지 확인
    pub fn connected(&mut self, x: usize, y: usize) -> bool {
        self.find(x) == self.find(y)
    }

    /// 두 집합을 합침
    ///
    /// 랭크 기반 합집합으로 트리 높이 최소화
    /// 이미 같은 집합이면 false, 합쳤으면 true 반환
    pub fn union(&mut self, x: usize, y: usize) -> bool {
        let root_x = self.find(x);
        let root_y = self.find(y);

        if root_x == root_y {
            return false;
        }

        // 랭크 기반 합집합
        match self.rank[root_x].cmp(&self.rank[root_y]) {
            std::cmp::Ordering::Less => {
                self.parent[root_x] = root_y;
            }
            std::cmp::Ordering::Greater => {
                self.parent[root_y] = root_x;
            }
            std::cmp::Ordering::Equal => {
                self.parent[root_y] = root_x;
                self.rank[root_x] += 1;
            }
        }

        self.set_count -= 1;
        true
    }

    /// 현재 집합의 개수
    pub fn count(&self) -> usize {
        self.set_count
    }

    /// 전체 원소 수
    pub fn size(&self) -> usize {
        self.parent.len()
    }

    /// 특정 집합의 크기 (원소 수)
    pub fn set_size(&mut self, x: usize) -> usize {
        let root = self.find(x);
        (0..self.parent.len())
            .filter(|&i| self.find(i) == root)
            .count()
    }

    /// 모든 집합의 대표 원소들
    pub fn roots(&mut self) -> Vec<usize> {
        (0..self.parent.len())
            .filter(|&i| self.find(i) == i)
            .collect()
    }

    /// 특정 집합에 속한 모든 원소
    pub fn members(&mut self, x: usize) -> Vec<usize> {
        let root = self.find(x);
        (0..self.parent.len())
            .filter(|&i| self.find(i) == root)
            .collect()
    }

    /// 모든 연결이 끊어진 상태로 리셋
    pub fn reset(&mut self) {
        for i in 0..self.parent.len() {
            self.parent[i] = i;
            self.rank[i] = 0;
        }
        self.set_count = self.parent.len();
    }
}
