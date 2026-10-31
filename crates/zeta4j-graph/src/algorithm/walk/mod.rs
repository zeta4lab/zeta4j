// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Random Walk 및 그래프 임베딩 알고리즘 모듈
//!
//! 그래프에서 무작위 탐색 및 임베딩을 수행하는 알고리즘들을 제공합니다.
//!
//! - Random Walk: 기본 무작위 탐색
//! - Random Walk with Restart (RWR): 시작점 복귀 기반 탐색
//! - Node2Vec: 편향된 무작위 탐색 (p, q 파라미터)
//! - FastRP: Fast Random Projection 임베딩

mod fastrp;
mod node2vec;
mod random_walk;
mod rwr;

pub use random_walk::{
    RandomWalk, RandomWalkConfig, RandomWalkResult, WalkDirection, random_walk, random_walks,
};

pub use rwr::{RWRConfig, RWRResult, RandomWalkWithRestart, rwr, rwr_with_restart_prob};

pub use node2vec::{
    BiasedRandomWalk, Node2VecConfig, Node2VecResult, node2vec_walk, node2vec_walks,
};

pub use fastrp::{FastRP, FastRPConfig, FastRPResult, fastrp};
