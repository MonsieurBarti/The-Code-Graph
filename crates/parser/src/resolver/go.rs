use std::path::Path;

use domain::model::{Edge, Language};

use crate::ParseResult;
use super::{ImportResolver, ResolveContext};

/// Go import resolver — go.mod + module path resolution.
pub struct GoResolver;

impl ImportResolver for GoResolver {
    fn languages(&self) -> &[Language] {
        &[Language::Go]
    }

    fn resolve(
        &self,
        _file_path: &Path,
        _parse_result: &ParseResult,
        _context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        // Placeholder — T10 implements full resolution
        Ok(Vec::new())
    }
}
