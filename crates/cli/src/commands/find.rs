use domain::error::Result;
use domain::model::EdgeKind;
use domain::ports::GraphStore;
use domain::use_cases::query::QueryUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::FindArgs;
use crate::output::{print, FindResult, OutputFormat};

pub fn run_find(args: &FindArgs, output_format: OutputFormat) -> Result<()> {
    let (store, _root) = open_graph()?;
    let uc = QueryUseCase::new(store.clone(), store.clone());
    let symbols = uc.find(&args.pattern)?;

    let mut results = Vec::new();
    for symbol in symbols {
        let callers: Vec<String> = store
            .get_edges_to(&symbol.qualified_name)?
            .into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .map(|e| e.source)
            .collect();
        let callees: Vec<String> = store
            .get_edges_from(&symbol.qualified_name)?
            .into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .map(|e| e.target)
            .collect();
        let tested_by: Vec<String> = store
            .get_edges_to(&symbol.qualified_name)?
            .into_iter()
            .filter(|e| e.kind == EdgeKind::TestedBy)
            .map(|e| e.source)
            .collect();
        results.push(FindResult { symbol, callers, callees, tested_by });
    }

    print(&results, output_format);
    Ok(())
}
