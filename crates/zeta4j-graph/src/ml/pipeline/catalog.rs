//! Pipeline Catalog
//!
//! 파이프라인 및 모델 관리

use super::base::{Pipeline, TrainedModel};
use super::link_prediction::LinkPredictionPipeline;
use super::node_classification::NodeClassificationPipeline;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// 파이프라인 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineType {
    NodeClassification,
    LinkPrediction,
}

/// 파이프라인 래퍼
#[derive(Debug, Clone)]
pub enum PipelineWrapper {
    NodeClassification(NodeClassificationPipeline),
    LinkPrediction(LinkPredictionPipeline),
}

impl PipelineWrapper {
    /// 이름 조회
    pub fn name(&self) -> &str {
        match self {
            Self::NodeClassification(p) => p.name(),
            Self::LinkPrediction(p) => p.name(),
        }
    }

    /// 파이프라인 종류
    pub fn pipeline_type(&self) -> PipelineType {
        match self {
            Self::NodeClassification(_) => PipelineType::NodeClassification,
            Self::LinkPrediction(_) => PipelineType::LinkPrediction,
        }
    }

    /// 훈련된 모델 조회
    pub fn trained_model(&self) -> Option<&TrainedModel> {
        match self {
            Self::NodeClassification(p) => p.trained_model(),
            Self::LinkPrediction(p) => p.trained_model(),
        }
    }

    /// Node Classification 파이프라인 참조
    pub fn as_node_classification(&self) -> Option<&NodeClassificationPipeline> {
        match self {
            Self::NodeClassification(p) => Some(p),
            _ => None,
        }
    }

    /// Node Classification 파이프라인 가변 참조
    pub fn as_node_classification_mut(&mut self) -> Option<&mut NodeClassificationPipeline> {
        match self {
            Self::NodeClassification(p) => Some(p),
            _ => None,
        }
    }

    /// Link Prediction 파이프라인 참조
    pub fn as_link_prediction(&self) -> Option<&LinkPredictionPipeline> {
        match self {
            Self::LinkPrediction(p) => Some(p),
            _ => None,
        }
    }

    /// Link Prediction 파이프라인 가변 참조
    pub fn as_link_prediction_mut(&mut self) -> Option<&mut LinkPredictionPipeline> {
        match self {
            Self::LinkPrediction(p) => Some(p),
            _ => None,
        }
    }
}

/// 파이프라인 카탈로그
///
/// 파이프라인 및 훈련된 모델 관리
#[derive(Debug, Clone, Default)]
pub struct PipelineCatalog {
    /// 파이프라인 저장소
    pipelines: Arc<RwLock<HashMap<String, PipelineWrapper>>>,
    /// 훈련된 모델 저장소
    models: Arc<RwLock<HashMap<String, TrainedModel>>>,
}

impl PipelineCatalog {
    /// 새 카탈로그 생성
    pub fn new() -> Self {
        Self {
            pipelines: Arc::new(RwLock::new(HashMap::new())),
            models: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Node Classification 파이프라인 생성
    pub fn create_node_classification(&self, name: &str) -> Result<(), String> {
        let mut pipelines = self.pipelines.write().map_err(|e| e.to_string())?;

        if pipelines.contains_key(name) {
            return Err(format!("Pipeline '{}' already exists", name));
        }

        let pipeline = NodeClassificationPipeline::new(name);
        pipelines.insert(
            name.to_string(),
            PipelineWrapper::NodeClassification(pipeline),
        );

        Ok(())
    }

    /// Link Prediction 파이프라인 생성
    pub fn create_link_prediction(&self, name: &str) -> Result<(), String> {
        let mut pipelines = self.pipelines.write().map_err(|e| e.to_string())?;

        if pipelines.contains_key(name) {
            return Err(format!("Pipeline '{}' already exists", name));
        }

        let pipeline = LinkPredictionPipeline::new(name);
        pipelines.insert(name.to_string(), PipelineWrapper::LinkPrediction(pipeline));

        Ok(())
    }

    /// 파이프라인 조회
    pub fn get_pipeline(&self, name: &str) -> Option<PipelineWrapper> {
        let pipelines = self.pipelines.read().ok()?;
        pipelines.get(name).cloned()
    }

    /// 파이프라인 수정
    pub fn with_pipeline_mut<F, R>(&self, name: &str, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut PipelineWrapper) -> R,
    {
        let mut pipelines = self.pipelines.write().map_err(|e| e.to_string())?;

        let pipeline = pipelines
            .get_mut(name)
            .ok_or_else(|| format!("Pipeline '{}' not found", name))?;

        Ok(f(pipeline))
    }

    /// 파이프라인 삭제
    pub fn drop_pipeline(&self, name: &str) -> Result<(), String> {
        let mut pipelines = self.pipelines.write().map_err(|e| e.to_string())?;

        if pipelines.remove(name).is_some() {
            Ok(())
        } else {
            Err(format!("Pipeline '{}' not found", name))
        }
    }

    /// 파이프라인 존재 여부
    pub fn exists(&self, name: &str) -> bool {
        self.pipelines
            .read()
            .map(|p| p.contains_key(name))
            .unwrap_or(false)
    }

    /// 모든 파이프라인 목록
    pub fn list_pipelines(&self) -> Vec<(String, PipelineType)> {
        self.pipelines
            .read()
            .map(|p| {
                p.iter()
                    .map(|(name, wrapper)| (name.clone(), wrapper.pipeline_type()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 훈련된 모델 저장
    pub fn store_model(&self, model: TrainedModel) -> Result<(), String> {
        let mut models = self.models.write().map_err(|e| e.to_string())?;
        models.insert(model.model_name.clone(), model);
        Ok(())
    }

    /// 훈련된 모델 조회
    pub fn get_model(&self, name: &str) -> Option<TrainedModel> {
        let models = self.models.read().ok()?;
        models.get(name).cloned()
    }

    /// 모델 삭제
    pub fn drop_model(&self, name: &str) -> Result<(), String> {
        let mut models = self.models.write().map_err(|e| e.to_string())?;

        if models.remove(name).is_some() {
            Ok(())
        } else {
            Err(format!("Model '{}' not found", name))
        }
    }

    /// 모든 모델 목록
    pub fn list_models(&self) -> Vec<String> {
        self.models
            .read()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// 파이프라인 개수
    pub fn pipeline_count(&self) -> usize {
        self.pipelines.read().map(|p| p.len()).unwrap_or(0)
    }

    /// 모델 개수
    pub fn model_count(&self) -> usize {
        self.models.read().map(|m| m.len()).unwrap_or(0)
    }

    /// 전체 초기화
    pub fn clear(&self) {
        if let Ok(mut pipelines) = self.pipelines.write() {
            pipelines.clear();
        }
        if let Ok(mut models) = self.models.write() {
            models.clear();
        }
    }
}
