// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Logical-database Fulltext repository.

use std::sync::Arc;

use redb::{Database, TableDefinition, WriteTransaction};
use zeta4j_fulltext::{
    FulltextConfig, FulltextGcReport, FulltextGeneration, FulltextIndex, RedbDirectory,
    Result as FulltextResult,
};

const FULLTEXT_REVISIONS: TableDefinition<&str, u64> =
    TableDefinition::new("fulltext_database_revisions");

/// Stages and reads Fulltext generations in the logical database's unified redb.
/// It never starts the final visibility transaction itself.
#[derive(Clone)]
pub struct DatabaseFulltextRepository {
    database: Arc<Database>,
}

impl DatabaseFulltextRepository {
    pub(super) fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub(crate) fn initialize(&self, transaction: &WriteTransaction) -> std::io::Result<()> {
        transaction
            .open_table(FULLTEXT_REVISIONS)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(())
    }

    pub fn stage_new(
        &self,
        index_name: &str,
        config: FulltextConfig,
    ) -> FulltextResult<(FulltextIndex, FulltextGeneration)> {
        FulltextIndex::stage_new_in_redb(self.database.clone(), index_name.as_bytes(), config)
    }

    pub fn stage_from_active(
        &self,
        index_name: &str,
        config: FulltextConfig,
    ) -> FulltextResult<(FulltextIndex, FulltextGeneration)> {
        FulltextIndex::stage_from_active_in_redb(
            self.database.clone(),
            index_name.as_bytes(),
            config,
        )
    }

    pub fn open_active(
        &self,
        index_name: &str,
        config: FulltextConfig,
    ) -> FulltextResult<FulltextIndex> {
        FulltextIndex::open_in_redb(self.database.clone(), index_name.as_bytes(), config)
    }

    pub fn has_active(&self, index_name: &str) -> std::io::Result<bool> {
        RedbDirectory::active_generation(&self.database, index_name.as_bytes())
            .map(|value| value.is_some())
    }

