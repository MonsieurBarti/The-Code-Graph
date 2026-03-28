use domain::error::Result;
use domain::use_cases::query::QueryUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::CalleesArgs;
use crate::output::{print, OutputFormat};

pub fn run_callees(args: &CalleesArgs, output_format: OutputFormat) -> Result<()> {
    let (store, _root) = open_graph()?;
    let uc = QueryUseCase::new(store.clone(), store);
    let callees = uc.callees(&args.qualified_name)?;
    print(&callees, output_format);
    Ok(())
}
