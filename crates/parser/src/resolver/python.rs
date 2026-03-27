use std::path::Path;

use domain::model::{Edge, Language};

use crate::ParseResult;
use super::{ImportResolver, ResolveContext};

/// Python import resolver — filesystem prober + stdlib detection.
pub struct PythonResolver;

impl ImportResolver for PythonResolver {
    fn languages(&self) -> &[Language] {
        &[Language::Python]
    }

    fn resolve(
        &self,
        _file_path: &Path,
        _parse_result: &ParseResult,
        _context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        // Placeholder — T09 implements full resolution
        Ok(Vec::new())
    }
}
