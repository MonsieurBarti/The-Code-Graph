use domain::error::Result;
use domain::use_cases::query::QueryUseCase;

use crate::commands::helpers::open_graph;
use crate::output::{print, OutputFormat};

pub fn run_stats(output_format: OutputFormat) -> Result<()> {
    let (store, _root) = open_graph()?;
    let uc = QueryUseCase::new(store.clone(), store);
    let stats = uc.stats()?;
    print(&stats, output_format);
    Ok(())
}
