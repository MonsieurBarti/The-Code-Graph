use domain::error::{CodeGraphError, Result};

pub fn not_implemented(command_name: &str) -> Result<()> {
    Err(CodeGraphError::Other(format!(
        "{command_name}: not yet implemented — coming in a future release"
    )))
}
