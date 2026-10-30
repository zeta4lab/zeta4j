//! Cypher Vector procedure provider backed by logical-database redb generations.

use std::sync::Arc;

use zeta4j_common::CanonicalEntityRef;
use zeta4j_cypher_executor::procedures::{
    ProcedureError, VectorSearchProvider, VectorSearchResult,
};
use zeta4j_graph::engine::GraphEngine;
use zeta4j_vector::DistanceFunction;

use crate::server::tenant::{DatabaseContext, TenantDatabaseManager};

pub struct DatabaseVectorSearchProvider {
    database_manager: Arc<TenantDatabaseManager>,
}

impl DatabaseVectorSearchProvider {
    pub fn new(database_manager: Arc<TenantDatabaseManager>) -> Self {
        Self { database_manager }
    }

    fn context(&self, engine: &GraphEngine) -> Result<Arc<DatabaseContext>, ProcedureError> {
        self.database_manager
            .context_for_engine(engine)
            .ok_or_else(|| {
                ProcedureError::Internal(
                    "query GraphEngine is not attached to a unified logical database context"
                        .to_string(),
                )
            })
    }
}

impl VectorSearchProvider for DatabaseVectorSearchProvider {
    fn query_nodes(
        &self,
        engine: &GraphEngine,
        index_name: &str,
        k: usize,
        query: &[f32],
    ) -> Result<Vec<VectorSearchResult>, ProcedureError> {
        let context = self.context(engine)?;
        let generation = context
            .vector()
            .load_active_hnsw(index_name)
            .map_err(repository_error)?
            .ok_or_else(|| ProcedureError::IndexNotFound(index_name.to_string()))?;
        let distance = generation.distance;
        let matches = generation
            .search(query, k, None)
            .map_err(|error| ProcedureError::Execution(error.to_string()))?;
        matches
            .into_iter()
            .map(|matched| {
                let record = context
                    .vector()
                    .get_active_record(index_name, matched.id)
                    .map_err(repository_error)?
                    .ok_or_else(|| {
                        ProcedureError::Execution(format!(
                            "vector index '{index_name}' has no canonical record for HNSW node {}",
                            matched.id
                        ))
                    })?;
                let CanonicalEntityRef::GraphNode { element_id } = record.entity;
                let entity_id = element_id.parse::<u64>().map_err(|error| {
                    ProcedureError::Execution(format!(
                        "vector graph element_id '{element_id}' is not a canonical node ID: {error}"
                    ))
                })?;
                context
                    .graph()
                    .node(entity_id)
                    .map_err(|error| ProcedureError::Execution(error.to_string()))?
                    .ok_or_else(|| {
                        ProcedureError::Execution(format!(
                            "vector index '{index_name}' references missing canonical graph node {entity_id}"
                        ))
                    })?;
                Ok(VectorSearchResult {
                    entity_id,
                    score: similarity_score(distance, matched.distance),
                })
            })
            .collect()
    }
}

fn similarity_score(distance: DistanceFunction, value: f32) -> f32 {
    match distance {
        DistanceFunction::Cosine => 1.0 - value,
        DistanceFunction::Euclidean => 1.0 / (1.0 + value),
        DistanceFunction::DotProduct => -value,
    }
}

fn repository_error(error: impl std::fmt::Display) -> ProcedureError {
    ProcedureError::Execution(error.to_string())
}

pub type SharedVectorProvider = Arc<dyn VectorSearchProvider>;

#[cfg(test)]
mod tests {
    use zeta4j_graph::core::{PropertyContainer, Value, VectorValue};

    use super::*;
    use crate::server::tenant::TenantDatabaseManagerConfig;

    #[tokio::test]
    async fn query_nodes_returns_the_canonical_graph_node() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let manager = Arc::new(
            TenantDatabaseManager::new_with_storage(
                TenantDatabaseManagerConfig::default().with_data_path(temp.path()),
            )
            .await?,
        );
        let engine = manager
            .default_engine()
            .await
            .ok_or_else(|| std::io::Error::other("default engine missing"))?;
        let node_id = engine.create_node(vec!["Person".to_string()]).await?;
        let embedding = VectorValue::new(vec![1.0, 0.0, 0.0])?;
        engine
            .update_node(node_id, |node| {
                node.set_property("embedding", Value::Vector(embedding));
            })
            .await?;
        engine
            .create_vector_index("person_embedding", "Person", "embedding", 3, "cosine")
            .await?;

        let provider = DatabaseVectorSearchProvider::new(manager);
        let results = provider.query_nodes(&engine, "person_embedding", 1, &[1.0, 0.0, 0.0])?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, node_id);
        assert!((results[0].score - 1.0).abs() < f32::EPSILON);
        Ok(())
    }
}
