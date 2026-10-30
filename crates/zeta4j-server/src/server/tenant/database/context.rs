//! Logical database storage context and commit ownership.
//!
//! A context owns the single redb database that will become the canonical
//! persistence boundary for one logical Zeta4j database. Components prepare
//! changes independently, but only [`TransactionCoordinator`] may commit them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, MutexGuard};
use zeta4j_common::{CanonicalEntityRef, WriteSurfaceError, WriteSurfaceSet};
use zeta4j_fulltext::{ChunkDocument, FulltextConfig, FulltextGeneration};
use zeta4j_graph::core::{Edge, Node, PropertyContainer, Value};
use zeta4j_graph::engine::{
    CanonicalGraphStore, GraphEngine, GraphIndexDefinition, GraphIndexKind, GraphTransactionDelta,
    PreparedGraphCommit,
};
use zeta4j_graph::transaction::TransactionId;
use zeta4j_vector::{DistanceFunction, HnswGeneration, HnswGenerationParams};

use super::{StagedVectorGeneration, VectorEntityBinding, VectorGenerationWriteSet};

type MetadataTable = TableDefinition<'static, &'static [u8], &'static [u8]>;

const METADATA_TABLE: MetadataTable = TableDefinition::new("database_metadata");
const SCHEMA_VERSION_KEY: &[u8] = b"schema_version";
const TRANSACTION_REVISION_KEY: &[u8] = b"transaction_revision";
const SCHEMA_VERSION: u64 = 8;
const DATABASE_FILE: &str = "database.redb";

/// Errors produced by the logical database persistence boundary.
#[derive(Debug, Error)]
pub enum DatabaseContextError {
    /// The logical database directory could not be created.
    #[error("failed to create logical database directory {path}: {source}")]
    CreateDirectory {
        /// Directory path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// redb could not be opened or accessed.
    #[error("logical database storage error: {0}")]
    Storage(String),
    /// A prepared component change could not be applied.
    #[error("logical database transaction rejected: {0}")]
    Apply(String),
    /// The write set does not include the required graph surface.
    #[error(transparent)]
    InvalidWriteSet(#[from] WriteSurfaceError),
    /// The on-disk schema is not supported by this binary.
    #[error(
        "unsupported logical database schema version {found}; expected {expected}; \
         {remediation}",
        remediation = zeta4j_common::StateFormatError::remediation()
    )]
    UnsupportedSchema {
        /// Version found in redb.
        found: u64,
        /// Version supported by this binary.
        expected: u64,
    },
    /// An existing database has no schema identity and cannot be adopted.
    #[error(
        "logical database schema version is missing from existing state; \
         {remediation}",
        remediation = zeta4j_common::StateFormatError::remediation()
    )]
    MissingSchema,
    /// An existing database has a malformed schema identity.
    #[error(
        "invalid logical database schema version encoding; \
         {remediation}",
        remediation = zeta4j_common::StateFormatError::remediation()
    )]
    InvalidSchema,
}

/// The only owner allowed to start and finish logical database writes.
#[derive(Clone)]
pub struct TransactionCoordinator {
    database: Arc<Database>,
    snapshot_gate: Arc<StdMutex<()>>,
}

impl TransactionCoordinator {
    fn new(database: Arc<Database>) -> Self {
        Self {
            database,
            snapshot_gate: Arc::new(StdMutex::new(())),
        }
    }

    /// Apply all prepared component changes and commit them as one redb write.
    ///
    /// Dropping the transaction after an `apply` error aborts every change.
    fn commit_internal<T>(
        &self,
        apply: impl FnOnce(&WriteTransaction) -> Result<T, DatabaseContextError>,
    ) -> Result<T, DatabaseContextError> {
        let _snapshot_guard = self
            .snapshot_gate
            .lock()
            .map_err(|_| DatabaseContextError::Storage("snapshot gate is poisoned".to_string()))?;
        let transaction = self
            .database
            .begin_write()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        let output = apply(&transaction)?;
        transaction
            .commit()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        Ok(output)
    }

    /// Commit a validated write set.
    pub fn commit<T>(
        &self,
        write_set: &UnifiedWriteSet,
        apply: impl FnOnce(&WriteTransaction, u64) -> Result<T, DatabaseContextError>,
    ) -> Result<T, DatabaseContextError> {
        write_set.validate()?;
        self.commit_internal(|transaction| {
            let revision = next_transaction_revision(transaction)?;
            apply(transaction, revision)
        })
    }
}

/// Versioned, logical export of every canonical table in one database.
///
/// Keys and values retain their redb types so restore can replace all tables in
/// one write transaction without reopening the database or invalidating the
/// runtime GraphEngine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseSnapshot {
    version: u32,
    bytes_bytes: BTreeMap<String, Vec<(Vec<u8>, Vec<u8>)>>,
    u64_bytes: BTreeMap<String, Vec<(u64, Vec<u8>)>>,
    bytes_u64: BTreeMap<String, Vec<(Vec<u8>, u64)>>,
    str_bytes: BTreeMap<String, Vec<(String, Vec<u8>)>>,
    str_u64: BTreeMap<String, Vec<(String, u64)>>,
}

const DATABASE_SNAPSHOT_VERSION: u32 = 1;

const BYTES_BYTES_TABLES: &[&str] = &[
    "database_metadata",
    "vector_generation_counters",
    "vector_generation_metadata",
    "vector_generation_records",
    "vector_generation_nodes",
    "vector_active_generations",
    "vector_dropped_indices",
    "fulltext_blobs",
    "fulltext_generation_files",
    "fulltext_dropped_namespaces",
];
const U64_BYTES_TABLES: &[&str] = &["graph_nodes", "graph_edges"];
const BYTES_U64_TABLES: &[&str] = &[
    "graph_label_index",
    "graph_relation_index",
    "graph_adjacency_index",
    "fulltext_active_generations",
    "fulltext_generation_counters",
];
const STR_BYTES_TABLES: &[&str] = &["graph_index_definitions"];
const STR_U64_TABLES: &[&str] = &["graph_metadata", "fulltext_database_revisions"];

impl DatabaseSnapshot {
    fn validate(&self) -> Result<(), DatabaseContextError> {
        if self.version != DATABASE_SNAPSHOT_VERSION {
            return Err(invalid_snapshot(format!(
                "unsupported version {}; expected {}",
                self.version, DATABASE_SNAPSHOT_VERSION
            )));
        }
        validate_table_set(&self.bytes_bytes, BYTES_BYTES_TABLES)?;
        validate_table_set(&self.u64_bytes, U64_BYTES_TABLES)?;
        validate_table_set(&self.bytes_u64, BYTES_U64_TABLES)?;
        validate_table_set(&self.str_bytes, STR_BYTES_TABLES)?;
        validate_table_set(&self.str_u64, STR_U64_TABLES)?;
        for rows in self.bytes_bytes.values() {
            validate_unique_keys(rows.iter().map(|(key, _)| key.as_slice()))?;
        }
        for rows in self.u64_bytes.values() {
            validate_unique_keys(rows.iter().map(|(key, _)| *key))?;
        }
        for rows in self.bytes_u64.values() {
            validate_unique_keys(rows.iter().map(|(key, _)| key.as_slice()))?;
        }
        for rows in self.str_bytes.values() {
            validate_unique_keys(rows.iter().map(|(key, _)| key.as_str()))?;
        }
        for rows in self.str_u64.values() {
            validate_unique_keys(rows.iter().map(|(key, _)| key.as_str()))?;
        }
        self.validate_schema()?;
        self.validate_graph()
    }

