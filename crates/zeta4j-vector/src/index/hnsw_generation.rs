//! Deterministic, storage-neutral HNSW generation construction and search.
//!
//! A generation is built completely in memory, then persisted by the logical
//! database repository. It never owns files, pages, WAL records, or commit state.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use super::{DistanceFunction, HNSWError, SearchResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HnswGenerationParams {
    pub m: u32,
    pub m_max0: u32,
    pub ef_construction: u32,
    pub ef_search: u32,
}

impl Default for HnswGenerationParams {
    fn default() -> Self {
        Self {
            m: 16,
            m_max0: 32,
            ef_construction: 200,
            ef_search: 50,
        }
    }
}

impl HnswGenerationParams {
    pub fn validate(self) -> Result<(), HNSWError> {
        if self.m < 2 {
            return Err(HNSWError::InvalidParams(
                "generation m must be at least 2".to_string(),
            ));
        }
        if self.m_max0 < self.m {
            return Err(HNSWError::InvalidParams(
                "generation m_max0 must be greater than or equal to m".to_string(),
            ));
        }
        if self.ef_construction < self.m {
            return Err(HNSWError::InvalidParams(
                "generation ef_construction must be greater than or equal to m".to_string(),
            ));
        }
        if self.ef_search == 0 {
            return Err(HNSWError::InvalidParams(
                "generation ef_search must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HnswGenerationNode {
    pub node_id: u64,
    pub vector: Vec<f32>,
    pub level: u8,
    pub neighbors: Vec<Vec<u64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HnswGeneration {
    pub dimensions: u32,
    pub distance: DistanceFunction,
    pub params: HnswGenerationParams,
    pub entry_point: Option<u64>,
    pub max_level: u8,
    pub nodes: Vec<HnswGenerationNode>,
}

impl HnswGeneration {
    /// Build a deterministic immutable HNSW generation. Input order does not
    /// affect node levels, adjacency, or the entry point.
    pub fn build(
        dimensions: u32,
        distance: DistanceFunction,
        params: HnswGenerationParams,
        vectors: impl IntoIterator<Item = (u64, Vec<f32>)>,
    ) -> Result<Self, HNSWError> {
        params.validate()?;
        if dimensions == 0 {
            return Err(HNSWError::InvalidParams(
                "generation dimensions must be greater than zero".to_string(),
            ));
        }

        let mut seen = HashSet::new();
        let mut nodes = vectors
            .into_iter()
            .map(|(node_id, vector)| {
                if !seen.insert(node_id) {
                    return Err(HNSWError::InvalidParams(format!(
                        "duplicate generation node id {node_id}"
                    )));
                }
                if vector.len() != dimensions as usize {
                    return Err(HNSWError::DimensionMismatch {
                        expected: dimensions,
                        got: u32::try_from(vector.len()).unwrap_or(u32::MAX),
                    });
                }
                if !vector.iter().all(|value| value.is_finite()) {
                    return Err(HNSWError::InvalidParams(format!(
                        "generation node {node_id} contains a non-finite vector component"
                    )));
                }
                let level = deterministic_level(node_id, params.m);
                Ok(HnswGenerationNode {
                    node_id,
                    vector,
                    level,
                    neighbors: vec![Vec::new(); usize::from(level) + 1],
                })
            })
            .collect::<Result<Vec<_>, HNSWError>>()?;
        nodes.sort_by_key(|node| node.node_id);

        let max_level = nodes.iter().map(|node| node.level).max().unwrap_or(0);
        let entry_point = nodes
            .iter()
            .filter(|node| node.level == max_level)
            .map(|node| node.node_id)
            .min();
        let mut generation = Self {
            dimensions,
            distance,
            params,
            entry_point,
            max_level,
            nodes,
        };
        generation.build_adjacency()?;
        generation.validate()?;
        Ok(generation)
    }

    fn build_adjacency(&mut self) -> Result<(), HNSWError> {
        let Some(first) = self.nodes.first() else {
            return Ok(());
        };
        let mut build_entry = first.node_id;
        let mut build_max_level = first.level;

        for position in 1..self.nodes.len() {
            let node_id = self.nodes[position].node_id;
            let node_level = self.nodes[position].level;
            let vector = self.nodes[position].vector.clone();
            let mut entry = build_entry;

            if node_level < build_max_level {
                for layer in ((usize::from(node_level) + 1)..=usize::from(build_max_level)).rev() {
                    if let Some(nearest) = self.search_layer(&vector, entry, 1, layer)?.first() {
                        entry = nearest.id;
                    }
                }
            }

            let shared_max = node_level.min(build_max_level);
            for layer in (0..=usize::from(shared_max)).rev() {
                let candidates =
                    self.search_layer(&vector, entry, self.params.ef_construction as usize, layer)?;
                let limit = self.layer_limit(layer);
                for neighbor in candidates.iter().take(limit).map(|result| result.id) {
                    self.connect_and_prune(layer, node_id, neighbor)?;
                }
                if let Some(nearest) = candidates.first() {
                    entry = nearest.id;
                }
            }

            if node_level > build_max_level {
                build_entry = node_id;
                build_max_level = node_level;
            }
        }
        Ok(())
    }

    fn node_position(&self, node_id: u64) -> Result<usize, HNSWError> {
        self.nodes
            .binary_search_by_key(&node_id, |node| node.node_id)
            .map_err(|_| HNSWError::VectorNotFound(node_id))
    }

    fn node_distance(&self, left: u64, right: u64) -> Result<f32, HNSWError> {
        let left = self.node_position(left)?;
        let right = self.node_position(right)?;
        Ok(self
            .distance
            .compute(&self.nodes[left].vector, &self.nodes[right].vector))
    }

    fn layer_limit(&self, layer: usize) -> usize {
        if layer == 0 {
            self.params.m_max0 as usize
        } else {
            self.params.m as usize
        }
    }

    fn connect_and_prune(&mut self, layer: usize, left: u64, right: u64) -> Result<(), HNSWError> {
        if left == right {
            return Ok(());
        }
        let left_position = self.node_position(left)?;
        let right_position = self.node_position(right)?;
        if !self.nodes[left_position].neighbors[layer].contains(&right) {
            self.nodes[left_position].neighbors[layer].push(right);
        }
        if !self.nodes[right_position].neighbors[layer].contains(&left) {
            self.nodes[right_position].neighbors[layer].push(left);
        }
        self.prune_node(layer, left)?;
        self.prune_node(layer, right)?;
        Ok(())
    }

    fn prune_node(&mut self, layer: usize, node_id: u64) -> Result<(), HNSWError> {
        let position = self.node_position(node_id)?;
        let mut by_distance = self.nodes[position].neighbors[layer]
            .iter()
            .map(|neighbor| {
                self.node_distance(node_id, *neighbor)
                    .map(|distance| (OrderedFloat(distance), *neighbor))
            })
            .collect::<Result<Vec<_>, _>>()?;
        by_distance.sort_unstable();
        by_distance.truncate(self.layer_limit(layer));
        let mut neighbors = by_distance
            .into_iter()
            .map(|(_, neighbor)| neighbor)
            .collect::<Vec<_>>();
        neighbors.sort_unstable();
        let retained = neighbors.iter().copied().collect::<HashSet<_>>();
        let removed = self.nodes[position].neighbors[layer]
            .iter()
            .copied()
            .filter(|neighbor| !retained.contains(neighbor))
            .collect::<Vec<_>>();
        self.nodes[position].neighbors[layer] = neighbors;
        for neighbor in removed {
            let neighbor_position = self.node_position(neighbor)?;
            if let Some(adjacency) = self.nodes[neighbor_position].neighbors.get_mut(layer) {
                adjacency.retain(|candidate| *candidate != node_id);
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), HNSWError> {
        self.params.validate()?;
        let ids = self
            .nodes
            .iter()
            .map(|node| node.node_id)
            .collect::<HashSet<_>>();
        if ids.len() != self.nodes.len() {
            return Err(HNSWError::InvalidParams(
                "generation contains duplicate node ids".to_string(),
            ));
        }
        if self.nodes.is_empty() {
            if self.entry_point.is_some() || self.max_level != 0 {
                return Err(HNSWError::InvalidParams(
                    "empty generation has entry-point metadata".to_string(),
                ));
            }
            return Ok(());
        }
        let entry = self.entry_point.ok_or_else(|| {
            HNSWError::InvalidParams("non-empty generation has no entry point".to_string())
        })?;
        let entry_position = self.node_position(entry)?;
        if self.nodes[entry_position].level != self.max_level {
            return Err(HNSWError::InvalidParams(
                "generation entry point is not at max level".to_string(),
            ));
        }
        for node in &self.nodes {
            if node.vector.len() != self.dimensions as usize {
                return Err(HNSWError::DimensionMismatch {
                    expected: self.dimensions,
                    got: u32::try_from(node.vector.len()).unwrap_or(u32::MAX),
                });
            }
            if node.neighbors.len() != usize::from(node.level) + 1 {
                return Err(HNSWError::InvalidParams(format!(
                    "generation node {} has invalid layer count",
                    node.node_id
                )));
            }
            for (layer, neighbors) in node.neighbors.iter().enumerate() {
                if neighbors.len() > self.layer_limit(layer) {
                    return Err(HNSWError::InvalidParams(format!(
                        "generation node {} exceeds layer {} degree",
                        node.node_id, layer
                    )));
                }
                let mut unique = HashSet::new();
                for neighbor in neighbors {
                    if *neighbor == node.node_id || !unique.insert(*neighbor) {
                        return Err(HNSWError::InvalidParams(format!(
                            "generation node {} has invalid neighbor {}",
                            node.node_id, neighbor
                        )));
                    }
                    let neighbor_position = self.node_position(*neighbor)?;
                    let reciprocal = self.nodes[neighbor_position]
                        .neighbors
                        .get(layer)
                        .is_some_and(|adjacent| adjacent.contains(&node.node_id));
                    if !reciprocal {
                        return Err(HNSWError::InvalidParams(format!(
                            "generation edge {}->{} is not reciprocal",
                            node.node_id, neighbor
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn search(
        &self,
        query: &[f32],
        k: usize,
        ef: Option<usize>,
    ) -> Result<Vec<SearchResult>, HNSWError> {
        if query.len() != self.dimensions as usize {
            return Err(HNSWError::DimensionMismatch {
                expected: self.dimensions,
                got: u32::try_from(query.len()).unwrap_or(u32::MAX),
            });
        }
        let Some(mut entry) = self.entry_point else {
            return Ok(Vec::new());
        };
        for layer in (1..=usize::from(self.max_level)).rev() {
            let nearest = self.search_layer(query, entry, 1, layer)?;
            if let Some(result) = nearest.first() {
                entry = result.id;
            }
        }
        let ef = ef.unwrap_or(self.params.ef_search as usize).max(k).max(1);
        let mut results = self.search_layer(query, entry, ef, 0)?;
        results.truncate(k);
        Ok(results)
    }

    fn search_layer(
        &self,
        query: &[f32],
        entry: u64,
        ef: usize,
        layer: usize,
    ) -> Result<Vec<SearchResult>, HNSWError> {
        let mut visited = HashSet::from([entry]);
        let entry_distance = self.distance_to_query(query, entry)?;
        let mut candidates = BinaryHeap::from([Reverse((OrderedFloat(entry_distance), entry))]);
        let mut results = BinaryHeap::from([(OrderedFloat(entry_distance), entry)]);

        while let Some(Reverse((OrderedFloat(candidate_distance), candidate_id))) = candidates.pop()
        {
            let furthest = results
                .peek()
                .map(|(distance, _)| distance.0)
                .unwrap_or(f32::MAX);
            if candidate_distance > furthest && results.len() >= ef {
                break;
            }
            let position = self.node_position(candidate_id)?;
            let Some(neighbors) = self.nodes[position].neighbors.get(layer) else {
                continue;
            };
            for neighbor in neighbors {
                if !visited.insert(*neighbor) {
                    continue;
                }
                let distance = self.distance_to_query(query, *neighbor)?;
                let furthest = results.peek().map(|(value, _)| value.0).unwrap_or(f32::MAX);
                if distance < furthest || results.len() < ef {
                    candidates.push(Reverse((OrderedFloat(distance), *neighbor)));
                    results.push((OrderedFloat(distance), *neighbor));
                    if results.len() > ef {
                        results.pop();
                    }
                }
            }
        }

        let mut ordered = results
            .into_iter()
            .map(|(distance, id)| SearchResult::new(id, distance.0))
            .collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            left.distance
                .total_cmp(&right.distance)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(ordered)
    }

    fn distance_to_query(&self, query: &[f32], node_id: u64) -> Result<f32, HNSWError> {
        let position = self.node_position(node_id)?;
        Ok(self.distance.compute(query, &self.nodes[position].vector))
    }
}

/// Stable level assignment is required for deterministic rebuilds.
/// SplitMix64 provides a well-distributed value per ID.
fn deterministic_level(node_id: u64, m: u32) -> u8 {
    let mut value = node_id.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    let mantissa = (value >> 11) + 1;
    let uniform = mantissa as f64 / ((1_u64 << 53) as f64 + 1.0);
    let multiplier = 1.0 / f64::from(m).ln();
    (-uniform.ln() * multiplier).floor().min(32.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vectors() -> Vec<(u64, Vec<f32>)> {
        vec![
            (4, vec![4.0, 0.0]),
            (1, vec![1.0, 0.0]),
            (3, vec![3.0, 0.0]),
            (2, vec![2.0, 0.0]),
        ]
    }

    #[test]
    fn build_is_deterministic_and_input_order_independent() -> Result<(), HNSWError> {
        let first = HnswGeneration::build(
            2,
            DistanceFunction::Euclidean,
            HnswGenerationParams::default(),
            vectors(),
        )?;
        let mut reversed = vectors();
        reversed.reverse();
        let second = HnswGeneration::build(
            2,
            DistanceFunction::Euclidean,
            HnswGenerationParams::default(),
            reversed,
        )?;
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn generation_is_reciprocal_bounded_and_searchable() -> Result<(), HNSWError> {
        let generation = HnswGeneration::build(
            2,
            DistanceFunction::Euclidean,
            HnswGenerationParams {
                m: 2,
                m_max0: 2,
                ef_construction: 4,
                ef_search: 4,
            },
            vectors(),
        )?;
        generation.validate()?;
        let results = generation.search(&[2.1, 0.0], 2, None)?;
        assert_eq!(
            results.iter().map(|result| result.id).collect::<Vec<_>>(),
            vec![2, 3]
        );
        Ok(())
    }

    #[test]
    fn rejects_non_finite_vector_before_building_graph() {
        let result = HnswGeneration::build(
            2,
            DistanceFunction::Cosine,
            HnswGenerationParams::default(),
            [(1, vec![f32::NAN, 0.0])],
        );
        assert!(matches!(result, Err(HNSWError::InvalidParams(_))));
    }
}
