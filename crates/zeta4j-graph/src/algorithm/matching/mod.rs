// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 이분 매칭 알고리즘
//!
//! 이분 그래프(Bipartite Graph)에서 최대 매칭을 찾는 알고리즘들을 제공합니다.
//!
//! - Bipartite Check: O(V + E) - BFS 2-coloring으로 이분 그래프 판별
//! - Kuhn's Algorithm: O(V × E) - 단순한 증가 경로 기반 최대 매칭
//! - Hopcroft-Karp: O(E × √V) - 효율적인 최대 이분 매칭
//! - Hungarian: O(V³) - 가중치 이분 매칭 (최소/최대 비용)

mod bipartite;
mod hopcroft_karp;
mod hungarian;

pub use bipartite::{
    BipartiteChecker, BipartiteConfig, BipartiteResult, MatchingResult, bipartite_partition,
    is_bipartite, kuhn_matching, maximum_matching,
};

pub use hopcroft_karp::{HopcroftKarp, HopcroftKarpConfig, hopcroft_karp};

pub use hungarian::{
    Hungarian, HungarianConfig, HungarianResult, hungarian, max_weight_matching, min_cost_matching,
};