    fn validate_schema(&self) -> Result<(), DatabaseContextError> {
        let metadata = self
            .bytes_bytes
            .get("database_metadata")
            .ok_or_else(|| invalid_snapshot("database metadata table is missing"))?;
        let encoded = metadata
            .iter()
            .find_map(|(key, value)| (key == SCHEMA_VERSION_KEY).then_some(value))
            .ok_or_else(|| invalid_snapshot("database schema version is missing"))?;
        let bytes: [u8; 8] = encoded
            .as_slice()
            .try_into()
            .map_err(|_| invalid_snapshot("database schema version encoding is invalid"))?;
        let found = u64::from_le_bytes(bytes);
        if found != SCHEMA_VERSION {
            return Err(invalid_snapshot(format!(
                "database schema version {found} does not match {SCHEMA_VERSION}"
            )));
        }
        Ok(())
    }

    fn validate_graph(&self) -> Result<(), DatabaseContextError> {
        let node_rows = self
            .u64_bytes
            .get("graph_nodes")
            .ok_or_else(|| invalid_snapshot("graph node table is missing"))?;
        let mut node_ids = BTreeSet::new();
        for (key, encoded) in node_rows {
            let node: Node = zeta4j_codec::deserialize(encoded)
                .map_err(|error| invalid_snapshot(format!("invalid graph node {key}: {error}")))?;
            if node.id() != *key {
                return Err(invalid_snapshot(format!(
                    "graph node key {key} does not match record ID {}",
                    node.id()
                )));
            }
            if *key == u64::MAX {
                return Err(invalid_snapshot("graph node ID space is exhausted"));
            }
            node_ids.insert(*key);
        }

        let edge_rows = self
            .u64_bytes
            .get("graph_edges")
            .ok_or_else(|| invalid_snapshot("graph edge table is missing"))?;
        for (key, encoded) in edge_rows {
            let edge: Edge = zeta4j_codec::deserialize(encoded)
                .map_err(|error| invalid_snapshot(format!("invalid graph edge {key}: {error}")))?;
            if edge.id() != *key {
                return Err(invalid_snapshot(format!(
                    "graph edge key {key} does not match record ID {}",
                    edge.id()
                )));
            }
            if *key == u64::MAX {
                return Err(invalid_snapshot("graph edge ID space is exhausted"));
            }
            if !node_ids.contains(&edge.from_node()) || !node_ids.contains(&edge.to_node()) {
                return Err(invalid_snapshot(format!(
                    "graph edge {key} references a missing endpoint"
                )));
            }
        }

        let definitions = self
            .str_bytes
            .get("graph_index_definitions")
            .ok_or_else(|| invalid_snapshot("graph index definition table is missing"))?;
        for (key, encoded) in definitions {
            let definition: GraphIndexDefinition =
                zeta4j_codec::deserialize(encoded).map_err(|error| {
                    invalid_snapshot(format!("invalid graph index definition '{key}': {error}"))
                })?;
            if definition.name != *key {
                return Err(invalid_snapshot(format!(
                    "graph index key '{key}' does not match definition name '{}'",
                    definition.name
                )));
            }
            definition.validate().map_err(|error| {
                invalid_snapshot(format!("invalid graph index definition '{key}': {error}"))
            })?;
        }
        Ok(())
    }
}

fn validate_table_set<T>(
    tables: &BTreeMap<String, T>,
    expected: &[&str],
) -> Result<(), DatabaseContextError> {
    let actual: BTreeSet<_> = tables.keys().map(String::as_str).collect();
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    if actual != expected {
        return Err(invalid_snapshot(format!(
            "table set does not match schema; found {actual:?}, expected {expected:?}"
        )));
    }
    Ok(())
}

fn validate_unique_keys<'a, T: Ord + 'a>(
    keys: impl Iterator<Item = T>,
) -> Result<(), DatabaseContextError> {
    let mut seen = BTreeSet::new();
    for key in keys {
        if !seen.insert(key) {
            return Err(invalid_snapshot("table contains duplicate keys"));
        }
    }
    Ok(())
}

fn invalid_snapshot(message: impl Into<String>) -> DatabaseContextError {
    DatabaseContextError::Storage(format!("invalid database snapshot: {}", message.into()))
}

/// Prepared component boundary for one atomic logical database commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnifiedWriteSet {
    surfaces: WriteSurfaceSet,
}

impl UnifiedWriteSet {
    /// Create a write set. Validation is repeated at commit to keep the invariant
    /// at the transaction boundary even when preparation and commit are separated.
    pub const fn new(surfaces: WriteSurfaceSet) -> Self {
        Self { surfaces }
    }

    pub const fn surfaces(&self) -> WriteSurfaceSet {
        self.surfaces
    }

    pub fn validate(&self) -> Result<(), WriteSurfaceError> {
        self.surfaces.validate()
    }
}

/// Persistence context owned by one [`super::TenantDatabase`].
pub struct DatabaseContext {
    name: String,
    path: PathBuf,
    database: Arc<Database>,
    coordinator: TransactionCoordinator,
    graph: super::DatabaseGraphRepository,
    fulltext: super::DatabaseFulltextRepository,
    vector: super::DatabaseVectorRepository,
    index_maintenance_lock: Mutex<()>,
}

impl DatabaseContext {
    /// Open or create `{database_directory}/database.redb`.
    pub fn open(
        name: impl Into<String>,
        database_directory: impl AsRef<Path>,
    ) -> Result<Self, DatabaseContextError> {
        let directory = database_directory.as_ref();
        std::fs::create_dir_all(directory)
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;

        let path = directory.join(DATABASE_FILE);
        let is_new = !path.exists();
        let database = Arc::new(
            Database::create(&path)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?,
        );
        let coordinator = TransactionCoordinator::new(database.clone());
        let graph = super::DatabaseGraphRepository::new(database.clone());
        let fulltext = super::DatabaseFulltextRepository::new(database.clone());
        let vector = super::DatabaseVectorRepository::new(database.clone());
        let context = Self {
            name: name.into(),
            path,
            database,
            coordinator,
            graph,
            fulltext,
            vector,
            index_maintenance_lock: Mutex::new(()),
        };
        context.initialize_schema(is_new)?;
        context.prune_all_inactive_artifacts()?;
        Ok(context)
    }

