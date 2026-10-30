//! Canonical property graph repository owned by one logical database.
//!
//! The repository never starts or commits a redb transaction. A graph write set
//! is validated and applied only inside the logical database coordinator.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};
use thiserror::Error;
use zeta4j_graph::core::{Edge, Node};
use zeta4j_graph::engine::{GraphIndexDefinition, GraphTransactionDelta};

type EntityTable = TableDefinition<'static, u64, &'static [u8]>;
type IndexTable = TableDefinition<'static, &'static [u8], u64>;
type MetadataTable = TableDefinition<'static, &'static str, u64>;
type DefinitionTable = TableDefinition<'static, &'static str, &'static [u8]>;

const GRAPH_NODES: EntityTable = TableDefinition::new("graph_nodes");
const GRAPH_EDGES: EntityTable = TableDefinition::new("graph_edges");
const GRAPH_LABEL_INDEX: IndexTable = TableDefinition::new("graph_label_index");
const GRAPH_RELATION_INDEX: IndexTable = TableDefinition::new("graph_relation_index");
const GRAPH_ADJACENCY_INDEX: IndexTable = TableDefinition::new("graph_adjacency_index");
const GRAPH_METADATA: MetadataTable = TableDefinition::new("graph_metadata");
const GRAPH_INDEX_DEFINITIONS: DefinitionTable = TableDefinition::new("graph_index_definitions");
const GRAPH_REVISION_KEY: &str = "revision";

/// A complete canonical graph delta prepared before opening a redb write.
#[derive(Debug, Clone, Default)]
pub struct GraphWriteSet {
    pub upsert_nodes: Vec<Node>,
    pub delete_node_ids: Vec<u64>,
    pub upsert_edges: Vec<Edge>,
    pub delete_edge_ids: Vec<u64>,
}

impl GraphWriteSet {
    pub fn is_empty(&self) -> bool {
        self.upsert_nodes.is_empty()
            && self.delete_node_ids.is_empty()
            && self.upsert_edges.is_empty()
            && self.delete_edge_ids.is_empty()
    }

    fn validate_ids(&self) -> Result<(), GraphRepositoryError> {
        validate_entity_ids(
            "node",
            self.upsert_nodes.iter().map(Node::id),
            &self.delete_node_ids,
        )?;
        validate_entity_ids(
            "edge",
            self.upsert_edges.iter().map(Edge::id),
            &self.delete_edge_ids,
        )
    }
}

impl From<GraphTransactionDelta> for GraphWriteSet {
    fn from(delta: GraphTransactionDelta) -> Self {
        Self {
            upsert_nodes: delta.upsert_nodes,
            delete_node_ids: delta.delete_node_ids,
            upsert_edges: delta.upsert_edges,
            delete_edge_ids: delta.delete_edge_ids,
        }
    }
}