    pub fn has_any_active(&self) -> std::io::Result<bool> {
        for index_name in self.list_indices()? {
            if self.has_active(&index_name)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn activate(
        &self,
        transaction: &WriteTransaction,
        generation: &FulltextGeneration,
        database_revision: u64,
    ) -> std::io::Result<()> {
        generation.activate(transaction)?;
        transaction
            .open_table(FULLTEXT_REVISIONS)
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .insert(
                String::from_utf8_lossy(generation.namespace()).as_ref(),
                database_revision,
            )
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(())
    }

    pub fn deactivate(
        &self,
        transaction: &WriteTransaction,
        index_name: &str,
    ) -> std::io::Result<()> {
        if !RedbDirectory::deactivate(transaction, index_name.as_bytes())? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("fulltext index '{index_name}' has no active generation"),
            ));
        }
        transaction
            .open_table(FULLTEXT_REVISIONS)
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .remove(index_name)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(())
    }

    pub fn active_revision(&self, index_name: &str) -> std::io::Result<Option<u64>> {
        let read = self
            .database
            .begin_read()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let table = read
            .open_table(FULLTEXT_REVISIONS)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        table
            .get(index_name)
            .map_err(|error| std::io::Error::other(error.to_string()))
            .map(|value| value.map(|value| value.value()))
    }

    pub fn prune_inactive(
        &self,
        transaction: &WriteTransaction,
        index_name: &str,
    ) -> std::io::Result<FulltextGcReport> {
        RedbDirectory::prune_inactive_generations(transaction, index_name.as_bytes())
    }

    pub fn list_indices(&self) -> std::io::Result<Vec<String>> {
        RedbDirectory::list_namespaces(&self.database)?
            .into_iter()
            .map(|namespace| {
                String::from_utf8(namespace).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                })
            })
            .collect()
    }

    pub fn list_dropped_indices(&self) -> std::io::Result<Vec<String>> {
        RedbDirectory::list_dropped_namespaces(&self.database)?
            .into_iter()
            .map(|namespace| {
                String::from_utf8(namespace).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                })
            })
            .collect()
    }

    pub fn prune_dropped(
        &self,
        transaction: &WriteTransaction,
        index_name: &str,
    ) -> std::io::Result<FulltextGcReport> {
        RedbDirectory::prune_dropped_namespace(transaction, index_name.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use zeta4j_common::{CanonicalEntityRef, WriteSurfaceSet};
    use zeta4j_fulltext::ChunkDocument;

    use super::*;
    use crate::server::tenant::{
        DatabaseContext, DatabaseContextError, HnswGenerationNode, UnifiedWriteSet, VectorDistance,
        VectorGenerationWriteSet, VectorRecord,
    };

    fn index_boundary() -> UnifiedWriteSet {
        UnifiedWriteSet::new(
            WriteSurfaceSet::new()
                .with_graph()
                .with_fulltext()
                .with_vector(),
        )
    }

    fn node_entity(element_id: &str) -> CanonicalEntityRef {
        CanonicalEntityRef::GraphNode {
            element_id: element_id.to_string(),
        }
    }

    fn vector_generation(value: f32) -> VectorGenerationWriteSet {
        VectorGenerationWriteSet {
            index_name: "knowledge".to_string(),
            dimensions: 2,
            distance: VectorDistance::Cosine,
            entry_point: Some(7),
            max_level: 0,
            m: 16,
            m_max0: 32,
            ef_construction: 200,
            ef_search: 50,
            records: vec![VectorRecord {
                node_id: 7,
                entity: node_entity("7"),
                vector: vec![value, 0.0],
                metadata_json: None,
            }],
            nodes: vec![HnswGenerationNode {
                node_id: 7,
                level: 0,
                neighbors: vec![vec![]],
            }],
        }
    }

    #[test]
    fn fulltext_and_vector_visibility_commit_together() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let config = FulltextConfig::default().without_auto_commit();
        let (staged, generation) = context.fulltext().stage_new("knowledge", config.clone())?;
        staged.writer().add_document(&ChunkDocument::new(
            7,
            "first generation",
            node_entity("7"),
        ))?;
        staged.commit()?;
        let vector_generation = context.vector().stage(&vector_generation(1.0))?;
        assert!(
            context
                .fulltext()
                .open_active("knowledge", config.clone())
                .is_err()
        );

        context
            .coordinator()
            .commit(&index_boundary(), |transaction, revision| {
                context
                    .fulltext()
                    .activate(transaction, &generation, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                context
                    .vector()
                    .activate(transaction, &vector_generation, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                Ok(())
            })?;

        let active = context
            .fulltext()
            .open_active("knowledge", config.clone())?;
        assert_eq!(active.reader().search_bm25("first", 10)?.len(), 1);
        assert_eq!(
            context
                .vector()
                .get_active_record("knowledge", 7)?
                .map(|record| record.vector),
            Some(vec![1.0, 0.0])
        );
        Ok(())
    }

    #[test]
    fn failed_commit_keeps_previous_fulltext_generation() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let context = DatabaseContext::open("alpha", temp.path())?;
        let config = FulltextConfig::default().without_auto_commit();
        let (first, first_generation) =
            context.fulltext().stage_new("knowledge", config.clone())?;
        first.writer().add_document(&ChunkDocument::new(
            7,
            "first generation",
            node_entity("7"),
        ))?;
        first.commit()?;
        let first_vector = context.vector().stage(&vector_generation(1.0))?;
        context
            .coordinator()
            .commit(&index_boundary(), |transaction, revision| {
                context
                    .fulltext()
                    .activate(transaction, &first_generation, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                context
                    .vector()
                    .activate(transaction, &first_vector, revision)
                    .map_err(|error| DatabaseContextError::Apply(error.to_string()))
            })?;

        let (second, second_generation) = context
            .fulltext()
            .stage_from_active("knowledge", config.clone())?;
        second.writer().add_document(&ChunkDocument::new(
            8,
            "second generation",
            node_entity("8"),
        ))?;
        second.commit()?;
        let second_vector = context.vector().stage(&vector_generation(2.0))?;
        let result: Result<(), DatabaseContextError> =
            context
                .coordinator()
                .commit(&index_boundary(), |transaction, revision| {
                    context
                        .fulltext()
                        .activate(transaction, &second_generation, revision)
                        .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                    context
                        .vector()
                        .activate(transaction, &second_vector, revision)
                        .map_err(|error| DatabaseContextError::Apply(error.to_string()))?;
                    Err(DatabaseContextError::Apply("injected failure".to_string()))
                });
        assert!(result.is_err());

        let active = context
            .fulltext()
            .open_active("knowledge", config.clone())?;
        assert_eq!(active.reader().search_bm25("first", 10)?.len(), 1);
        assert!(active.reader().search_bm25("second", 10)?.is_empty());
        assert_eq!(
            context
                .vector()
                .get_active_record("knowledge", 7)?
                .map(|record| record.vector),
            Some(vec![1.0, 0.0])
        );
        Ok(())
    }
}
