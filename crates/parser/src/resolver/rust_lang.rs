use std::path::Path;

use domain::model::{Edge, Language};

use crate::ParseResult;
use super::{ImportResolver, ResolveContext};

/// Rust import resolver — module tree + use path resolution.
pub struct RustResolver;

impl ImportResolver for RustResolver {
    fn languages(&self) -> &[Language] {
        &[Language::Rust]
    }

    fn resolve(
        &self,
        _file_path: &Path,
        _parse_result: &ParseResult,
        _context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        // Placeholder — T08 implements full resolution
        Ok(Vec::new())
    }
}