#[derive(Debug, Error)]
pub enum GraphRepositoryError {
    #[error("graph storage error: {0}")]
    Storage(String),
    #[error("graph serialization error: {0}")]
    Serialization(String),
    #[error("duplicate {kind} id in graph write set: {id}")]
    DuplicateId { kind: &'static str, id: u64 },
    #[error("{kind} id is both upserted and deleted: {id}")]
    ConflictingMutation { kind: &'static str, id: u64 },
    #[error("edge {edge_id} references missing {endpoint} node {node_id}")]
    MissingEndpoint {
        edge_id: u64,
        endpoint: &'static str,
        node_id: u64,
    },
}

/// Canonical graph records and derived structural indexes.
#[derive(Clone)]
pub struct DatabaseGraphRepository {
    database: Arc<Database>,
}

impl DatabaseGraphRepository {
    pub(crate) fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub(crate) fn initialize(
        &self,
        transaction: &WriteTransaction,
    ) -> Result<(), GraphRepositoryError> {
        transaction.open_table(GRAPH_NODES).map_err(storage_error)?;
        transaction.open_table(GRAPH_EDGES).map_err(storage_error)?;
        transaction
            .open_table(GRAPH_LABEL_INDEX)
            .map_err(storage_error)?;
        transaction
            .open_table(GRAPH_RELATION_INDEX)
            .map_err(storage_error)?;
        transaction
            .open_table(GRAPH_ADJACENCY_INDEX)
            .map_err(storage_error)?;
        transaction
            .open_table(GRAPH_METADATA)
            .map_err(storage_error)?;
        transaction
            .open_table(GRAPH_INDEX_DEFINITIONS)
            .map_err(storage_error)?;
        Ok(())
    }

    /// Apply canonical records and every structural index in the caller's transaction.
    pub fn apply(
        &self,
        transaction: &WriteTransaction,
        revision: u64,
        write_set: &GraphWriteSet,
    ) -> Result<(), GraphRepositoryError> {
        write_set.validate_ids()?;

        let mut nodes = transaction.open_table(GRAPH_NODES).map_err(storage_error)?;
        let mut edges = transaction.open_table(GRAPH_EDGES).map_err(storage_error)?;

        let mut post_node_ids = HashSet::new();
        for entry in nodes.iter().map_err(storage_error)? {
            let (id, _) = entry.map_err(storage_error)?;
            post_node_ids.insert(id.value());
        }
        for id in &write_set.delete_node_ids {
            post_node_ids.remove(id);
        }
        post_node_ids.extend(write_set.upsert_nodes.iter().map(Node::id));

        let deleted_edge_ids: HashSet<u64> = write_set.delete_edge_ids.iter().copied().collect();
        let upsert_edges: HashMap<u64, &Edge> = write_set
            .upsert_edges
            .iter()
            .map(|edge| (edge.id(), edge))
            .collect();

        for entry in edges.iter().map_err(storage_error)? {
            let (id, value) = entry.map_err(storage_error)?;
            let edge_id = id.value();
            if deleted_edge_ids.contains(&edge_id) || upsert_edges.contains_key(&edge_id) {
                continue;
            }
            let edge: Edge = deserialize(value.value())?;
            validate_edge_endpoints(&edge, &post_node_ids)?;
        }
        for edge in &write_set.upsert_edges {
            validate_edge_endpoints(edge, &post_node_ids)?;
        }

        let mut labels = transaction
            .open_table(GRAPH_LABEL_INDEX)
            .map_err(storage_error)?;
        let mut relations = transaction
            .open_table(GRAPH_RELATION_INDEX)
            .map_err(storage_error)?;
        let mut adjacency = transaction
            .open_table(GRAPH_ADJACENCY_INDEX)
            .map_err(storage_error)?;

        for edge_id in &write_set.delete_edge_ids {
            if let Some(old) = edges.get(*edge_id).map_err(storage_error)? {
                let old: Edge = deserialize(old.value())?;
                remove_edge_indexes(&mut relations, &mut adjacency, &old)?;
            }
            edges.remove(*edge_id).map_err(storage_error)?;
        }
        for node_id in &write_set.delete_node_ids {
            if let Some(old) = nodes.get(*node_id).map_err(storage_error)? {
                let old: Node = deserialize(old.value())?;
                remove_node_indexes(&mut labels, &old)?;
            }
            nodes.remove(*node_id).map_err(storage_error)?;
        }
        for node in &write_set.upsert_nodes {
            if let Some(old) = nodes.get(node.id()).map_err(storage_error)? {
                let old: Node = deserialize(old.value())?;
                remove_node_indexes(&mut labels, &old)?;
            }
            let encoded = serialize(node)?;
            nodes
                .insert(node.id(), encoded.as_slice())
                .map_err(storage_error)?;
            insert_node_indexes(&mut labels, node)?;
        }
        for edge in &write_set.upsert_edges {
            if let Some(old) = edges.get(edge.id()).map_err(storage_error)? {
                let old: Edge = deserialize(old.value())?;
                remove_edge_indexes(&mut relations, &mut adjacency, &old)?;
            }
            let encoded = serialize(edge)?;
            edges
                .insert(edge.id(), encoded.as_slice())
                .map_err(storage_error)?;
            insert_edge_indexes(&mut relations, &mut adjacency, edge)?;
        }

        transaction
            .open_table(GRAPH_METADATA)
            .map_err(storage_error)?
            .insert(GRAPH_REVISION_KEY, revision)
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn node(&self, id: u64) -> Result<Option<Node>, GraphRepositoryError> {
        let read = self.database.begin_read().map_err(storage_error)?;
        let table = read.open_table(GRAPH_NODES).map_err(storage_error)?;
        table
            .get(id)
            .map_err(storage_error)?
            .map(|value| deserialize(value.value()))
            .transpose()
    }

    pub fn edge(&self, id: u64) -> Result<Option<Edge>, GraphRepositoryError> {
        let read = self.database.begin_read().map_err(storage_error)?;
        let table = read.open_table(GRAPH_EDGES).map_err(storage_error)?;
        table
            .get(id)
            .map_err(storage_error)?
            .map(|value| deserialize(value.value()))
            .transpose()
    }

    pub fn all_node_ids(&self) -> Result<Vec<u64>, GraphRepositoryError> {
        self.entity_ids(GRAPH_NODES)
    }

    pub fn all_edge_ids(&self) -> Result<Vec<u64>, GraphRepositoryError> {
        self.entity_ids(GRAPH_EDGES)
    }

    pub fn index_definitions(&self) -> Result<Vec<GraphIndexDefinition>, GraphRepositoryError> {
        let read = self.database.begin_read().map_err(storage_error)?;
        let table = read
            .open_table(GRAPH_INDEX_DEFINITIONS)
            .map_err(storage_error)?;
        let mut definitions = Vec::new();
        for entry in table.iter().map_err(storage_error)? {
            let (_, value) = entry.map_err(storage_error)?;
            definitions.push(deserialize(value.value())?);
        }
        Ok(definitions)
    }

    pub(crate) fn create_index_definition(
        &self,
        transaction: &WriteTransaction,
        revision: u64,
        definition: &GraphIndexDefinition,
    ) -> Result<(), GraphRepositoryError> {
        definition
            .validate()
            .map_err(|error| GraphRepositoryError::Storage(error.to_string()))?;
        let mut definitions = transaction
            .open_table(GRAPH_INDEX_DEFINITIONS)
            .map_err(storage_error)?;
        if definitions
            .get(definition.name.as_str())
            .map_err(storage_error)?
            .is_some()
        {
            return Err(GraphRepositoryError::Storage(format!(
                "index '{}' already exists",
                definition.name
            )));
        }
        let encoded = serialize(definition)?;
        definitions
            .insert(definition.name.as_str(), encoded.as_slice())
            .map_err(storage_error)?;
        transaction
            .open_table(GRAPH_METADATA)
            .map_err(storage_error)?
            .insert(GRAPH_REVISION_KEY, revision)
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn drop_index_definition(
        &self,
        transaction: &WriteTransaction,
        revision: u64,
        name: &str,
    ) -> Result<(), GraphRepositoryError> {
        let mut definitions = transaction
            .open_table(GRAPH_INDEX_DEFINITIONS)
            .map_err(storage_error)?;
        if definitions.remove(name).map_err(storage_error)?.is_none() {
            return Err(GraphRepositoryError::Storage(format!(
                "index '{name}' not found"
            )));
        }
        transaction
            .open_table(GRAPH_METADATA)
            .map_err(storage_error)?
            .insert(GRAPH_REVISION_KEY, revision)
            .map_err(storage_error)?;
        Ok(())
    }

    fn entity_ids(&self, definition: EntityTable) -> Result<Vec<u64>, GraphRepositoryError> {
        let read = self.database.begin_read().map_err(storage_error)?;
        let table = read.open_table(definition).map_err(storage_error)?;
        let mut ids = Vec::new();
        for entry in table.iter().map_err(storage_error)? {
            let (key, _) = entry.map_err(storage_error)?;
            ids.push(key.value());
        }
        Ok(ids)
    }

    pub fn revision(&self) -> Result<Option<u64>, GraphRepositoryError> {
        let read = self.database.begin_read().map_err(storage_error)?;
        let table = read.open_table(GRAPH_METADATA).map_err(storage_error)?;
        Ok(table
            .get(GRAPH_REVISION_KEY)
            .map_err(storage_error)?
            .map(|value| value.value()))
    }

    pub fn nodes_by_label(&self, label: &str) -> Result<Vec<u64>, GraphRepositoryError> {
        let read = self.database.begin_read().map_err(storage_error)?;
        let table = read.open_table(GRAPH_LABEL_INDEX).map_err(storage_error)?;
        read_text_index_ids(&table, label)
    }

    pub fn edges_by_relation(&self, relation: &str) -> Result<Vec<u64>, GraphRepositoryError> {
        let read = self.database.begin_read().map_err(storage_error)?;
        let table = read
            .open_table(GRAPH_RELATION_INDEX)
            .map_err(storage_error)?;
        read_text_index_ids(&table, relation)
    }

    pub fn connected_edges(&self, node_id: u64) -> Result<Vec<u64>, GraphRepositoryError> {
        let read = self.database.begin_read().map_err(storage_error)?;
        let table = read
            .open_table(GRAPH_ADJACENCY_INDEX)
            .map_err(storage_error)?;
        read_numeric_index_ids(&table, node_id)
    }
}

fn validate_entity_ids(
    kind: &'static str,
    upserts: impl Iterator<Item = u64>,
    deletes: &[u64],
) -> Result<(), GraphRepositoryError> {
    let mut upsert_ids = HashSet::new();
    for id in upserts {
        if !upsert_ids.insert(id) {
            return Err(GraphRepositoryError::DuplicateId { kind, id });
        }
    }
    let mut delete_ids = HashSet::new();
    for id in deletes {
        if !delete_ids.insert(*id) {
            return Err(GraphRepositoryError::DuplicateId { kind, id: *id });
        }
        if upsert_ids.contains(id) {
            return Err(GraphRepositoryError::ConflictingMutation { kind, id: *id });
        }
    }
    Ok(())
}

fn validate_edge_endpoints(
    edge: &Edge,
    node_ids: &HashSet<u64>,
) -> Result<(), GraphRepositoryError> {
    if !node_ids.contains(&edge.from_node()) {
        return Err(GraphRepositoryError::MissingEndpoint {
            edge_id: edge.id(),
            endpoint: "source",
            node_id: edge.from_node(),
        });
    }
    if !node_ids.contains(&edge.to_node()) {
        return Err(GraphRepositoryError::MissingEndpoint {
            edge_id: edge.id(),
            endpoint: "target",
            node_id: edge.to_node(),
        });
    }
    Ok(())
}

fn insert_node_indexes(
    labels: &mut redb::Table<&[u8], u64>,
    node: &Node,
) -> Result<(), GraphRepositoryError> {
    for label in node.labels() {
        let key = text_index_key(label, node.id());
        labels
            .insert(key.as_slice(), node.id())
            .map_err(storage_error)?;
    }
    Ok(())
}

fn remove_node_indexes(
    labels: &mut redb::Table<&[u8], u64>,
    node: &Node,
) -> Result<(), GraphRepositoryError> {
    for label in node.labels() {
        let key = text_index_key(label, node.id());
        labels.remove(key.as_slice()).map_err(storage_error)?;
    }
    Ok(())
}

fn insert_edge_indexes(
    relations: &mut redb::Table<&[u8], u64>,
    adjacency: &mut redb::Table<&[u8], u64>,
    edge: &Edge,
) -> Result<(), GraphRepositoryError> {
    let relation_key = text_index_key(edge.relation_type(), edge.id());
    relations
        .insert(relation_key.as_slice(), edge.id())
        .map_err(storage_error)?;
    for node_id in [edge.from_node(), edge.to_node()] {
        let key = numeric_index_key(node_id, edge.id());
        adjacency
            .insert(key.as_slice(), edge.id())
            .map_err(storage_error)?;
    }
    Ok(())
}

fn remove_edge_indexes(
    relations: &mut redb::Table<&[u8], u64>,
    adjacency: &mut redb::Table<&[u8], u64>,
    edge: &Edge,
) -> Result<(), GraphRepositoryError> {
    let relation_key = text_index_key(edge.relation_type(), edge.id());
    relations
        .remove(relation_key.as_slice())
        .map_err(storage_error)?;
    for node_id in [edge.from_node(), edge.to_node()] {
        let key = numeric_index_key(node_id, edge.id());
        adjacency.remove(key.as_slice()).map_err(storage_error)?;
    }
    Ok(())
}

fn text_index_key(value: &str, id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(value.len() + 9);
    key.extend_from_slice(value.as_bytes());
    key.push(0);
    key.extend_from_slice(&id.to_be_bytes());
    key
}

fn numeric_index_key(value: u64, id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(16);
    key.extend_from_slice(&value.to_be_bytes());
    key.extend_from_slice(&id.to_be_bytes());
    key
}

fn read_text_index_ids(
    table: &impl ReadableTable<&'static [u8], u64>,
    value: &str,
) -> Result<Vec<u64>, GraphRepositoryError> {
    let prefix = value.as_bytes();
    let mut ids = Vec::new();
    for entry in table.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let bytes = key.value();
        if bytes.len() > prefix.len() && bytes.starts_with(prefix) && bytes[prefix.len()] == 0 {
            ids.push(value.value());
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

fn read_numeric_index_ids(
    table: &impl ReadableTable<&'static [u8], u64>,
    value: u64,
) -> Result<Vec<u64>, GraphRepositoryError> {
    let prefix = value.to_be_bytes();
    let mut ids = Vec::new();
    for entry in table.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        if key.value().starts_with(&prefix) {
            ids.push(value.value());
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

fn serialize<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, GraphRepositoryError> {
    zeta4j_codec::serialize(value)
        .map_err(|error| GraphRepositoryError::Serialization(error.to_string()))
}

fn deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, GraphRepositoryError> {
    zeta4j_codec::deserialize(bytes)
        .map_err(|error| GraphRepositoryError::Serialization(error.to_string()))
}

fn storage_error(error: impl std::fmt::Display) -> GraphRepositoryError {
    GraphRepositoryError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zeta4j_common::WriteSurfaceSet;
    use zeta4j_graph::core::{Edge, Node, PropertyContainer, Value};
    use zeta4j_graph::engine::{GraphEngine, GraphEngineConfig, GraphTransactionDelta};
    use zeta4j_graph::index::FunctionExpression;

    use super::*;
    use crate::server::tenant::{DatabaseContext, DatabaseContextError, UnifiedWriteSet};

    fn graph_boundary() -> UnifiedWriteSet {
        UnifiedWriteSet::new(WriteSurfaceSet::new().with_graph())
    }

    #[test]
    fn graph_records_indexes_and_revision_commit_atomically()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let mut person = Node::new(1);
        person.add_label("Person");
        person.add_label("12345678");
        person.set_property("name", Value::String("Ada".to_string()));
        let mut company = Node::new(2);
        company.add_label("Company");
        company.add_label("12345678x");
        let edge = Edge::new(7, "WORKS_AT", 1, 2);
        let write_set = GraphWriteSet {
            upsert_nodes: vec![person.clone(), company],
            upsert_edges: vec![edge.clone()],
            ..Default::default()
        };

        context
            .coordinator()
            .commit(&graph_boundary(), |transaction, revision| {
                context
                    .graph()
                    .apply(transaction, revision, &write_set)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))
            })?;

        assert_eq!(context.graph().node(1)?, Some(person));
        assert_eq!(context.graph().edge(7)?, Some(edge));
        assert_eq!(context.graph().nodes_by_label("Person")?, vec![1]);
        assert_eq!(context.graph().nodes_by_label("12345678")?, vec![1]);
        assert_eq!(context.graph().edges_by_relation("WORKS_AT")?, vec![7]);
        assert_eq!(context.graph().connected_edges(2)?, vec![7]);
        assert_eq!(context.graph().revision()?, Some(1));

        drop(context);
        let reopened = DatabaseContext::open("alpha", temp.path())?;
        assert_eq!(reopened.graph().nodes_by_label("Person")?, vec![1]);
        assert_eq!(reopened.graph().revision()?, Some(1));
        Ok(())
    }

    #[test]
    fn dangling_edge_aborts_graph_and_revision() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let write_set = GraphWriteSet {
            upsert_nodes: vec![Node::new(1)],
            upsert_edges: vec![Edge::new(9, "BROKEN", 1, 2)],
            ..Default::default()
        };

        let result: Result<(), DatabaseContextError> =
            context
                .coordinator()
                .commit(&graph_boundary(), |transaction, revision| {
                    context
                        .graph()
                        .apply(transaction, revision, &write_set)
                        .map_err(|error| DatabaseContextError::Apply(error.to_string()))
                });
        assert!(matches!(result, Err(DatabaseContextError::Apply(_))));
        assert_eq!(context.graph().node(1)?, None);
        assert_eq!(context.graph().revision()?, None);
        Ok(())
    }

    #[test]
    fn database_context_commits_frozen_graph_delta() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let mut node = Node::new(11);
        node.add_label("Entity");
        let revision = context.commit_graph_delta(GraphTransactionDelta {
            upsert_nodes: vec![node.clone()],
            ..Default::default()
        })?;

        assert_eq!(revision, 1);
        assert_eq!(context.graph().node(11)?, Some(node));
        assert_eq!(context.graph().revision()?, Some(revision));
        Ok(())
    }

    #[tokio::test]
    async fn database_context_consumes_prepared_graph_transaction()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path().join("canonical"))?;
        let engine = GraphEngine::with_temp_dir().await?;
        let tx_id = engine.begin_transaction()?;
        let node_id = engine
            .create_node_internal(tx_id, vec!["Entity".to_string()])
            .await?;

        let revision = context.commit_graph_transaction(&engine, tx_id)?;

        assert_eq!(revision, 1);
        assert_eq!(
            context.graph().node(node_id)?.map(|node| node.id()),
            Some(node_id)
        );
        assert!(engine.prepare_transaction_delta(tx_id).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn graph_engine_commits_and_reopens_through_database_context()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let database_path = temp.path().join("database");
        let engine_path = temp.path().join("engine");
        let context = Arc::new(DatabaseContext::open("alpha", &database_path)?);
        let config = GraphEngineConfig {
            data_path: engine_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let engine = GraphEngine::new(config.clone(), context.clone()).await?;

        let node_id = engine.create_node(vec!["Entity".to_string()]).await?;
        assert_eq!(
            engine.get_node(node_id).await?.map(|node| node.id()),
            Some(node_id)
        );
        assert_eq!(context.graph().nodes_by_label("Entity")?, vec![node_id]);
        drop(engine);
        drop(context);

        let reopened = Arc::new(DatabaseContext::open("alpha", &database_path)?);
        let engine = GraphEngine::new(config, reopened).await?;
        assert_eq!(
            engine.get_node(node_id).await?.map(|node| node.id()),
            Some(node_id)
        );
        assert_eq!(
            engine.create_node(vec!["Entity".to_string()]).await?,
            node_id + 1
        );
        Ok(())
    }

    #[tokio::test]
    async fn graph_index_definitions_reopen_as_runtime_projections()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let database_path = temp.path().join("canonical");
        let context = Arc::new(DatabaseContext::open("alpha", &database_path)?);
        let config = GraphEngineConfig {
            data_path: temp.path().join("runtime").to_string_lossy().into_owned(),
            ..GraphEngineConfig::default()
        };
        let engine = GraphEngine::new(config.clone(), context.clone()).await?;
        let node_id = engine.create_node(vec!["Person".to_string()]).await?;
        engine
            .update_node(node_id, |node| {
                node.set_property("name", Value::String("Ada".to_string()));
                node.set_property("family", Value::String("Lovelace".to_string()));
            })
            .await?;
        engine
            .create_property_index("person_name", "Person", &["name".to_string()])
            .await?;
        engine
            .create_property_index(
                "person_identity",
                "Person",
                &["name".to_string(), "family".to_string()],
            )
            .await?;
        engine.create_spatial_index("Person", "location").await?;
        engine
            .create_function_index(
                "person_lower_name",
                "Person",
                FunctionExpression::Lower("name".to_string()),
            )
            .await?;
        assert_eq!(
            engine.find_nodes_by_index("Person", "name", &Value::String("Ada".to_string())),
            Some(vec![node_id])
        );
        engine
            .update_node(node_id, |node| {
                node.set_property("name", Value::String("Grace".to_string()));
            })
            .await?;
        assert_eq!(
            engine.find_nodes_by_index("Person", "name", &Value::String("Ada".to_string())),
            Some(Vec::new())
        );
        assert_eq!(
            engine.find_nodes_by_index("Person", "name", &Value::String("Grace".to_string())),
            Some(vec![node_id])
        );

        drop(engine);
        drop(context);
        let reopened_context = Arc::new(DatabaseContext::open("alpha", &database_path)?);
        let reopened = GraphEngine::new(config, reopened_context).await?;
        assert_eq!(reopened.list_property_indexes().len(), 2);
        assert!(reopened.has_spatial_index("Person", "location"));
        assert!(reopened.has_function_index("person_lower_name"));
        assert_eq!(
            reopened.find_nodes_by_index("Person", "name", &Value::String("Grace".to_string())),
            Some(vec![node_id])
        );
        assert_eq!(
            reopened.find_nodes_by_function_index(
                "Person",
                &FunctionExpression::Lower("name".to_string()),
                &Value::String("grace".to_string())
            ),
            Some(vec![node_id])
        );
        Ok(())
    }
}
