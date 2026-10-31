// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! FastRP (Fast Random Projection) 임베딩 알고리즘
//!
//! Node2Vec보다 빠른 그래프 임베딩 생성 알고리즘입니다.
//! 랜덤 행렬을 사용한 차원 축소로 O(E) 복잡도를 가집니다.
//!
//! ## 알고리즘 개요
//! 1. 각 노드에 랜덤 초기 임베딩 할당
//! 2. K번 반복: 이웃 임베딩 평균으로 업데이트 (+ L2 정규화)
//! 3. 각 iteration의 임베딩을 가중 합산하여 최종 결과 생성
//!
//! ## 사용 예시
//! ```ignore
//! let config = FastRPConfig::default()
//!     .with_embedding_dimension(128)
//!     .with_iteration_weights(vec![0.0, 1.0, 1.0]);
//!
//! let fastrp = FastRP::with_config(&engine, config);
//! let result = fastrp.generate(None).await?;
//!
//! for (node_id, embedding) in result.embeddings {
//!     println!("Node {} embedding: {:?}", node_id, &embedding[..5]);
//! }
//! ```

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

/// FastRP 설정
#[derive(Debug, Clone)]
pub struct FastRPConfig {
    /// 임베딩 차원 (기본값: 128)
    pub embedding_dimension: usize,
    /// Iteration별 가중치 (기본값: [0.0, 1.0, 1.0])
    /// - iteration_weights[0]: 초기 임베딩 가중치
    /// - iteration_weights[i]: i번째 propagation 가중치
    pub iteration_weights: Vec<f64>,
    /// L2 정규화 강도 (0.0 = 정규화 안함, 기본값: 0.0)
    pub normalization_strength: f64,
    /// 노드 속성 사용 비율 (0.0 = 랜덤 초기화만, 기본값: 0.0)
    pub property_ratio: f64,
    /// 관계 타입 필터 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 랜덤 시드 (재현성을 위해)
    pub seed: Option<u64>,
}

impl Default for FastRPConfig {
    fn default() -> Self {
        Self {
            embedding_dimension: 128,
            iteration_weights: vec![0.0, 1.0, 1.0], // GDS 기본값
            normalization_strength: 0.0,
            property_ratio: 0.0,
            relationship_types: None,
            seed: None,
        }
    }
}

impl FastRPConfig {
    /// 임베딩 차원 설정
    pub fn with_embedding_dimension(mut self, dim: usize) -> Self {
        self.embedding_dimension = dim;
        self
    }

    /// Iteration 가중치 설정
    pub fn with_iteration_weights(mut self, weights: Vec<f64>) -> Self {
        self.iteration_weights = weights;
        self
    }

    /// 정규화 강도 설정
    pub fn with_normalization_strength(mut self, strength: f64) -> Self {
        self.normalization_strength = strength;
        self
    }

