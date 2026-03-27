use domain::error::{CodeGraphError, Result};

use super::SetupArgs;

pub fn run_setup(args: &SetupArgs) -> Result<()> {
    Err(CodeGraphError::Other("setup: not yet implemented".into()))
}
