use domain::error::Result;
use domain::use_cases::query::QueryUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::CallersArgs;
use crate::output::{print, OutputFormat};

pub fn run_callers(args: &CallersArgs, output_format: OutputFormat) -> Result<()> {
    let (store, _root) = open_graph()?;
    let uc = QueryUseCase::new(store.clone(), store);
    let callers = uc.callers(&args.qualified_name)?;
    print(&callers, output_format);
    Ok(())
}
