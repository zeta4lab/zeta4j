//! Cypher Fulltext procedure provider backed by logical-database redb generations.

use std::sync::Arc;

use zeta4j_common::CanonicalEntityRef;
use zeta4j_cypher_executor::procedures::{
    AnalyzerInfo, FulltextSearchProvider, FulltextSearchResult, ProcedureError,
};
use zeta4j_fulltext::FulltextConfig;
use zeta4j_graph::engine::GraphEngine;

use crate::server::tenant::{DatabaseContext, TenantDatabaseManager};

/// Resolves the active Fulltext generation belonging to the GraphEngine that
/// executes the current Cypher query.
pub struct DatabaseFulltextSearchProvider {
    database_manager: Arc<TenantDatabaseManager>,
}

impl DatabaseFulltextSearchProvider {
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

impl FulltextSearchProvider for DatabaseFulltextSearchProvider {
    fn query_nodes(
        &self,
        engine: &GraphEngine,
        index_name: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FulltextSearchResult>, ProcedureError> {
        let context = self.context(engine)?;
        if !context
            .fulltext()
            .has_active(index_name)
            .map_err(repository_error)?
        {
            return Err(ProcedureError::IndexNotFound(index_name.to_string()));
        }
        let index = context
            .fulltext()
            .open_active(index_name, FulltextConfig::default().without_auto_commit())
            .map_err(|error| ProcedureError::Execution(error.to_string()))?;
        let matches = index
            .reader()
            .search_bm25(query, limit)
            .map_err(|error| ProcedureError::Execution(error.to_string()))?;

        matches
            .into_iter()
            .map(|matched| {
                let CanonicalEntityRef::GraphNode { element_id } = matched.entity;
                let entity_id = element_id.parse::<u64>().map_err(|error| {
                    ProcedureError::Execution(format!(
                        "fulltext graph element_id '{element_id}' is not a canonical node ID: {error}"
                    ))
                })?;
                context
                    .graph()
                    .node(entity_id)
                    .map_err(|error| ProcedureError::Execution(error.to_string()))?
                    .ok_or_else(|| {
                        ProcedureError::Execution(format!(
                            "fulltext index '{index_name}' references missing canonical graph node {entity_id}"
                        ))
                    })?;
                Ok(FulltextSearchResult {
                    entity_id,
                    score: matched.score,
                    content: matched.content,
                })
            })
            .collect()
    }

    fn query_relationships(
        &self,
        _engine: &GraphEngine,
        _index_name: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<FulltextSearchResult>, ProcedureError> {
        Err(ProcedureError::NotImplemented(
            "relationship Fulltext indexes require a canonical relationship-derived generation"
                .to_string(),
        ))
    }

    fn list_analyzers(&self) -> Result<Vec<AnalyzerInfo>, ProcedureError> {
        Ok(vec![
            AnalyzerInfo {
                name: "standard".to_string(),
                description: "Tantivy default analyzer".to_string(),
            },
            AnalyzerInfo {
                name: "korean".to_string(),
                description: "Korean analyzer".to_string(),
            },
            AnalyzerInfo {
                name: "english".to_string(),
                description: "English analyzer with stemming".to_string(),
            },
        ])
    }

    fn await_index_refresh(&self) -> Result<(), ProcedureError> {
        // Unified generations have synchronous visibility at coordinator commit.
        Ok(())
    }
}

fn repository_error(error: std::io::Error) -> ProcedureError {
    ProcedureError::Execution(error.to_string())
}

/// Shared Cypher procedure provider.
pub type SharedFulltextProvider = Arc<dyn FulltextSearchProvider>;

#[cfg(test)]
mod tests {
    use zeta4j_graph::core::{PropertyContainer, Value};

    use super::*;
    use crate::server::tenant::TenantDatabaseManagerConfig;

    #[tokio::test]
    async fn query_nodes_returns_only_the_canonical_graph_node()
    -> Result<(), Box<dyn std::error::Error>> {
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
        let context = manager
            .context_for_engine(&engine)
            .ok_or_else(|| std::io::Error::other("database context missing"))?;
        let node_id = 42;
        engine
            .create_node_with_id(node_id, vec!["Chunk".to_string()])
            .await?;
        engine
            .update_node(node_id, |node| {
                node.set_property("text", Value::String("canonical graph text".to_string()));
            })
            .await?;
        engine
            .create_fulltext_index("knowledge", "Chunk", &["text".to_string()], "english")
            .await?;
        assert!(context.fulltext().has_active("knowledge")?);

        let provider = DatabaseFulltextSearchProvider::new(manager);
        let results = provider.query_nodes(&engine, "knowledge", "canonical", 10)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, node_id);
        Ok(())
    }
}
