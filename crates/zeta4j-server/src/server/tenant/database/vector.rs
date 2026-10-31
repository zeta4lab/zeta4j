// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Canonical vector records and staged HNSW generations for one logical database.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeta4j_common::CanonicalEntityRef;
use zeta4j_vector::{
    DistanceFunction, HnswGeneration, HnswGenerationNode as RuntimeHnswNode, HnswGenerationParams,
};

type BytesTable = TableDefinition<'static, &'static [u8], &'static [u8]>;

const VECTOR_GENERATION_COUNTERS: BytesTable = TableDefinition::new("vector_generation_counters");
const VECTOR_GENERATION_METADATA: BytesTable = TableDefinition::new("vector_generation_metadata");
const VECTOR_GENERATION_RECORDS: BytesTable = TableDefinition::new("vector_generation_records");
const VECTOR_GENERATION_NODES: BytesTable = TableDefinition::new("vector_generation_nodes");
const VECTOR_ACTIVE_GENERATIONS: BytesTable = TableDefinition::new("vector_active_generations");
const VECTOR_DROPPED_INDICES: BytesTable = TableDefinition::new("vector_dropped_indices");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorDistance {
    Euclidean,
    Cosine,
    DotProduct,
}

pub type VectorEntityRef = CanonicalEntityRef;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorRecord {
    pub node_id: u64,
    pub entity: VectorEntityRef,
    pub vector: Vec<f32>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorEntityBinding {
    pub node_id: u64,
    pub entity: VectorEntityRef,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HnswGenerationNode {
    pub node_id: u64,
    pub level: u8,
    pub neighbors: Vec<Vec<u64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorGenerationWriteSet {
    pub index_name: String,
    pub dimensions: u32,
    pub distance: VectorDistance,
    pub entry_point: Option<u64>,
    pub max_level: u8,
    pub m: u32,
    pub m_max0: u32,
    pub ef_construction: u32,
    pub ef_search: u32,
    pub records: Vec<VectorRecord>,
    pub nodes: Vec<HnswGenerationNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VectorGenerationMetadata {
    dimensions: u32,
    distance: VectorDistance,
    entry_point: Option<u64>,
    max_level: u8,
    m: u32,
    m_max0: u32,
    ef_construction: u32,
    ef_search: u32,
    record_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveVectorGeneration {
    pub generation: u64,
    pub database_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedVectorGeneration {
    index_name: String,
    generation: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VectorGcReport {
    pub generations_removed: usize,
    pub records_removed: usize,
    pub nodes_removed: usize,
}

impl StagedVectorGeneration {
    pub fn index_name(&self) -> &str {
        &self.index_name
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Debug, Error)]
pub enum VectorRepositoryError {
    #[error("vector field '{field}' is required")]
    MissingField { field: &'static str },
    #[error("vector dimensions must be greater than zero")]
    InvalidDimensions,
    #[error("invalid HNSW parameters: {0}")]
    InvalidParameters(String),
    #[error("vector node {node_id} has dimension {found}; expected {expected}")]
    DimensionMismatch {
        node_id: u64,
        expected: u32,
        found: usize,
    },
    #[error("vector node {node_id} contains a non-finite component")]
    NonFiniteVector { node_id: u64 },
    #[error("duplicate vector node id {node_id}")]
    DuplicateNodeId { node_id: u64 },
    #[error("duplicate canonical vector entity {entity:?}")]
    DuplicateEntity { entity: VectorEntityRef },
    #[error("duplicate entity binding for vector node {node_id}")]
    DuplicateEntityBinding { node_id: u64 },
    #[error("missing entity binding for vector node {node_id}")]
    MissingEntityBinding { node_id: u64 },
    #[error("entity binding references unknown vector node {node_id}")]
    UnknownEntityBinding { node_id: u64 },
    #[error("HNSW node set does not exactly match vector record node set")]
    NodeSetMismatch,
    #[error("HNSW node {node_id} declares level {level} but has {layers} adjacency layers")]
    InvalidLayerCount {
        node_id: u64,
        level: u8,
        layers: usize,
    },
    #[error("HNSW node {node_id} references unknown neighbor {neighbor_id}")]
    UnknownNeighbor { node_id: u64, neighbor_id: u64 },
    #[error("HNSW node {node_id} has duplicate or self neighbor {neighbor_id} at layer {layer}")]
    InvalidNeighbor {
        node_id: u64,
        neighbor_id: u64,
        layer: usize,
    },
    #[error("HNSW edge {node_id}->{neighbor_id} at layer {layer} is not reciprocal")]
    AsymmetricEdge {
        node_id: u64,
        neighbor_id: u64,
        layer: usize,
    },
    #[error("HNSW entry point {entry_point} is not present")]
    UnknownEntryPoint { entry_point: u64 },
    #[error("HNSW empty generation cannot have an entry point")]
    EmptyEntryPoint,
    #[error("HNSW non-empty generation requires an entry point")]
    MissingEntryPoint,
    #[error("HNSW max level {declared} does not match node maximum {found}")]
    MaxLevelMismatch { declared: u8, found: u8 },
    #[error("HNSW entry point {entry_point} is at level {found}; expected max level {expected}")]
    EntryPointLevelMismatch {
        entry_point: u64,
        expected: u8,
        found: u8,
    },
    #[error("HNSW node {node_id} has {found} neighbors at layer {layer}; maximum is {maximum}")]
    TooManyNeighbors {
        node_id: u64,
        layer: usize,
        found: usize,
        maximum: u32,
    },
    #[error("staged vector generation {generation} for index '{index_name}' does not exist")]
    GenerationNotFound { index_name: String, generation: u64 },
    #[error("vector index '{index_name}' has no active generation")]
    IndexNotFound { index_name: String },
    #[error("vector generation counter overflow for index '{index_name}'")]
    GenerationOverflow { index_name: String },
    #[error("vector repository storage error: {0}")]
    Storage(String),
    #[error("vector repository serialization error: {0}")]
    Serialization(String),
    #[error("invalid HNSW generation: {0}")]
    InvalidHnsw(String),
}

impl VectorGenerationWriteSet {
    /// Convert storage-neutral HNSW output into the canonical persistence write set.
    pub fn from_hnsw_generation(
        index_name: impl Into<String>,
        generation: HnswGeneration,
        bindings: impl IntoIterator<Item = VectorEntityBinding>,
    ) -> Result<Self, VectorRepositoryError> {
        generation
            .validate()
            .map_err(|error| VectorRepositoryError::InvalidHnsw(error.to_string()))?;
        let mut bindings = bindings.into_iter().try_fold(
            HashMap::<u64, VectorEntityBinding>::new(),
            |mut by_node, binding| {
                let node_id = binding.node_id;
                if by_node.insert(node_id, binding).is_some() {
                    return Err(VectorRepositoryError::DuplicateEntityBinding { node_id });
                }
                Ok(by_node)
            },
        )?;

        let mut records = Vec::with_capacity(generation.nodes.len());
        let mut nodes = Vec::with_capacity(generation.nodes.len());
        for node in generation.nodes {
            let binding = bindings.remove(&node.node_id).ok_or(
                VectorRepositoryError::MissingEntityBinding {
                    node_id: node.node_id,
                },
            )?;
            records.push(VectorRecord {
                node_id: node.node_id,
                entity: binding.entity,
                vector: node.vector,
                metadata_json: binding.metadata_json,
            });
            nodes.push(HnswGenerationNode {
                node_id: node.node_id,
                level: node.level,
                neighbors: node.neighbors,
            });
        }
        if let Some(node_id) = bindings.keys().copied().min() {
            return Err(VectorRepositoryError::UnknownEntityBinding { node_id });
        }

        let write_set = Self {
            index_name: index_name.into(),
            dimensions: generation.dimensions,
            distance: generation.distance.into(),
            entry_point: generation.entry_point,
            max_level: generation.max_level,
            m: generation.params.m,
            m_max0: generation.params.m_max0,
            ef_construction: generation.params.ef_construction,
            ef_search: generation.params.ef_search,
            records,
            nodes,
        };
        write_set.validate()?;
        Ok(write_set)
    }

    pub fn validate(&self) -> Result<(), VectorRepositoryError> {
        if self.index_name.trim().is_empty() {
            return Err(VectorRepositoryError::MissingField {
                field: "index_name",
            });
        }
        if self.dimensions == 0 {
            return Err(VectorRepositoryError::InvalidDimensions);
        }
        if self.m < 2 {
            return Err(VectorRepositoryError::InvalidParameters(
                "m must be at least 2".to_string(),
            ));
        }
        if self.m_max0 < self.m {
            return Err(VectorRepositoryError::InvalidParameters(
                "m_max0 must be greater than or equal to m".to_string(),
            ));
        }
        if self.ef_construction < self.m {
            return Err(VectorRepositoryError::InvalidParameters(
                "ef_construction must be greater than or equal to m".to_string(),
            ));
        }
        if self.ef_search == 0 {
            return Err(VectorRepositoryError::InvalidParameters(
                "ef_search must be greater than zero".to_string(),
            ));
        }

        let mut record_ids = HashSet::with_capacity(self.records.len());
        let mut entities = HashSet::with_capacity(self.records.len());
        for record in &self.records {
            if record.vector.len() != self.dimensions as usize {
                return Err(VectorRepositoryError::DimensionMismatch {
                    node_id: record.node_id,
                    expected: self.dimensions,
                    found: record.vector.len(),
                });
            }
            if !record.vector.iter().all(|value| value.is_finite()) {
                return Err(VectorRepositoryError::NonFiniteVector {
                    node_id: record.node_id,
                });
            }
            if !record_ids.insert(record.node_id) {
                return Err(VectorRepositoryError::DuplicateNodeId {
                    node_id: record.node_id,
                });
            }
            validate_entity(&record.entity)?;
            if !entities.insert(record.entity.clone()) {
                return Err(VectorRepositoryError::DuplicateEntity {
                    entity: record.entity.clone(),
                });
            }
        }

        let nodes = self
            .nodes
            .iter()
            .map(|node| (node.node_id, node))
            .collect::<HashMap<_, _>>();
        if nodes.len() != self.nodes.len()
            || nodes.keys().copied().collect::<HashSet<_>>() != record_ids
        {
            return Err(VectorRepositoryError::NodeSetMismatch);
        }

        if nodes.is_empty() {
            if self.entry_point.is_some() {
                return Err(VectorRepositoryError::EmptyEntryPoint);
            }
            if self.max_level != 0 {
                return Err(VectorRepositoryError::MaxLevelMismatch {
                    declared: self.max_level,
                    found: 0,
                });
            }
            return Ok(());
        }

        let entry_point = self
            .entry_point
            .ok_or(VectorRepositoryError::MissingEntryPoint)?;
        if !nodes.contains_key(&entry_point) {
            return Err(VectorRepositoryError::UnknownEntryPoint { entry_point });
        }
        let found_max = self.nodes.iter().map(|node| node.level).max().unwrap_or(0);
        if found_max != self.max_level {
            return Err(VectorRepositoryError::MaxLevelMismatch {
                declared: self.max_level,
                found: found_max,
            });
        }
        let entry_level = nodes
            .get(&entry_point)
            .map(|node| node.level)
            .ok_or(VectorRepositoryError::UnknownEntryPoint { entry_point })?;
        if entry_level != self.max_level {
            return Err(VectorRepositoryError::EntryPointLevelMismatch {
                entry_point,
                expected: self.max_level,
                found: entry_level,
            });
        }

        for node in &self.nodes {
            if node.neighbors.len() != usize::from(node.level) + 1 {
                return Err(VectorRepositoryError::InvalidLayerCount {
                    node_id: node.node_id,
                    level: node.level,
                    layers: node.neighbors.len(),
                });
            }
            for (layer, neighbors) in node.neighbors.iter().enumerate() {
                let maximum = if layer == 0 { self.m_max0 } else { self.m };
                if neighbors.len() > maximum as usize {
                    return Err(VectorRepositoryError::TooManyNeighbors {
                        node_id: node.node_id,
                        layer,
                        found: neighbors.len(),
                        maximum,
                    });
                }
                let mut unique = HashSet::with_capacity(neighbors.len());
                for neighbor_id in neighbors {
                    let Some(neighbor) = nodes.get(neighbor_id) else {
                        return Err(VectorRepositoryError::UnknownNeighbor {
                            node_id: node.node_id,
                            neighbor_id: *neighbor_id,
                        });
                    };
                    if *neighbor_id == node.node_id || !unique.insert(*neighbor_id) {
                        return Err(VectorRepositoryError::InvalidNeighbor {
                            node_id: node.node_id,
                            neighbor_id: *neighbor_id,
                            layer,
                        });
                    }
                    let reciprocal = neighbor
                        .neighbors
                        .get(layer)
                        .is_some_and(|adjacent| adjacent.contains(&node.node_id));
                    if !reciprocal {
                        return Err(VectorRepositoryError::AsymmetricEdge {
                            node_id: node.node_id,
                            neighbor_id: *neighbor_id,
                            layer,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_entity(entity: &VectorEntityRef) -> Result<(), VectorRepositoryError> {
    let VectorEntityRef::GraphNode { element_id } = entity;
    if element_id.trim().is_empty() {
        return Err(VectorRepositoryError::MissingField {
            field: "vector.entity",
        });
    }
    Ok(())
}

#[derive(Clone)]
pub struct DatabaseVectorRepository {
    database: Arc<Database>,
}

impl DatabaseVectorRepository {
    pub(super) fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    /// Persist an immutable, non-visible generation. Expensive HNSW construction and
    /// serialization must be complete before this staging transaction starts.
    pub fn stage(
        &self,
        write_set: &VectorGenerationWriteSet,
    ) -> Result<StagedVectorGeneration, VectorRepositoryError> {
        write_set.validate()?;
        let metadata = VectorGenerationMetadata {
            dimensions: write_set.dimensions,
            distance: write_set.distance,
            entry_point: write_set.entry_point,
            max_level: write_set.max_level,
            m: write_set.m,
            m_max0: write_set.m_max0,
            ef_construction: write_set.ef_construction,
            ef_search: write_set.ef_search,
            record_count: write_set.records.len() as u64,
        };
        let metadata_bytes = serialize(&metadata)?;
        let record_bytes = write_set
            .records
            .iter()
            .map(|record| serialize(record).map(|bytes| (record.node_id, bytes)))
            .collect::<Result<Vec<_>, _>>()?;
        let node_bytes = write_set
            .nodes
            .iter()
            .map(|node| serialize(node).map(|bytes| (node.node_id, bytes)))
            .collect::<Result<Vec<_>, _>>()?;

        let transaction = self.database.begin_write().map_err(storage_error)?;
        let generation = {
            let mut counters = transaction
                .open_table(VECTOR_GENERATION_COUNTERS)
                .map_err(storage_error)?;
            let current = counters
                .get(write_set.index_name.as_bytes())
                .map_err(storage_error)?
                .map(|value| decode_u64(value.value()))
                .transpose()?
                .unwrap_or(0);
            let next = current.checked_add(1).ok_or_else(|| {
                VectorRepositoryError::GenerationOverflow {
                    index_name: write_set.index_name.clone(),
                }
            })?;
            counters
                .insert(
                    write_set.index_name.as_bytes(),
                    next.to_le_bytes().as_slice(),
                )
                .map_err(storage_error)?;
            next
        };

        let generation_prefix = generation_prefix(&write_set.index_name, generation)?;
        transaction
            .open_table(VECTOR_GENERATION_METADATA)
            .map_err(storage_error)?
            .insert(generation_prefix.as_slice(), metadata_bytes.as_slice())
            .map_err(storage_error)?;
        {
            let mut records = transaction
                .open_table(VECTOR_GENERATION_RECORDS)
                .map_err(storage_error)?;
            for (node_id, bytes) in record_bytes {
                let key = node_key(&generation_prefix, node_id);
                records
                    .insert(key.as_slice(), bytes.as_slice())
                    .map_err(storage_error)?;
            }
        }
        {
            let mut nodes = transaction
                .open_table(VECTOR_GENERATION_NODES)
                .map_err(storage_error)?;
            for (node_id, bytes) in node_bytes {
                let key = node_key(&generation_prefix, node_id);
                nodes
                    .insert(key.as_slice(), bytes.as_slice())
                    .map_err(storage_error)?;
            }
        }
        transaction.commit().map_err(storage_error)?;
        Ok(StagedVectorGeneration {
            index_name: write_set.index_name.clone(),
            generation,
        })
    }

    /// Change only the visibility pointer inside the coordinator-owned transaction.
    pub fn activate(
        &self,
        transaction: &WriteTransaction,
        staged: &StagedVectorGeneration,
        database_revision: u64,
    ) -> Result<(), VectorRepositoryError> {
        let generation_key = generation_prefix(&staged.index_name, staged.generation)?;
        let metadata = transaction
            .open_table(VECTOR_GENERATION_METADATA)
            .map_err(storage_error)?;
        if metadata
            .get(generation_key.as_slice())
            .map_err(storage_error)?
            .is_none()
        {
            return Err(VectorRepositoryError::GenerationNotFound {
                index_name: staged.index_name.clone(),
                generation: staged.generation,
            });
        }
        drop(metadata);

        let active = ActiveVectorGeneration {
            generation: staged.generation,
            database_revision,
        };
        let bytes = serialize(&active)?;
        transaction
            .open_table(VECTOR_ACTIVE_GENERATIONS)
            .map_err(storage_error)?
            .insert(staged.index_name.as_bytes(), bytes.as_slice())
            .map_err(storage_error)?;
        transaction
            .open_table(VECTOR_DROPPED_INDICES)
            .map_err(storage_error)?
            .remove(staged.index_name.as_bytes())
            .map_err(storage_error)?;
        Ok(())
    }

    /// Remove an index visibility pointer inside the coordinator transaction.
    pub fn deactivate(
        &self,
        transaction: &WriteTransaction,
        index_name: &str,
    ) -> Result<(), VectorRepositoryError> {
        let mut active = transaction
            .open_table(VECTOR_ACTIVE_GENERATIONS)
            .map_err(storage_error)?;
        let removed = {
            let value = active
                .remove(index_name.as_bytes())
                .map_err(storage_error)?;
            value.is_some()
        };
        if !removed {
            return Err(VectorRepositoryError::IndexNotFound {
                index_name: index_name.to_string(),
            });
        }
        drop(active);
        let marker: &[u8] = &[];
        transaction
            .open_table(VECTOR_DROPPED_INDICES)
            .map_err(storage_error)?
            .insert(index_name.as_bytes(), marker)
            .map_err(storage_error)?;
        Ok(())
    }

    /// Remove every generation except the active one. The caller must guarantee
    /// that no reader or stager still owns an inactive generation.
    pub fn prune_inactive(
        &self,
        transaction: &WriteTransaction,
        index_name: &str,
    ) -> Result<VectorGcReport, VectorRepositoryError> {
        let active = {
            let active_table = transaction
                .open_table(VECTOR_ACTIVE_GENERATIONS)
                .map_err(storage_error)?;
            active_table
                .get(index_name.as_bytes())
                .map_err(storage_error)?
                .map(|value| deserialize::<ActiveVectorGeneration>(value.value()))
                .transpose()?
                .ok_or_else(|| VectorRepositoryError::GenerationNotFound {
                    index_name: index_name.to_string(),
                    generation: 0,
                })?
        };
        let index_prefix = index_prefix(index_name)?;
        let active_prefix = generation_prefix(index_name, active.generation)?;
        let generations_removed = {
            let mut metadata = transaction
                .open_table(VECTOR_GENERATION_METADATA)
                .map_err(storage_error)?;
            let stale = prefixed_table_keys(&metadata, &index_prefix)?
                .into_iter()
                .filter(|key| key.as_slice() != active_prefix.as_slice())
                .collect::<Vec<_>>();
            for key in &stale {
                metadata.remove(key.as_slice()).map_err(storage_error)?;
            }
            stale.len()
        };
        let records_removed = {
            let mut records = transaction
                .open_table(VECTOR_GENERATION_RECORDS)
                .map_err(storage_error)?;
            let stale = prefixed_table_keys(&records, &index_prefix)?
                .into_iter()
                .filter(|key| !key.starts_with(&active_prefix))
                .collect::<Vec<_>>();
            for key in &stale {
                records.remove(key.as_slice()).map_err(storage_error)?;
            }
            stale.len()
        };
        let nodes_removed = {
            let mut nodes = transaction
                .open_table(VECTOR_GENERATION_NODES)
                .map_err(storage_error)?;
            let stale = prefixed_table_keys(&nodes, &index_prefix)?
                .into_iter()
                .filter(|key| !key.starts_with(&active_prefix))
                .collect::<Vec<_>>();
            for key in &stale {
                nodes.remove(key.as_slice()).map_err(storage_error)?;
            }
            stale.len()
        };
        Ok(VectorGcReport {
            generations_removed,
            records_removed,
            nodes_removed,
        })
    }

    pub fn active_generation(
        &self,
        index_name: &str,
    ) -> Result<Option<ActiveVectorGeneration>, VectorRepositoryError> {
        let transaction = self.database.begin_read().map_err(storage_error)?;
        let table = match transaction.open_table(VECTOR_ACTIVE_GENERATIONS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        };
        table
            .get(index_name.as_bytes())
            .map_err(storage_error)?
            .map(|value| deserialize(value.value()))
            .transpose()
    }

    pub fn list_active_indices(&self) -> Result<Vec<String>, VectorRepositoryError> {
        let transaction = self.database.begin_read().map_err(storage_error)?;
        let table = match transaction.open_table(VECTOR_ACTIVE_GENERATIONS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(storage_error(error)),
        };
        let mut indices = Vec::new();
        for entry in table.iter().map_err(storage_error)? {
            let (index_name, _) = entry.map_err(storage_error)?;
            let name = std::str::from_utf8(index_name.value())
                .map_err(|error| VectorRepositoryError::Storage(error.to_string()))?;
            indices.push(name.to_string());
        }
        indices.sort();
        Ok(indices)
    }

    pub fn list_dropped_indices(&self) -> Result<Vec<String>, VectorRepositoryError> {
        let transaction = self.database.begin_read().map_err(storage_error)?;
        let table = match transaction.open_table(VECTOR_DROPPED_INDICES) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(storage_error(error)),
        };
        let mut indices = Vec::new();
        for entry in table.iter().map_err(storage_error)? {
            let (index_name, _) = entry.map_err(storage_error)?;
            let name = std::str::from_utf8(index_name.value())
                .map_err(|error| VectorRepositoryError::Storage(error.to_string()))?;
            indices.push(name.to_string());
        }
        indices.sort();
        Ok(indices)
    }

    /// Purge every generation for a tombstoned index during database reopen.
    pub fn prune_dropped(
        &self,
        transaction: &WriteTransaction,
        index_name: &str,
    ) -> Result<VectorGcReport, VectorRepositoryError> {
        let tombstoned = transaction
            .open_table(VECTOR_DROPPED_INDICES)
            .map_err(storage_error)?
            .get(index_name.as_bytes())
            .map_err(storage_error)?
            .is_some();
        if !tombstoned {
            return Err(VectorRepositoryError::IndexNotFound {
                index_name: index_name.to_string(),
            });
        }
        if transaction
            .open_table(VECTOR_ACTIVE_GENERATIONS)
            .map_err(storage_error)?
            .get(index_name.as_bytes())
            .map_err(storage_error)?
            .is_some()
        {
            return Err(VectorRepositoryError::Storage(format!(
                "tombstoned vector index '{index_name}' is active"
            )));
        }
        let prefix = index_prefix(index_name)?;
        let generations_removed = {
            let mut metadata = transaction
                .open_table(VECTOR_GENERATION_METADATA)
                .map_err(storage_error)?;
            let keys = prefixed_table_keys(&metadata, &prefix)?;
            for key in &keys {
                metadata.remove(key.as_slice()).map_err(storage_error)?;
            }
            keys.len()
        };
        let records_removed = {
            let mut records = transaction
                .open_table(VECTOR_GENERATION_RECORDS)
                .map_err(storage_error)?;
            let keys = prefixed_table_keys(&records, &prefix)?;
            for key in &keys {
                records.remove(key.as_slice()).map_err(storage_error)?;
            }
            keys.len()
        };
        let nodes_removed = {
            let mut nodes = transaction
                .open_table(VECTOR_GENERATION_NODES)
                .map_err(storage_error)?;
            let keys = prefixed_table_keys(&nodes, &prefix)?;
            for key in &keys {
                nodes.remove(key.as_slice()).map_err(storage_error)?;
            }
            keys.len()
        };
        transaction
            .open_table(VECTOR_GENERATION_COUNTERS)
            .map_err(storage_error)?
            .remove(index_name.as_bytes())
            .map_err(storage_error)?;
        transaction
            .open_table(VECTOR_DROPPED_INDICES)
            .map_err(storage_error)?
            .remove(index_name.as_bytes())
            .map_err(storage_error)?;
        Ok(VectorGcReport {
            generations_removed,
            records_removed,
            nodes_removed,
        })
    }

    pub fn get_active_record(
        &self,
        index_name: &str,
        node_id: u64,
    ) -> Result<Option<VectorRecord>, VectorRepositoryError> {
        let Some(active) = self.active_generation(index_name)? else {
            return Ok(None);
        };
        let prefix = generation_prefix(index_name, active.generation)?;
        let key = node_key(&prefix, node_id);
        let transaction = self.database.begin_read().map_err(storage_error)?;
        let table = transaction
            .open_table(VECTOR_GENERATION_RECORDS)
            .map_err(storage_error)?;
        table
            .get(key.as_slice())
            .map_err(storage_error)?
            .map(|value| deserialize(value.value()))
            .transpose()
    }

    pub fn get_active_node(
        &self,
        index_name: &str,
        node_id: u64,
    ) -> Result<Option<HnswGenerationNode>, VectorRepositoryError> {
        let Some(active) = self.active_generation(index_name)? else {
            return Ok(None);
        };
        let prefix = generation_prefix(index_name, active.generation)?;
        let key = node_key(&prefix, node_id);
        let transaction = self.database.begin_read().map_err(storage_error)?;
        let table = transaction
            .open_table(VECTOR_GENERATION_NODES)
            .map_err(storage_error)?;
        table
            .get(key.as_slice())
            .map_err(storage_error)?
            .map(|value| deserialize(value.value()))
            .transpose()
    }

    pub fn active_records(
        &self,
        index_name: &str,
    ) -> Result<Vec<VectorRecord>, VectorRepositoryError> {
        let Some(active) = self.active_generation(index_name)? else {
            return Ok(Vec::new());
        };
        let prefix = generation_prefix(index_name, active.generation)?;
        let transaction = self.database.begin_read().map_err(storage_error)?;
        let table = transaction
            .open_table(VECTOR_GENERATION_RECORDS)
            .map_err(storage_error)?;
        let mut records = Vec::new();
        for entry in table.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if key.value().starts_with(&prefix) {
                records.push(deserialize(value.value())?);
            }
        }
        records.sort_by_key(|record: &VectorRecord| record.node_id);
        Ok(records)
    }

    /// Reconstruct the immutable search runtime from the active redb generation.
    pub fn load_active_hnsw(
        &self,
        index_name: &str,
    ) -> Result<Option<HnswGeneration>, VectorRepositoryError> {
        let Some(active) = self.active_generation(index_name)? else {
            return Ok(None);
        };
        let prefix = generation_prefix(index_name, active.generation)?;
        let transaction = self.database.begin_read().map_err(storage_error)?;
        let metadata_table = transaction
            .open_table(VECTOR_GENERATION_METADATA)
            .map_err(storage_error)?;
        let metadata = metadata_table
            .get(prefix.as_slice())
            .map_err(storage_error)?
            .ok_or_else(|| VectorRepositoryError::GenerationNotFound {
                index_name: index_name.to_string(),
                generation: active.generation,
            })?;
        let metadata: VectorGenerationMetadata = deserialize(metadata.value())?;

        let records_table = transaction
            .open_table(VECTOR_GENERATION_RECORDS)
            .map_err(storage_error)?;
        let mut vectors = HashMap::new();
        for entry in records_table.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if key.value().starts_with(&prefix) {
                let record: VectorRecord = deserialize(value.value())?;
                vectors.insert(record.node_id, record.vector);
            }
        }

        let nodes_table = transaction
            .open_table(VECTOR_GENERATION_NODES)
            .map_err(storage_error)?;
        let mut nodes = Vec::with_capacity(vectors.len());
        for entry in nodes_table.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if key.value().starts_with(&prefix) {
                let node: HnswGenerationNode = deserialize(value.value())?;
                let vector = vectors.remove(&node.node_id).ok_or_else(|| {
                    VectorRepositoryError::InvalidHnsw(format!(
                        "node {} has no canonical vector record",
                        node.node_id
                    ))
                })?;
                nodes.push(RuntimeHnswNode {
                    node_id: node.node_id,
                    vector,
                    level: node.level,
                    neighbors: node.neighbors,
                });
            }
        }
        if let Some(node_id) = vectors.keys().copied().min() {
            return Err(VectorRepositoryError::InvalidHnsw(format!(
                "vector record {node_id} has no HNSW node"
            )));
        }
        nodes.sort_by_key(|node| node.node_id);
        let generation = HnswGeneration {
            dimensions: metadata.dimensions,
            distance: metadata.distance.into(),
            params: HnswGenerationParams {
                m: metadata.m,
                m_max0: metadata.m_max0,
                ef_construction: metadata.ef_construction,
                ef_search: metadata.ef_search,
            },
            entry_point: metadata.entry_point,
            max_level: metadata.max_level,
            nodes,
        };
        generation
            .validate()
            .map_err(|error| VectorRepositoryError::InvalidHnsw(error.to_string()))?;
        Ok(Some(generation))
    }
}

impl From<DistanceFunction> for VectorDistance {
    fn from(value: DistanceFunction) -> Self {
        match value {
            DistanceFunction::Euclidean => Self::Euclidean,
            DistanceFunction::Cosine => Self::Cosine,
            DistanceFunction::DotProduct => Self::DotProduct,
        }
    }
}

impl From<VectorDistance> for DistanceFunction {
    fn from(value: VectorDistance) -> Self {
        match value {
            VectorDistance::Euclidean => Self::Euclidean,
            VectorDistance::Cosine => Self::Cosine,
            VectorDistance::DotProduct => Self::DotProduct,
        }
    }
}

fn generation_prefix(index_name: &str, generation: u64) -> Result<Vec<u8>, VectorRepositoryError> {
    let mut key = index_prefix(index_name)?;
    key.reserve(8);
    key.extend_from_slice(&generation.to_be_bytes());
    Ok(key)
}

fn index_prefix(index_name: &str) -> Result<Vec<u8>, VectorRepositoryError> {
    let name_len = u32::try_from(index_name.len())
        .map_err(|_| VectorRepositoryError::Storage("vector index name is too long".to_string()))?;
    let mut key = Vec::with_capacity(4 + index_name.len());
    key.extend_from_slice(&name_len.to_be_bytes());
    key.extend_from_slice(index_name.as_bytes());
    Ok(key)
}

fn prefixed_table_keys(
    table: &redb::Table<&[u8], &[u8]>,
    prefix: &[u8],
) -> Result<Vec<Vec<u8>>, VectorRepositoryError> {
    table
        .range::<&[u8]>(prefix..)
        .map_err(storage_error)?
        .map(|entry| {
            let (key, _) = entry.map_err(storage_error)?;
            Ok(key.value().to_vec())
        })
        .take_while(|entry: &Result<Vec<u8>, VectorRepositoryError>| {
            entry
                .as_ref()
                .map(|key| key.starts_with(prefix))
                .unwrap_or(true)
        })
        .collect()
}

fn node_key(generation_prefix: &[u8], node_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(generation_prefix.len() + 8);
    key.extend_from_slice(generation_prefix);
    key.extend_from_slice(&node_id.to_be_bytes());
    key
}

fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, VectorRepositoryError> {
    zeta4j_codec::serialize(value)
        .map_err(|error| VectorRepositoryError::Serialization(error.to_string()))
}

fn deserialize<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, VectorRepositoryError> {
    zeta4j_codec::deserialize(bytes)
        .map_err(|error| VectorRepositoryError::Serialization(error.to_string()))
}

fn decode_u64(bytes: &[u8]) -> Result<u64, VectorRepositoryError> {
    let value: [u8; 8] = bytes.try_into().map_err(|_| {
        VectorRepositoryError::Storage("invalid vector generation counter encoding".to_string())
    })?;
    Ok(u64::from_le_bytes(value))
}

fn storage_error(error: impl std::fmt::Display) -> VectorRepositoryError {
    VectorRepositoryError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use zeta4j_common::WriteSurfaceSet;

    use super::*;
    use crate::server::tenant::{DatabaseContext, DatabaseContextError, UnifiedWriteSet};

    fn index_boundary() -> UnifiedWriteSet {
        UnifiedWriteSet::new(
            WriteSurfaceSet::new()
                .with_graph()
                .with_fulltext()
                .with_vector(),
        )
    }

    fn generation(index_name: &str, value: f32) -> VectorGenerationWriteSet {
        VectorGenerationWriteSet {
            index_name: index_name.to_string(),
            dimensions: 2,
            distance: VectorDistance::Cosine,
            entry_point: Some(1),
            max_level: 0,
            m: 16,
            m_max0: 32,
            ef_construction: 200,
            ef_search: 50,
            records: vec![VectorRecord {
                node_id: 1,
                entity: VectorEntityRef::GraphNode {
                    element_id: "1".to_string(),
                },
                vector: vec![value, 0.0],
                metadata_json: None,
            }],
            nodes: vec![HnswGenerationNode {
                node_id: 1,
                level: 0,
                neighbors: vec![vec![]],
            }],
        }
    }

    #[test]
    fn staged_generation_is_invisible_until_coordinator_activation()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let staged = context.vector().stage(&generation("knowledge", 1.0))?;
        assert!(context.vector().active_generation("knowledge")?.is_none());
        assert!(
            context
                .vector()
                .get_active_record("knowledge", 1)?
                .is_none()
        );

        let revision =
            context
                .coordinator()
                .commit(&index_boundary(), |transaction, revision| {
                    context
                        .vector()
                        .activate(transaction, &staged, revision)
                        .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                    Ok(revision)
                })?;
        assert_eq!(
            context.vector().active_generation("knowledge")?,
            Some(ActiveVectorGeneration {
                generation: staged.generation(),
                database_revision: revision,
            })
        );
        assert_eq!(
            context
                .vector()
                .get_active_record("knowledge", 1)?
                .map(|record| record.vector),
            Some(vec![1.0, 0.0])
        );

        drop(context);
        let reopened = DatabaseContext::open("alpha", temp.path())?;
        assert_eq!(
            reopened.vector().active_generation("knowledge")?,
            Some(ActiveVectorGeneration {
                generation: staged.generation(),
                database_revision: revision,
            })
        );
        assert!(reopened.vector().get_active_node("knowledge", 1)?.is_some());
        Ok(())
    }

    #[test]
    fn failed_coordinator_commit_keeps_previous_vector_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let first = context.vector().stage(&generation("knowledge", 1.0))?;
        context
            .coordinator()
            .commit(&index_boundary(), |transaction, revision| {
                context
                    .vector()
                    .activate(transaction, &first, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))
            })?;

        let second = context.vector().stage(&generation("knowledge", 2.0))?;
        let result: Result<(), DatabaseContextError> =
            context
                .coordinator()
                .commit(&index_boundary(), |transaction, revision| {
                    context
                        .vector()
                        .activate(transaction, &second, revision)
                        .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                    Err(DatabaseContextError::Apply(
                        "injected fulltext failure".to_string(),
                    ))
                });
        assert!(result.is_err());
        assert_eq!(
            context
                .vector()
                .get_active_record("knowledge", 1)?
                .map(|record| record.vector),
            Some(vec![1.0, 0.0])
        );
        Ok(())
    }

    #[test]
    fn pruning_removes_every_inactive_vector_generation() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let first = context.vector().stage(&generation("knowledge", 1.0))?;
        context
            .coordinator()
            .commit(&index_boundary(), |transaction, revision| {
                context
                    .vector()
                    .activate(transaction, &first, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))
            })?;
        let _abandoned = context.vector().stage(&generation("knowledge", 2.0))?;
        let active = context.vector().stage(&generation("knowledge", 3.0))?;

        let report = context
            .coordinator()
            .commit(&index_boundary(), |transaction, revision| {
                context
                    .vector()
                    .activate(transaction, &active, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                context
                    .vector()
                    .prune_inactive(transaction, "knowledge")
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))
            })?;

        assert_eq!(report.generations_removed, 2);
        assert_eq!(report.records_removed, 2);
        assert_eq!(report.nodes_removed, 2);
        assert_eq!(
            context
                .vector()
                .get_active_record("knowledge", 1)?
                .map(|record| record.vector),
            Some(vec![3.0, 0.0])
        );
        Ok(())
    }

    #[test]
    fn reopen_purges_every_generation_of_a_dropped_vector_index()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let first = context.vector().stage(&generation("knowledge", 1.0))?;
        context
            .coordinator()
            .commit(&index_boundary(), |transaction, revision| {
                context
                    .vector()
                    .activate(transaction, &first, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))
            })?;
        let _abandoned = context.vector().stage(&generation("knowledge", 2.0))?;
        context
            .coordinator()
            .commit(&index_boundary(), |transaction, _revision| {
                context
                    .vector()
                    .deactivate(transaction, "knowledge")
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))
            })?;
        assert_eq!(
            context.vector().list_dropped_indices()?,
            vec!["knowledge".to_string()]
        );
        drop(context);

        let reopened = DatabaseContext::open("alpha", temp.path())?;
        assert!(reopened.vector().list_dropped_indices()?.is_empty());
        assert!(reopened.vector().active_generation("knowledge")?.is_none());
        let recreated = reopened.vector().stage(&generation("knowledge", 3.0))?;
        assert_eq!(recreated.generation(), 1);
        Ok(())
    }

    #[test]
    fn rejects_asymmetric_hnsw_before_opening_staging_transaction()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let mut invalid = generation("knowledge", 1.0);
        invalid.records.push(VectorRecord {
            node_id: 2,
            entity: VectorEntityRef::GraphNode {
                element_id: "2".to_string(),
            },
            vector: vec![0.0, 1.0],
            metadata_json: None,
        });
        invalid.nodes[0].neighbors[0].push(2);
        invalid.nodes.push(HnswGenerationNode {
            node_id: 2,
            level: 0,
            neighbors: vec![vec![]],
        });
        assert!(matches!(
            context.vector().stage(&invalid),
            Err(VectorRepositoryError::AsymmetricEdge { .. })
        ));
        assert!(context.vector().active_generation("knowledge")?.is_none());
        Ok(())
    }

    #[test]
    fn deterministic_hnsw_round_trips_through_active_redb_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let hnsw = HnswGeneration::build(
            2,
            DistanceFunction::Euclidean,
            HnswGenerationParams::default(),
            [
                (1, vec![1.0, 0.0]),
                (2, vec![2.0, 0.0]),
                (3, vec![3.0, 0.0]),
            ],
        )?;
        let write_set = VectorGenerationWriteSet::from_hnsw_generation(
            "knowledge",
            hnsw,
            (1..=3).map(|node_id| VectorEntityBinding {
                node_id,
                entity: VectorEntityRef::GraphNode {
                    element_id: node_id.to_string(),
                },
                metadata_json: None,
            }),
        )?;
        let staged = context.vector().stage(&write_set)?;
        context
            .coordinator()
            .commit(&index_boundary(), |transaction, revision| {
                context
                    .vector()
                    .activate(transaction, &staged, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))
            })?;

        drop(context);
        let reopened = DatabaseContext::open("alpha", temp.path())?;
        let active = reopened
            .vector()
            .load_active_hnsw("knowledge")?
            .ok_or("active HNSW generation is missing")?;
        let results = active.search(&[2.1, 0.0], 2, None)?;
        assert_eq!(
            results.iter().map(|result| result.id).collect::<Vec<_>>(),
            vec![2, 3]
        );
        Ok(())
    }
}
