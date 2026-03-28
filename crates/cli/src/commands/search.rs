use std::sync::Arc;

use domain::error::Result;
use domain::model::{HybridSearchConfig, SearchMode};
use domain::ports::VectorStore;
use domain::use_cases::query::QueryUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::SearchArgs;
use crate::config::load_config;
use crate::output::{print, OutputFormat};

pub fn run_search(args: &SearchArgs, output_format: OutputFormat) -> Result<()> {
    if args.semantic_only && args.fts_only {
        return Err(domain::error::CodeGraphError::Other(
            "--semantic-only and --fts-only are mutually exclusive".into(),
        ));
    }

    let (store, root) = open_graph()?;
    let config = load_config(&root)?;

    let mode = match (args.semantic_only, args.fts_only) {
        (true, false) => SearchMode::SemanticOnly,
        (false, true) => SearchMode::FtsOnly,
        _ => SearchMode::Hybrid,
    };

    let hybrid_config = HybridSearchConfig {
        rrf_k: config.search.as_ref().and_then(|s| s.rrf_k).unwrap_or(60),
        kind_boost: config
            .search
            .as_ref()
            .and_then(|s| s.kind_boost)
            .unwrap_or(true),
    };

    let vs: Arc<dyn VectorStore> = Arc::new(store.clone());
    let has_embeddings = vs.has_embeddings();

    if has_embeddings {
        let model = config
            .embeddings
            .as_ref()
            .and_then(|e| e.model.clone())
            .unwrap_or_else(|| "all-MiniLM-L6-v2".into());
        let ep: Arc<dyn domain::ports::EmbeddingProvider> = Arc::new(
            embeddings::OnnxEmbeddingProvider::from_model_name(&model, 384)
                .map_err(|e| domain::error::CodeGraphError::Other(e.to_string()))?,
        );
        let uc = QueryUseCase::with_hybrid(store.clone(), store, Some(vs), Some(ep));
        let results = uc.hybrid_search(&args.query, args.limit, mode, &hybrid_config)?;
        print(&results, output_format);
    } else {
        if args.semantic_only {
            return Err(domain::error::CodeGraphError::Other(
                "no embeddings found; run 'code-graph index --embed' first".into(),
            ));
        }
        let uc = QueryUseCase::new(store.clone(), store);
        let results = uc.search(&args.query, args.limit)?;
        print(&results, output_format);
    }

    Ok(())
}
