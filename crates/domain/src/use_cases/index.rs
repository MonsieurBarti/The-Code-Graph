use crate::error::Result;
use crate::model::IndexStats;
use crate::ports::{GraphStore, FileSystem, GitProvider};

pub struct IndexUseCase<S, F, G> {
    store: S,
    fs: F,
    git: G,
}

impl<S: GraphStore, F: FileSystem, G: GitProvider> IndexUseCase<S, F, G> {
    pub fn new(store: S, fs: F, git: G) -> Self {
        Self { store, fs, git }
    }

    pub fn full_index(&self, _root: &std::path::Path) -> Result<IndexStats> {
        todo!("Implemented in parser slice")
    }

    pub fn incremental_index(&self, _root: &std::path::Path) -> Result<IndexStats> {
        todo!("Implemented in parser slice")
    }
}
