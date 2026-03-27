use domain::error::Result;
use domain::use_cases::query::QueryUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::SearchArgs;
use crate::output::{print, OutputFormat};

pub fn run_search(args: &SearchArgs, output_format: OutputFormat) -> Result<()> {
    let (store, _root) = open_graph()?;
    let uc = QueryUseCase::new(store.clone(), store);
    let results = uc.search(&args.query, args.limit)?;
    print(&results, output_format);
    Ok(())
}