    /// 속성 비율 설정
    pub fn with_property_ratio(mut self, ratio: f64) -> Self {
        self.property_ratio = ratio;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 랜덤 시드 설정
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

/// FastRP 결과
#[derive(Debug, Clone)]
pub struct FastRPResult {
    /// 노드별 임베딩 벡터
    pub embeddings: HashMap<NodeId, Vec<f32>>,
    /// 임베딩 차원
    pub dimension: usize,
    /// 처리된 노드 수
    pub node_count: usize,
}

/// FastRP 알고리즘 실행기
pub struct FastRP<'a> {
    engine: &'a GraphEngine,
    config: FastRPConfig,
}

impl<'a> FastRP<'a> {
    /// 기본 설정으로 FastRP 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: FastRPConfig::default(),
        }
    }

    /// 커스텀 설정으로 FastRP 생성
    pub fn with_config(engine: &'a GraphEngine, config: FastRPConfig) -> Self {
        Self { engine, config }
    }

    /// FastRP 임베딩 생성
    pub async fn generate(&self, rel_types: Option<&[String]>) -> Result<FastRPResult> {
        // 노드 수집
        let mut node_ids: Vec<NodeId> = Vec::new();
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                node_ids.push(node_id);
            }
        }

        let n = node_ids.len();

        if n == 0 {
            return Ok(FastRPResult {
                embeddings: HashMap::new(),
                dimension: self.config.embedding_dimension,
                node_count: 0,
            });
        }

        let dim = self.config.embedding_dimension;
        let num_iterations = self.config.iteration_weights.len();

        // 노드 ID → 인덱스 매핑
        let node_to_idx: HashMap<NodeId, usize> = node_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();

        // RNG 초기화
        let mut rng = if let Some(seed) = self.config.seed {
            ChaCha8Rng::seed_from_u64(seed)
        } else {
            ChaCha8Rng::from_os_rng()
        };

        // 랜덤 초기 임베딩 (sparse random projection)
        // 각 차원에 +1, -1, 0을 랜덤하게 할당 (sparsity = 1/sqrt(dim))
        let mut embeddings: Vec<Vec<f32>> = vec![vec![0.0f32; dim]; n];

        let sparsity = 1.0 / (dim as f64).sqrt();
        for emb in &mut embeddings {
            for val in emb.iter_mut() {
                let r: f64 = rng.random();
                *val = if r < sparsity / 2.0 {
                    (dim as f32).sqrt()
                } else if r < sparsity {
                    -(dim as f32).sqrt()
                } else {
                    0.0
                };
            }
        }

        // 활성 관계 타입
        let active_rel_types = rel_types.or(self.config.relationship_types.as_deref());

        // 인접 리스트 구축 (양방향)
        let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];

        // 엣지를 순회하여 인접 리스트 구축
        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(types) = active_rel_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // 양방향 인접 리스트 업데이트
                if let (Some(&from_idx), Some(&to_idx)) =
                    (node_to_idx.get(&from), node_to_idx.get(&to))
                {
                    adjacency[from_idx].push(to_idx);
                    adjacency[to_idx].push(from_idx);
                }
            }
        }

        // Iteration별 임베딩 저장
        let mut iteration_embeddings: Vec<Vec<Vec<f32>>> = Vec::with_capacity(num_iterations);
        iteration_embeddings.push(embeddings.clone()); // X_0 (초기 임베딩)

        // Propagation iterations
        for _iter in 1..num_iterations {
            let prev_emb = match iteration_embeddings.last() {
                Some(emb) => emb,
                None => break,
            };
            let mut new_emb: Vec<Vec<f32>> = vec![vec![0.0f32; dim]; n];

            for (i, neighbors) in adjacency.iter().enumerate() {
                if neighbors.is_empty() {
                    // 이웃이 없으면 자기 자신의 임베딩 유지
                    new_emb[i] = prev_emb[i].clone();
                } else {
                    // 이웃 임베딩의 평균
                    let neighbor_count = neighbors.len() as f32;
                    for d in 0..dim {
                        let sum: f32 = neighbors.iter().map(|&j| prev_emb[j][d]).sum();
                        new_emb[i][d] = sum / neighbor_count;
                    }
                }
            }

            // L2 정규화 (옵션)
            if self.config.normalization_strength > 0.0 {
                for emb in &mut new_emb {
                    let norm: f32 = emb.iter().map(|&x| x * x).sum::<f32>().sqrt();
                    if norm > 0.0 {
                        let factor = 1.0 / norm;
                        for val in emb.iter_mut() {
                            *val *= factor;
                        }
                    }
                }
            }

            iteration_embeddings.push(new_emb);
        }

        // 가중 합산하여 최종 임베딩 생성
        let mut final_embeddings: Vec<Vec<f32>> = vec![vec![0.0f32; dim]; n];
        for (iter_idx, iter_emb) in iteration_embeddings.iter().enumerate() {
            let weight = self.config.iteration_weights[iter_idx] as f32;
            if weight == 0.0 {
                continue;
            }
            for i in 0..n {
                for d in 0..dim {
                    final_embeddings[i][d] += weight * iter_emb[i][d];
                }
            }
        }

        // 최종 L2 정규화
        for emb in &mut final_embeddings {
            let norm: f32 = emb.iter().map(|&x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                let factor = 1.0 / norm;
                for val in emb.iter_mut() {
                    *val *= factor;
                }
            }
        }

        // 결과 변환
        let mut result_embeddings: HashMap<NodeId, Vec<f32>> = HashMap::new();
        for (i, &node_id) in node_ids.iter().enumerate() {
            result_embeddings.insert(node_id, final_embeddings[i].clone());
        }

        Ok(FastRPResult {
            embeddings: result_embeddings,
            dimension: dim,
            node_count: n,
        })
    }
}

/// 단순화된 FastRP 함수
pub async fn fastrp(
    engine: &GraphEngine,
    embedding_dimension: usize,
    iteration_weights: Vec<f64>,
) -> Result<FastRPResult> {
    let config = FastRPConfig::default()
        .with_embedding_dimension(embedding_dimension)
        .with_iteration_weights(iteration_weights);
    FastRP::with_config(engine, config).generate(None).await
}