    /// Capture every canonical table under the same coordinator exclusion
    /// boundary used by logical writes.
    pub fn snapshot(&self) -> Result<DatabaseSnapshot, DatabaseContextError> {
        let _snapshot_guard =
            self.coordinator.snapshot_gate.lock().map_err(|_| {
                DatabaseContextError::Storage("snapshot gate is poisoned".to_string())
            })?;
        let read = self
            .database
            .begin_read()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        let mut snapshot = DatabaseSnapshot {
            version: DATABASE_SNAPSHOT_VERSION,
            bytes_bytes: BTreeMap::new(),
            u64_bytes: BTreeMap::new(),
            bytes_u64: BTreeMap::new(),
            str_bytes: BTreeMap::new(),
            str_u64: BTreeMap::new(),
        };

        for &name in BYTES_BYTES_TABLES {
            let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(name);
            let rows = match read.open_table(definition) {
                Ok(table) => table
                    .iter()
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
                    .map(|row| {
                        let (key, value) =
                            row.map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                        Ok((key.value().to_vec(), value.value().to_vec()))
                    })
                    .collect::<Result<Vec<_>, DatabaseContextError>>()?,
                Err(redb::TableError::TableDoesNotExist(_)) => Vec::new(),
                Err(error) => return Err(DatabaseContextError::Storage(error.to_string())),
            };
            snapshot.bytes_bytes.insert(name.to_string(), rows);
        }
        for &name in U64_BYTES_TABLES {
            let definition: TableDefinition<u64, &[u8]> = TableDefinition::new(name);
            let rows = match read.open_table(definition) {
                Ok(table) => table
                    .iter()
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
                    .map(|row| {
                        let (key, value) =
                            row.map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                        Ok((key.value(), value.value().to_vec()))
                    })
                    .collect::<Result<Vec<_>, DatabaseContextError>>()?,
                Err(redb::TableError::TableDoesNotExist(_)) => Vec::new(),
                Err(error) => return Err(DatabaseContextError::Storage(error.to_string())),
            };
            snapshot.u64_bytes.insert(name.to_string(), rows);
        }
        for &name in BYTES_U64_TABLES {
            let definition: TableDefinition<&[u8], u64> = TableDefinition::new(name);
            let rows = match read.open_table(definition) {
                Ok(table) => table
                    .iter()
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
                    .map(|row| {
                        let (key, value) =
                            row.map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                        Ok((key.value().to_vec(), value.value()))
                    })
                    .collect::<Result<Vec<_>, DatabaseContextError>>()?,
                Err(redb::TableError::TableDoesNotExist(_)) => Vec::new(),
                Err(error) => return Err(DatabaseContextError::Storage(error.to_string())),
            };
            snapshot.bytes_u64.insert(name.to_string(), rows);
        }
        for &name in STR_BYTES_TABLES {
            let definition: TableDefinition<&str, &[u8]> = TableDefinition::new(name);
            let rows = match read.open_table(definition) {
                Ok(table) => table
                    .iter()
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
                    .map(|row| {
                        let (key, value) =
                            row.map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                        Ok((key.value().to_string(), value.value().to_vec()))
                    })
                    .collect::<Result<Vec<_>, DatabaseContextError>>()?,
                Err(redb::TableError::TableDoesNotExist(_)) => Vec::new(),
                Err(error) => return Err(DatabaseContextError::Storage(error.to_string())),
            };
            snapshot.str_bytes.insert(name.to_string(), rows);
        }
        for &name in STR_U64_TABLES {
            let definition: TableDefinition<&str, u64> = TableDefinition::new(name);
            let rows = match read.open_table(definition) {
                Ok(table) => table
                    .iter()
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
                    .map(|row| {
                        let (key, value) =
                            row.map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                        Ok((key.value().to_string(), value.value()))
                    })
                    .collect::<Result<Vec<_>, DatabaseContextError>>()?,
                Err(redb::TableError::TableDoesNotExist(_)) => Vec::new(),
                Err(error) => return Err(DatabaseContextError::Storage(error.to_string())),
            };
            snapshot.str_u64.insert(name.to_string(), rows);
        }

        Ok(snapshot)
    }

