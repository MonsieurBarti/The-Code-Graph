use std::path::{Path, PathBuf};

use domain::model::{Edge, Language};

use crate::ParseResult;
use super::{ImportResolver, ResolveContext};

/// TS/JS import resolver using oxc_resolver + barrel chain traversal.
pub struct TypeScriptResolver {
    project_root: PathBuf,
}

impl TypeScriptResolver {
    pub fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
        }
    }
}

impl ImportResolver for TypeScriptResolver {
    fn languages(&self) -> &[Language] {
        &[Language::TypeScript, Language::JavaScript]
    }

    fn resolve(
        &self,
        _file_path: &Path,
        _parse_result: &ParseResult,
        _context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        // Placeholder — T07 implements full resolution
        Ok(Vec::new())
    }
}
