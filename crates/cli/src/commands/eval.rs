use domain::error::Result;
use crate::output::OutputFormat;
use super::EvalArgs;

pub fn run_eval(args: &EvalArgs, output_format: OutputFormat) -> Result<()> {
    let _ = (args, output_format);
    Err(domain::error::CodeGraphError::Other(
        "eval: not yet implemented".into(),
    ))
}