    /// Atomically replace every canonical table from a validated snapshot.
    pub fn restore(&self, snapshot: &DatabaseSnapshot) -> Result<(), DatabaseContextError> {
        snapshot.validate()?;
        let _snapshot_guard =
            self.coordinator.snapshot_gate.lock().map_err(|_| {
                DatabaseContextError::Storage("snapshot gate is poisoned".to_string())
            })?;
        let transaction = self
            .database
            .begin_write()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;

        for &name in BYTES_BYTES_TABLES {
            let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(name);
            let mut table = transaction
                .open_table(definition)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            table
                .retain(|_, _| false)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            for (key, value) in snapshot.bytes_bytes.get(name).into_iter().flatten() {
                table
                    .insert(key.as_slice(), value.as_slice())
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            }
        }
        for &name in U64_BYTES_TABLES {
            let definition: TableDefinition<u64, &[u8]> = TableDefinition::new(name);
            let mut table = transaction
                .open_table(definition)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            table
                .retain(|_, _| false)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            for (key, value) in snapshot.u64_bytes.get(name).into_iter().flatten() {
                table
                    .insert(*key, value.as_slice())
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            }
        }
        for &name in BYTES_U64_TABLES {
            let definition: TableDefinition<&[u8], u64> = TableDefinition::new(name);
            let mut table = transaction
                .open_table(definition)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            table
                .retain(|_, _| false)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            for (key, value) in snapshot.bytes_u64.get(name).into_iter().flatten() {
                table
                    .insert(key.as_slice(), *value)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            }
        }
        for &name in STR_BYTES_TABLES {
            let definition: TableDefinition<&str, &[u8]> = TableDefinition::new(name);
            let mut table = transaction
                .open_table(definition)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            table
                .retain(|_, _| false)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            for (key, value) in snapshot.str_bytes.get(name).into_iter().flatten() {
                table
                    .insert(key.as_str(), value.as_slice())
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            }
        }
        for &name in STR_U64_TABLES {
            let definition: TableDefinition<&str, u64> = TableDefinition::new(name);
            let mut table = transaction
                .open_table(definition)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            table
                .retain(|_, _| false)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            for (key, value) in snapshot.str_u64.get(name).into_iter().flatten() {
                table
                    .insert(key.as_str(), *value)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            }
        }

        transaction
            .commit()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))
    }

    fn initialize_schema(&self, is_new: bool) -> Result<(), DatabaseContextError> {
        self.coordinator.commit_internal(|transaction| {
            let mut metadata = transaction
                .open_table(METADATA_TABLE)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;

            let stored = metadata
                .get(SCHEMA_VERSION_KEY)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
                .map(|value| {
                    let bytes: [u8; 8] = value
                        .value()
                        .try_into()
                        .map_err(|_| DatabaseContextError::InvalidSchema)?;
                    Ok::<u64, DatabaseContextError>(u64::from_le_bytes(bytes))
                })
                .transpose()?;

            match stored {
                Some(found) if found != SCHEMA_VERSION => {
                    return Err(DatabaseContextError::UnsupportedSchema {
                        found,
                        expected: SCHEMA_VERSION,
                    });
                }
                Some(_) => {}
                None if is_new => {
                    metadata
                        .insert(SCHEMA_VERSION_KEY, SCHEMA_VERSION.to_le_bytes().as_slice())
                        .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                }
                None => return Err(DatabaseContextError::MissingSchema),
            }
            drop(metadata);
            self.graph
                .initialize(transaction)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            self.fulltext
                .initialize(transaction)
                .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
            Ok(())
        })
    }

    /// Logical database name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Physical unified redb path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Transaction coordinator for this logical database.
    pub fn coordinator(&self) -> &TransactionCoordinator {
        &self.coordinator
    }

    /// Canonical property graph repository.
    pub fn graph(&self) -> &super::DatabaseGraphRepository {
        &self.graph
    }

    /// Serialize generation staging and visibility commit for this logical DB.
    /// Without this lease, concurrent sessions can clone the same active
    /// generation and the later activation can lose the earlier commit.
    pub async fn lock_index_maintenance(&self) -> MutexGuard<'_, ()> {
        self.index_maintenance_lock.lock().await
    }

    fn prune_all_inactive_artifacts(&self) -> Result<(), DatabaseContextError> {
        let fulltext_indices = self
            .fulltext
            .list_indices()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
            .into_iter()
            .collect::<BTreeSet<_>>();
        for index_name in fulltext_indices {
            self.coordinator.commit_internal(|transaction| {
                self.fulltext
                    .prune_inactive(transaction, &index_name)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                Ok(())
            })?;
        }
        let dropped_fulltext = self
            .fulltext
            .list_dropped_indices()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        for index_name in dropped_fulltext {
            self.coordinator.commit_internal(|transaction| {
                self.fulltext
                    .prune_dropped(transaction, &index_name)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                Ok(())
            })?;
        }
        let vector_indices = self
            .vector
            .list_active_indices()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        for index_name in vector_indices {
            self.coordinator.commit_internal(|transaction| {
                self.vector
                    .prune_inactive(transaction, &index_name)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                Ok(())
            })?;
        }
        let dropped_vectors = self
            .vector
            .list_dropped_indices()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        for index_name in dropped_vectors {
            self.coordinator.commit_internal(|transaction| {
                self.vector
                    .prune_dropped(transaction, &index_name)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                Ok(())
            })?;
        }
        Ok(())
    }

    /// Commit a prepared GraphEngine delta as a graph-only logical database write.
    pub fn commit_graph_delta(
        &self,
        delta: GraphTransactionDelta,
    ) -> Result<u64, DatabaseContextError> {
        let graph_write = super::GraphWriteSet::from(delta);
        let staged_fulltext = self.stage_graph_fulltext(&graph_write)?;
        let staged_vector = self.stage_graph_vector(&graph_write)?;
        let mut surfaces = WriteSurfaceSet::new().with_graph();
        if !staged_fulltext.is_empty() {
            surfaces = surfaces.with_fulltext();
        }
        if !staged_vector.is_empty() {
            surfaces = surfaces.with_vector();
        }
        let boundary = UnifiedWriteSet::new(surfaces);
        self.coordinator.commit(&boundary, |transaction, revision| {
            self.graph
                .apply(transaction, revision, &graph_write)
                .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            self.activate_graph_fulltext(transaction, revision, &staged_fulltext)?;
            self.activate_graph_vector(transaction, revision, &staged_vector)?;
            Ok(revision)
        })
    }

    /// Commit a prepared graph delta together with every other surface in one
    /// redb transaction, then publish or abort the GraphEngine projection.
    pub fn commit_prepared_graph<T>(
        &self,
        engine: &GraphEngine,
        prepared: PreparedGraphCommit,
        boundary: &UnifiedWriteSet,
        apply: impl FnOnce(&WriteTransaction, u64) -> Result<T, DatabaseContextError>,
    ) -> Result<(u64, T), DatabaseContextError> {
        let graph_write = super::GraphWriteSet::from(prepared.delta().clone());
        let staged_fulltext = match self.stage_graph_fulltext(&graph_write) {
            Ok(staged) => staged,
            Err(stage_error) => {
                engine.abort_transaction_commit(prepared);
                return Err(stage_error);
            }
        };
        let staged_vector = match self.stage_graph_vector(&graph_write) {
            Ok(staged) => staged,
            Err(stage_error) => {
                engine.abort_transaction_commit(prepared);
                return Err(stage_error);
            }
        };
        let mut effective_surfaces = boundary.surfaces();
        if !staged_fulltext.is_empty() {
            effective_surfaces = effective_surfaces.with_fulltext();
        }
        if !staged_vector.is_empty() {
            effective_surfaces = effective_surfaces.with_vector();
        }
        let effective_boundary = UnifiedWriteSet::new(effective_surfaces);
        let commit_result = if !boundary.surfaces().has_graph() {
            Err(DatabaseContextError::InvalidWriteSet(
                WriteSurfaceError::MissingGraph,
            ))
        } else {
            self.coordinator
                .commit(&effective_boundary, |transaction, revision| {
                    self.graph
                        .apply(transaction, revision, &graph_write)
                        .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                    self.activate_graph_fulltext(transaction, revision, &staged_fulltext)?;
                    self.activate_graph_vector(transaction, revision, &staged_vector)?;
                    let output = apply(transaction, revision)?;
                    Ok((revision, output))
                })
        };

        match commit_result {
            Ok(output) => {
                engine.complete_transaction_commit(prepared);
                Ok(output)
            }
            Err(commit_error) => {
                engine.abort_transaction_commit(prepared);
                Err(commit_error)
            }
        }
    }

    fn create_graph_index_definition(
        &self,
        definition: &GraphIndexDefinition,
    ) -> Result<u64, DatabaseContextError> {
        let staged_fulltext = if matches!(definition.kind, GraphIndexKind::Fulltext { .. }) {
            self.stage_graph_fulltext_definitions(
                &super::GraphWriteSet::default(),
                std::slice::from_ref(definition),
            )?
        } else {
            Vec::new()
        };
        let staged_vector = if matches!(definition.kind, GraphIndexKind::Vector { .. }) {
            self.stage_graph_vector_definitions(
                &super::GraphWriteSet::default(),
                std::slice::from_ref(definition),
            )?
        } else {
            Vec::new()
        };
        let mut surfaces = WriteSurfaceSet::new().with_graph();
        if !staged_fulltext.is_empty() {
            surfaces = surfaces.with_fulltext();
        }
        if !staged_vector.is_empty() {
            surfaces = surfaces.with_vector();
        }
        let boundary = UnifiedWriteSet::new(surfaces);
        self.coordinator.commit(&boundary, |transaction, revision| {
            self.graph
                .create_index_definition(transaction, revision, definition)
                .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            self.activate_graph_fulltext(transaction, revision, &staged_fulltext)?;
            self.activate_graph_vector(transaction, revision, &staged_vector)?;
            Ok(revision)
        })
    }

    fn drop_graph_index_definition(&self, name: &str) -> Result<u64, DatabaseContextError> {
        let definition = self
            .graph
            .index_definitions()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
            .into_iter()
            .find(|definition| definition.name == name)
            .ok_or_else(|| DatabaseContextError::Apply(format!("index '{name}' not found")))?;
        let is_fulltext = matches!(definition.kind, GraphIndexKind::Fulltext { .. });
        let is_vector = matches!(definition.kind, GraphIndexKind::Vector { .. });
        let mut surfaces = WriteSurfaceSet::new().with_graph();
        if is_fulltext {
            surfaces = surfaces.with_fulltext();
        }
        if is_vector {
            surfaces = surfaces.with_vector();
        }
        let boundary = UnifiedWriteSet::new(surfaces);
        self.coordinator.commit(&boundary, |transaction, revision| {
            self.graph
                .drop_index_definition(transaction, revision, name)
                .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            if is_fulltext {
                self.fulltext
                    .deactivate(transaction, name)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            }
            if is_vector {
                self.vector
                    .deactivate(transaction, name)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            }
            Ok(revision)
        })
    }

    fn stage_graph_fulltext(
        &self,
        graph_write: &super::GraphWriteSet,
    ) -> Result<Vec<FulltextGeneration>, DatabaseContextError> {
        let definitions = self
            .graph
            .index_definitions()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
            .into_iter()
            .filter(|definition| matches!(definition.kind, GraphIndexKind::Fulltext { .. }))
            .collect::<Vec<_>>();
        self.stage_graph_fulltext_definitions(graph_write, &definitions)
    }

    fn stage_graph_fulltext_definitions(
        &self,
        graph_write: &super::GraphWriteSet,
        definitions: &[GraphIndexDefinition],
    ) -> Result<Vec<FulltextGeneration>, DatabaseContextError> {
        if definitions.is_empty() {
            return Ok(Vec::new());
        }
        let nodes = self.post_graph_nodes(graph_write)?;

        let mut generations = Vec::with_capacity(definitions.len());
        for definition in definitions {
            let (index, generation) = self
                .fulltext
                .stage_new(
                    &definition.name,
                    FulltextConfig::default().without_auto_commit(),
                )
                .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            for node in nodes.values() {
                let Some(content) = graph_fulltext_content(node, definition)? else {
                    continue;
                };
                index
                    .writer()
                    .add_document(&ChunkDocument::new(
                        node.id(),
                        content,
                        CanonicalEntityRef::GraphNode {
                            element_id: node.id().to_string(),
                        },
                    ))
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            }
            index
                .commit()
                .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            generations.push(generation);
        }
        Ok(generations)
    }

    fn stage_graph_vector(
        &self,
        graph_write: &super::GraphWriteSet,
    ) -> Result<Vec<StagedVectorGeneration>, DatabaseContextError> {
        let definitions = self
            .graph
            .index_definitions()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
            .into_iter()
            .filter(|definition| matches!(definition.kind, GraphIndexKind::Vector { .. }))
            .collect::<Vec<_>>();
        self.stage_graph_vector_definitions(graph_write, &definitions)
    }

    fn stage_graph_vector_definitions(
        &self,
        graph_write: &super::GraphWriteSet,
        definitions: &[GraphIndexDefinition],
    ) -> Result<Vec<StagedVectorGeneration>, DatabaseContextError> {
        if definitions.is_empty() {
            return Ok(Vec::new());
        }
        let nodes = self.post_graph_nodes(graph_write)?;
        let mut staged = Vec::with_capacity(definitions.len());
        for definition in definitions {
            let GraphIndexKind::Vector {
                dimensions,
                ref distance,
            } = definition.kind
            else {
                return Err(DatabaseContextError::Apply(format!(
                    "index '{}' is not a vector definition",
                    definition.name
                )));
            };
            let property = &definition.properties[0];
            let mut vectors = Vec::new();
            let mut bindings = Vec::new();
            for node in nodes.values() {
                if !node.labels().contains(&definition.label) {
                    continue;
                }
                let Some(value) = node.get_property(property) else {
                    continue;
                };
                let Value::Vector(vector) = value else {
                    return Err(DatabaseContextError::Apply(format!(
                        "vector index '{}' requires vector property '{}.{}' on node {}",
                        definition.name,
                        definition.label,
                        property,
                        node.id()
                    )));
                };
                vectors.push((node.id(), vector.as_slice().to_vec()));
                bindings.push(VectorEntityBinding {
                    node_id: node.id(),
                    entity: CanonicalEntityRef::GraphNode {
                        element_id: node.id().to_string(),
                    },
                    metadata_json: None,
                });
            }
            let distance = graph_vector_distance(distance)?;
            let generation = HnswGeneration::build(
                dimensions,
                distance,
                HnswGenerationParams::default(),
                vectors,
            )
            .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            let write_set = VectorGenerationWriteSet::from_hnsw_generation(
                definition.name.clone(),
                generation,
                bindings,
            )
            .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
            staged.push(
                self.vector
                    .stage(&write_set)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))?,
            );
        }
        Ok(staged)
    }

    fn activate_graph_vector(
        &self,
        transaction: &WriteTransaction,
        revision: u64,
        generations: &[StagedVectorGeneration],
    ) -> Result<(), DatabaseContextError> {
        for generation in generations {
            self.vector
                .activate(transaction, generation, revision)
                .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
        }
        Ok(())
    }

    fn post_graph_nodes(
        &self,
        graph_write: &super::GraphWriteSet,
    ) -> Result<HashMap<u64, Node>, DatabaseContextError> {
        let mut nodes = self
            .graph
            .all_node_ids()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
            .into_iter()
            .map(|node_id| {
                self.graph
                    .node(node_id)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
                    .ok_or_else(|| {
                        DatabaseContextError::Storage(format!(
                            "canonical graph node {node_id} disappeared during semantic index staging"
                        ))
                    })
                    .map(|node| (node_id, node))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        for node_id in &graph_write.delete_node_ids {
            nodes.remove(node_id);
        }
        for node in &graph_write.upsert_nodes {
            nodes.insert(node.id(), node.clone());
        }
        Ok(nodes)
    }

    fn activate_graph_fulltext(
        &self,
        transaction: &WriteTransaction,
        revision: u64,
        generations: &[FulltextGeneration],
    ) -> Result<(), DatabaseContextError> {
        for generation in generations {
            self.fulltext
                .activate(transaction, generation, revision)
                .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
        }
        Ok(())
    }

    /// Prepare a live GraphEngine transaction, persist its canonical delta,
    /// then consume the prepared token to finalize or abort engine state.
    pub fn commit_graph_transaction(
        &self,
        engine: &GraphEngine,
        tx_id: TransactionId,
    ) -> Result<u64, DatabaseContextError> {
        let prepared = engine
            .prepare_transaction_commit(tx_id)
            .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;

        let boundary = UnifiedWriteSet::new(WriteSurfaceSet::new().with_graph());
        self.commit_prepared_graph(
            engine,
            prepared,
            &boundary,
            |_transaction, _revision| Ok(()),
        )
        .map(|(revision, ())| revision)
    }

    /// Fulltext staged-generation repository for this logical database.
    pub fn fulltext(&self) -> &super::DatabaseFulltextRepository {
        &self.fulltext
    }

    /// Deactivate one Fulltext generation as an atomic maintenance operation.
    /// Callers coordinating with index maintenance must hold [`Self::lock_index_maintenance`].
    pub fn deactivate_fulltext_index(&self, index_name: &str) -> Result<(), DatabaseContextError> {
        self.coordinator.commit_internal(|transaction| {
            self.fulltext
                .deactivate(transaction, index_name)
                .map_err(|error| DatabaseContextError::Apply(error.to_string()))
        })
    }

    /// Vector/HNSW staged-generation repository for this logical database.
    pub fn vector(&self) -> &super::DatabaseVectorRepository {
        &self.vector
    }

    /// Read the persisted logical database schema version.
    pub fn schema_version(&self) -> Result<u64, DatabaseContextError> {
        let transaction = self
            .database
            .begin_read()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        let metadata = transaction
            .open_table(METADATA_TABLE)
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        let value = metadata
            .get(SCHEMA_VERSION_KEY)
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
            .ok_or_else(|| {
                DatabaseContextError::Storage(
                    "logical database schema version is missing".to_string(),
                )
            })?;
        let bytes: [u8; 8] = value.value().try_into().map_err(|_| {
            DatabaseContextError::Storage(
                "invalid logical database schema version encoding".to_string(),
            )
        })?;
        Ok(u64::from_le_bytes(bytes))
    }
}

impl CanonicalGraphStore for DatabaseContext {
    fn node(&self, node_id: u64) -> zeta4j_graph::Result<Option<zeta4j_graph::core::Node>> {
        self.graph.node(node_id).map_err(graph_store_error)
    }

    fn edge(&self, edge_id: u64) -> zeta4j_graph::Result<Option<zeta4j_graph::core::Edge>> {
        self.graph.edge(edge_id).map_err(graph_store_error)
    }

    fn all_node_ids(&self) -> zeta4j_graph::Result<Vec<u64>> {
        self.graph.all_node_ids().map_err(graph_store_error)
    }

    fn all_edge_ids(&self) -> zeta4j_graph::Result<Vec<u64>> {
        self.graph.all_edge_ids().map_err(graph_store_error)
    }

    fn nodes_by_label(&self, label: &str) -> zeta4j_graph::Result<Vec<u64>> {
        self.graph.nodes_by_label(label).map_err(graph_store_error)
    }

    fn connected_edges(&self, node_id: u64) -> zeta4j_graph::Result<Vec<u64>> {
        self.graph
            .connected_edges(node_id)
            .map_err(graph_store_error)
    }

    fn index_definitions(&self) -> zeta4j_graph::Result<Vec<GraphIndexDefinition>> {
        self.graph.index_definitions().map_err(graph_store_error)
    }

    fn create_index_definition(
        &self,
        definition: GraphIndexDefinition,
    ) -> zeta4j_graph::Result<u64> {
        self.create_graph_index_definition(&definition)
            .map_err(graph_store_error)
    }

    fn drop_index_definition(&self, name: &str) -> zeta4j_graph::Result<u64> {
        self.drop_graph_index_definition(name)
            .map_err(graph_store_error)
    }

    fn commit_graph(&self, delta: GraphTransactionDelta) -> zeta4j_graph::Result<u64> {
        self.commit_graph_delta(delta).map_err(graph_store_error)
    }
}

fn graph_vector_distance(distance: &str) -> Result<DistanceFunction, DatabaseContextError> {
    match distance {
        "cosine" => Ok(DistanceFunction::Cosine),
        "euclidean" => Ok(DistanceFunction::Euclidean),
        "dot_product" => Ok(DistanceFunction::DotProduct),
        _ => Err(DatabaseContextError::Apply(format!(
            "unsupported graph vector distance '{distance}'"
        ))),
    }
}

fn graph_fulltext_content(
    node: &Node,
    definition: &GraphIndexDefinition,
) -> Result<Option<String>, DatabaseContextError> {
    if !node.labels().contains(&definition.label) {
        return Ok(None);
    }
    let mut values = Vec::new();
    for property in &definition.properties {
        match node.get_property(property) {
            Some(Value::String(value)) if !value.is_empty() => values.push(value.clone()),
            Some(Value::String(_)) | None => {}
            Some(_) => {
                return Err(DatabaseContextError::Apply(format!(
                    "fulltext index '{}' requires string property '{}.{}' on node {}",
                    definition.name,
                    definition.label,
                    property,
                    node.id()
                )));
            }
        }
    }
    if values.is_empty() {
        Ok(None)
    } else {
        Ok(Some(values.join("\n")))
    }
}

fn graph_store_error(error: impl std::fmt::Display) -> zeta4j_graph::GraphError {
    zeta4j_graph::GraphError::System {
        message: error.to_string(),
    }
}

fn next_transaction_revision(transaction: &WriteTransaction) -> Result<u64, DatabaseContextError> {
    let mut metadata = transaction
        .open_table(METADATA_TABLE)
        .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
    let current = metadata
        .get(TRANSACTION_REVISION_KEY)
        .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
        .map(|value| {
            let bytes: [u8; 8] = value.value().try_into().map_err(|_| {
                DatabaseContextError::Storage(
                    "invalid logical database transaction revision encoding".to_string(),
                )
            })?;
            Ok::<u64, DatabaseContextError>(u64::from_le_bytes(bytes))
        })
        .transpose()?
        .unwrap_or(0);
    let next = current.checked_add(1).ok_or_else(|| {
        DatabaseContextError::Storage("logical database transaction revision overflow".to_string())
    })?;
    metadata
        .insert(TRANSACTION_REVISION_KEY, next.to_le_bytes().as_slice())
        .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use redb::TableDefinition;
    use zeta4j_graph::core::VectorValue;
    use zeta4j_graph::engine::GraphEngineConfig;

    use super::*;

    const TEST_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("context_test");
    const CRASH_PHASE_ENV: &str = "ZETA4J_REDB_CRASH_TEST_PHASE";
    const CRASH_PATH_ENV: &str = "ZETA4J_REDB_CRASH_TEST_PATH";
    const CRASH_TEST_NAME: &str =
        "server::tenant::database::context::tests::process_crash_preserves_commit_boundary";
    const CRASH_EXIT_CODE: i32 = 86;

    fn transaction_revision(
        context: &DatabaseContext,
    ) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        let read = context.database.begin_read()?;
        let metadata = read.open_table(METADATA_TABLE)?;
        metadata
            .get(TRANSACTION_REVISION_KEY)?
            .map(|value| {
                let bytes: [u8; 8] = value.value().try_into()?;
                Ok(u64::from_le_bytes(bytes))
            })
            .transpose()
    }

    fn run_crash_child(phase: &str, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg(CRASH_TEST_NAME)
            .arg("--nocapture")
            .env(CRASH_PHASE_ENV, phase)
            .env(CRASH_PATH_ENV, path)
            .status()?;
        assert_eq!(status.code(), Some(CRASH_EXIT_CODE));
        Ok(())
    }

    fn exit_at_requested_commit_boundary() -> Result<bool, Box<dyn std::error::Error>> {
        let Ok(phase) = std::env::var(CRASH_PHASE_ENV) else {
            return Ok(false);
        };
        let path = std::env::var_os(CRASH_PATH_ENV)
            .map(PathBuf::from)
            .ok_or("crash test path is missing")?;
        let context = DatabaseContext::open("alpha", path)?;
        let graph = UnifiedWriteSet::new(WriteSurfaceSet::new().with_graph());
        let key = match phase.as_str() {
            "before" => b"before".as_slice(),
            "after" => b"after".as_slice(),
            _ => return Err(format!("unknown crash test phase '{phase}'").into()),
        };

        context
            .coordinator()
            .commit(&graph, |transaction, _revision| {
                let mut table = transaction
                    .open_table(TEST_TABLE)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                table
                    .insert(key, b"value".as_slice())
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                if phase == "before" {
                    std::process::exit(CRASH_EXIT_CODE);
                }
                Ok(())
            })?;
        std::process::exit(CRASH_EXIT_CODE);
    }

    #[test]
    fn context_is_scoped_to_one_logical_database_and_reopens()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path().join("alpha"))?;

        assert_eq!(context.name(), "alpha");
        assert_eq!(context.schema_version()?, SCHEMA_VERSION);
        assert!(context.path().ends_with(DATABASE_FILE));

        let write_set = UnifiedWriteSet::new(WriteSurfaceSet::new().with_graph());
        context
            .coordinator()
            .commit(&write_set, |transaction, _revision| {
                let mut table = transaction
                    .open_table(TEST_TABLE)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                table
                    .insert(b"key".as_slice(), b"value".as_slice())
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                Ok(())
            })?;
        drop(context);

        let reopened = DatabaseContext::open("alpha", temp.path().join("alpha"))?;
        let read = reopened
            .database
            .begin_read()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        let table = read
            .open_table(TEST_TABLE)
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        let value = table
            .get(b"key".as_slice())
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?
            .ok_or_else(|| DatabaseContextError::Apply("test value is missing".to_string()))?;
        assert_eq!(value.value(), b"value");
        Ok(())
    }

    #[test]
    fn existing_database_without_schema_identity_is_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let directory = temp.path().join("legacy");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(DATABASE_FILE);
        drop(Database::create(&path)?);

        let error = DatabaseContext::open("legacy", &directory)
            .err()
            .ok_or("existing database without schema identity was accepted")?;

        assert!(matches!(error, DatabaseContextError::MissingSchema));
        Ok(())
    }

    #[test]
    fn apply_error_aborts_every_surface_set_and_revision() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let graph = UnifiedWriteSet::new(WriteSurfaceSet::new().with_graph());
        context
            .coordinator()
            .commit(&graph, |transaction, _revision| {
                transaction
                    .open_table(TEST_TABLE)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                Ok(())
            })?;

        let surface_sets = [
            (graph, b"graph".as_slice()),
            (
                UnifiedWriteSet::new(
                    WriteSurfaceSet::new()
                        .with_graph()
                        .with_fulltext()
                        .with_vector(),
                ),
                b"graph_indexes".as_slice(),
            ),
        ];

        for (write_set, key) in surface_sets {
            let result: Result<(), DatabaseContextError> =
                context
                    .coordinator()
                    .commit(&write_set, |transaction, _revision| {
                        let mut table = transaction
                            .open_table(TEST_TABLE)
                            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                        table
                            .insert(key, b"value".as_slice())
                            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                        Err(DatabaseContextError::Apply("injected failure".to_string()))
                    });
            assert!(matches!(result, Err(DatabaseContextError::Apply(_))));
        }

        let read = context
            .database
            .begin_read()
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        let table = read
            .open_table(TEST_TABLE)
            .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
        for (_, key) in surface_sets {
            assert!(table.get(key)?.is_none());
        }
        drop(table);
        drop(read);

        let next_revision = context
            .coordinator()
            .commit(&graph, |_transaction, revision| Ok(revision))?;
        assert_eq!(next_revision, 2);
        Ok(())
    }

    #[test]
    fn process_crash_preserves_commit_boundary() -> Result<(), Box<dyn std::error::Error>> {
        if exit_at_requested_commit_boundary()? {
            return Ok(());
        }

        let temp = tempfile::tempdir()?;
        let graph = UnifiedWriteSet::new(WriteSurfaceSet::new().with_graph());
        let context = DatabaseContext::open("alpha", temp.path())?;
        let first_revision = context
            .coordinator()
            .commit(&graph, |transaction, revision| {
                transaction
                    .open_table(TEST_TABLE)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                Ok(revision)
            })?;
        assert_eq!(first_revision, 1);
        drop(context);

        run_crash_child("before", temp.path())?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        assert_eq!(transaction_revision(&context)?, Some(1));
        let read = context.database.begin_read()?;
        let table = read.open_table(TEST_TABLE)?;
        assert!(table.get(b"before".as_slice())?.is_none());
        drop(table);
        drop(read);
        let retry_revision = context
            .coordinator()
            .commit(&graph, |transaction, revision| {
                let mut table = transaction
                    .open_table(TEST_TABLE)
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                table
                    .insert(b"before".as_slice(), b"retried".as_slice())
                    .map_err(|error| DatabaseContextError::Storage(error.to_string()))?;
                Ok(revision)
            })?;
        assert_eq!(retry_revision, 2);
        drop(context);

        run_crash_child("after", temp.path())?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        assert_eq!(transaction_revision(&context)?, Some(3));
        let read = context.database.begin_read()?;
        let table = read.open_table(TEST_TABLE)?;
        assert_eq!(
            table
                .get(b"before".as_slice())?
                .ok_or("retried write is missing")?
                .value(),
            b"retried"
        );
        assert_eq!(
            table
                .get(b"after".as_slice())?
                .ok_or("post-commit write is missing")?
                .value(),
            b"value"
        );
        Ok(())
    }

    #[test]
    fn invalid_write_set_never_opens_a_redb_write() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let without_graph = UnifiedWriteSet::new(WriteSurfaceSet::new().with_fulltext());
        let apply_called = std::cell::Cell::new(false);

        let result: Result<(), DatabaseContextError> =
            context
                .coordinator()
                .commit(&without_graph, |_transaction, _revision| {
                    apply_called.set(true);
                    Ok(())
                });

        assert!(matches!(
            result,
            Err(DatabaseContextError::InvalidWriteSet(
                WriteSurfaceError::MissingGraph
            ))
        ));
        assert!(!apply_called.get());
        Ok(())
    }

    #[tokio::test]
    async fn prepared_graph_is_aborted_when_another_surface_rejects_the_commit()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path().join("canonical"))?;
        let engine = GraphEngine::with_temp_dir().await?;
        let tx_id = engine.begin_transaction()?;
        let node_id = engine
            .create_node_internal(tx_id, vec!["Entity".to_string()])
            .await?;
        let prepared = engine.prepare_transaction_commit(tx_id)?;
        let boundary = UnifiedWriteSet::new(WriteSurfaceSet::new().with_graph());

        let result: Result<(u64, ()), DatabaseContextError> = context.commit_prepared_graph(
            &engine,
            prepared,
            &boundary,
            |_transaction, _revision| {
                Err(DatabaseContextError::Apply(
                    "injected dependent surface failure".to_string(),
                ))
            },
        );

        assert!(matches!(result, Err(DatabaseContextError::Apply(_))));
        assert!(context.graph().node(node_id)?.is_none());
        assert!(context.graph().revision()?.is_none());
        assert!(engine.get_node(node_id).await?.is_none());
        assert!(engine.prepare_transaction_delta(tx_id).await.is_err());

        let invalid_tx = engine.begin_transaction()?;
        let invalid_node_id = engine
            .create_node_internal(invalid_tx, vec!["InvalidBoundary".to_string()])
            .await?;
        let invalid_prepared = engine.prepare_transaction_commit(invalid_tx)?;
        let indexes_only =
            UnifiedWriteSet::new(WriteSurfaceSet::new().with_fulltext().with_vector());
        let invalid_result = context.commit_prepared_graph(
            &engine,
            invalid_prepared,
            &indexes_only,
            |_transaction, _revision| Ok(()),
        );
        assert!(matches!(
            invalid_result,
            Err(DatabaseContextError::InvalidWriteSet(
                WriteSurfaceError::MissingGraph
            ))
        ));
        assert!(engine.get_node(invalid_node_id).await?.is_none());
        assert!(engine.prepare_transaction_delta(invalid_tx).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn graph_fulltext_generation_tracks_canonical_node_commits_and_reopens()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let canonical_path = temp.path().join("canonical");
        let context = Arc::new(DatabaseContext::open("alpha", &canonical_path)?);
        let engine = GraphEngine::new(
            GraphEngineConfig {
                data_path: temp.path().join("runtime").to_string_lossy().into_owned(),
                ..GraphEngineConfig::default()
            },
            context.clone(),
        )
        .await?;
        let node_id = engine.create_node(vec!["Person".to_string()]).await?;
        engine
            .update_node(node_id, |node| {
                node.set_property("name", Value::String("Ada Lovelace".to_string()));
                node.set_property("bio", Value::String("analytical engine".to_string()));
            })
            .await?;

        engine
            .create_fulltext_index(
                "person_text",
                "Person",
                &["name".to_string(), "bio".to_string()],
                "english",
            )
            .await?;
        let active = context.fulltext().open_active(
            "person_text",
            FulltextConfig::default().without_auto_commit(),
        )?;
        let initial = active.reader().search_bm25("analytical", 10)?;
        assert_eq!(initial.len(), 1);
        assert_eq!(
            initial[0].entity,
            CanonicalEntityRef::GraphNode {
                element_id: node_id.to_string()
            }
        );
        drop(active);
        assert_eq!(
            context.fulltext().active_revision("person_text")?,
            context.graph().revision()?
        );

        engine
            .update_node(node_id, |node| {
                node.set_property("name", Value::String("Grace Hopper".to_string()));
                node.set_property("bio", Value::String("compiler pioneer".to_string()));
            })
            .await?;
        let active = context.fulltext().open_active(
            "person_text",
            FulltextConfig::default().without_auto_commit(),
        )?;
        assert!(active.reader().search_bm25("analytical", 10)?.is_empty());
        assert_eq!(active.reader().search_bm25("compiler", 10)?.len(), 1);
        drop(active);

        engine
            .update_node(node_id, |node| {
                node.remove_label("Person");
                node.add_label("Engineer");
            })
            .await?;
        let active = context.fulltext().open_active(
            "person_text",
            FulltextConfig::default().without_auto_commit(),
        )?;
        assert!(active.reader().search_bm25("compiler", 10)?.is_empty());
        drop(active);
        engine
            .update_node(node_id, |node| {
                node.add_label("Person");
            })
            .await?;

        let invalid = engine
            .update_node(node_id, |node| {
                node.set_property("bio", Value::Integer(7));
            })
            .await;
        assert!(invalid.is_err());
        assert!(matches!(
            engine
                .get_node(node_id)
                .await?
                .and_then(|node| node.get_property("bio").cloned()),
            Some(Value::String(value)) if value == "compiler pioneer"
        ));
        let active = context.fulltext().open_active(
            "person_text",
            FulltextConfig::default().without_auto_commit(),
        )?;
        assert_eq!(active.reader().search_bm25("compiler", 10)?.len(), 1);
        drop(active);

        engine.shutdown().await?;
        drop(engine);
        drop(context);
        let reopened_context = Arc::new(DatabaseContext::open("alpha", &canonical_path)?);
        let reopened = GraphEngine::new(
            GraphEngineConfig {
                data_path: temp
                    .path()
                    .join("runtime-reopened")
                    .to_string_lossy()
                    .into_owned(),
                ..GraphEngineConfig::default()
            },
            reopened_context.clone(),
        )
        .await?;
        assert_eq!(reopened.list_fulltext_indexes()?.len(), 1);
        let active = reopened_context.fulltext().open_active(
            "person_text",
            FulltextConfig::default().without_auto_commit(),
        )?;
        assert_eq!(active.reader().search_bm25("compiler", 10)?.len(), 1);
        drop(active);

        reopened.delete_node(node_id).await?;
        let active = reopened_context.fulltext().open_active(
            "person_text",
            FulltextConfig::default().without_auto_commit(),
        )?;
        assert!(active.reader().search_bm25("compiler", 10)?.is_empty());
        drop(active);
        reopened.drop_fulltext_index("person_text")?;
        assert!(!reopened_context.fulltext().has_active("person_text")?);
        assert_eq!(
            reopened_context.fulltext().list_dropped_indices()?,
            vec!["person_text".to_string()]
        );
        drop(reopened);
        drop(reopened_context);
        let collected = DatabaseContext::open("alpha", &canonical_path)?;
        assert!(collected.fulltext().list_dropped_indices()?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn graph_vector_generation_tracks_canonical_node_commits_and_reopens()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let canonical_path = temp.path().join("canonical-vector");
        let context = Arc::new(DatabaseContext::open("alpha", &canonical_path)?);
        let engine = GraphEngine::new(
            GraphEngineConfig {
                data_path: temp
                    .path()
                    .join("runtime-vector")
                    .to_string_lossy()
                    .into_owned(),
                ..GraphEngineConfig::default()
            },
            context.clone(),
        )
        .await?;
        let node_id = engine.create_node(vec!["Person".to_string()]).await?;
        let initial_vector = VectorValue::new(vec![1.0, 0.0, 0.0])?;
        engine
            .update_node(node_id, |node| {
                node.set_property("embedding", Value::Vector(initial_vector));
            })
            .await?;

        engine
            .create_vector_index("person_embedding", "Person", "embedding", 3, "cosine")
            .await?;
        let active = context
            .vector()
            .active_generation("person_embedding")?
            .ok_or_else(|| DatabaseContextError::Apply("active vector missing".to_string()))?;
        assert_eq!(Some(active.database_revision), context.graph().revision()?);
        let records = context.vector().active_records("person_embedding")?;
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].entity,
            CanonicalEntityRef::GraphNode {
                element_id: node_id.to_string()
            }
        );
        assert_eq!(records[0].vector, vec![1.0, 0.0, 0.0]);

        let updated_vector = VectorValue::new(vec![0.0, 1.0, 0.0])?;
        engine
            .update_node(node_id, |node| {
                node.set_property("embedding", Value::Vector(updated_vector));
            })
            .await?;
        assert_eq!(
            context.vector().active_records("person_embedding")?[0].vector,
            vec![0.0, 1.0, 0.0]
        );

        engine
            .update_node(node_id, |node| {
                node.remove_label("Person");
            })
            .await?;
        assert!(
            context
                .vector()
                .active_records("person_embedding")?
                .is_empty()
        );
        engine
            .update_node(node_id, |node| {
                node.add_label("Person");
            })
            .await?;

        let invalid_vector = VectorValue::new(vec![1.0, 2.0])?;
        let invalid = engine
            .update_node(node_id, |node| {
                node.set_property("embedding", Value::Vector(invalid_vector));
            })
            .await;
        assert!(invalid.is_err());
        assert_eq!(
            context.vector().active_records("person_embedding")?[0].vector,
            vec![0.0, 1.0, 0.0]
        );

        drop(engine);
        drop(context);
        let reopened_context = Arc::new(DatabaseContext::open("alpha", &canonical_path)?);
        let reopened = GraphEngine::new(
            GraphEngineConfig {
                data_path: temp
                    .path()
                    .join("runtime-vector-reopened")
                    .to_string_lossy()
                    .into_owned(),
                ..GraphEngineConfig::default()
            },
            reopened_context.clone(),
        )
        .await?;
        assert_eq!(reopened.list_vector_indexes()?.len(), 1);
        let generation = reopened_context
            .vector()
            .load_active_hnsw("person_embedding")?
            .ok_or_else(|| DatabaseContextError::Apply("reopened HNSW missing".to_string()))?;
        let matched = generation.search(&[0.0, 1.0, 0.0], 1, None)?;
        assert_eq!(matched.first().map(|result| result.id), Some(node_id));

        reopened.delete_node(node_id).await?;
        assert!(
            reopened_context
                .vector()
                .active_records("person_embedding")?
                .is_empty()
        );
        reopened.drop_vector_index("person_embedding")?;
        assert!(
            reopened_context
                .vector()
                .active_generation("person_embedding")?
                .is_none()
        );
        assert_eq!(
            reopened_context.vector().list_dropped_indices()?,
            vec!["person_embedding".to_string()]
        );
        drop(reopened);
        drop(reopened_context);
        let collected = DatabaseContext::open("alpha", &canonical_path)?;
        assert!(collected.vector().list_dropped_indices()?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn database_snapshot_atomically_restores_canonical_graph_projection()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = Arc::new(DatabaseContext::open(
            "alpha",
            temp.path().join("canonical"),
        )?);
        let engine = GraphEngine::new(
            GraphEngineConfig {
                data_path: temp.path().join("runtime").to_string_lossy().into_owned(),
                ..GraphEngineConfig::default()
            },
            context.clone(),
        )
        .await?;

        let retained = engine.create_node(vec!["Retained".to_string()]).await?;
        let snapshot = context.snapshot()?;
        engine.delete_node(retained).await?;
        let removed = engine.create_node(vec!["Removed".to_string()]).await?;
        assert!(engine.get_node(removed).await?.is_some());

        context.restore(&snapshot)?;
        engine.reload_canonical_projection().await?;
        assert!(engine.get_node(retained).await?.is_some());
        assert!(engine.get_node(removed).await?.is_none());
        assert_eq!(context.snapshot()?, snapshot);
        Ok(())
    }

    #[test]
    fn database_snapshot_round_trips_every_canonical_table_kind()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let empty = context.snapshot()?;
        let transaction = context.database.begin_write()?;
        for &name in BYTES_BYTES_TABLES {
            let mut table = transaction.open_table(TableDefinition::<&[u8], &[u8]>::new(name))?;
            table.insert(b"key".as_slice(), name.as_bytes())?;
        }
        for &name in U64_BYTES_TABLES {
            let mut table = transaction.open_table(TableDefinition::<u64, &[u8]>::new(name))?;
            if name == "graph_nodes" {
                table.insert(41, zeta4j_codec::serialize(&Node::new(41))?.as_slice())?;
                table.insert(42, zeta4j_codec::serialize(&Node::new(42))?.as_slice())?;
            } else {
                table.insert(
                    41,
                    zeta4j_codec::serialize(&Edge::new(41, "RELATED", 41, 42))?.as_slice(),
                )?;
            }
        }
        for &name in BYTES_U64_TABLES {
            let mut table = transaction.open_table(TableDefinition::<&[u8], u64>::new(name))?;
            table.insert(b"key".as_slice(), 42)?;
        }
        for &name in STR_BYTES_TABLES {
            let mut table = transaction.open_table(TableDefinition::<&str, &[u8]>::new(name))?;
            if name == "graph_index_definitions" {
                let definition = GraphIndexDefinition {
                    name: "key".to_string(),
                    label: "Entity".to_string(),
                    properties: vec!["name".to_string()],
                    kind: GraphIndexKind::Property,
                };
                table.insert("key", zeta4j_codec::serialize(&definition)?.as_slice())?;
            } else {
                table.insert("key", name.as_bytes())?;
            }
        }
        for &name in STR_U64_TABLES {
            let mut table = transaction.open_table(TableDefinition::<&str, u64>::new(name))?;
            table.insert("key", 43)?;
        }
        transaction.commit()?;

        let populated = context.snapshot()?;
        assert_ne!(populated, empty);
        context.restore(&empty)?;
        assert_eq!(context.snapshot()?, empty);
        context.restore(&populated)?;
        assert_eq!(context.snapshot()?, populated);
        Ok(())
    }

    #[test]
    fn database_snapshot_rejects_invalid_graph_before_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let retained = context.snapshot()?;
        let mut invalid = retained.clone();
        let nodes = invalid
            .u64_bytes
            .get_mut("graph_nodes")
            .ok_or_else(|| DatabaseContextError::Apply("graph nodes missing".to_string()))?;
        nodes.push((7, zeta4j_codec::serialize(&Node::new(8))?));

        assert!(context.restore(&invalid).is_err());
        assert_eq!(context.snapshot()?, retained);
        Ok(())
    }
}
