use crate::analysis::clones::{
    cluster_matches, compare_pair, compute_fingerprints, group_into_buckets,
};
use crate::model::*;
use crate::ports::{FileSystem, GraphStore};
use crate::Result;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct CloneUseCase<S, F> {
    store: S,
    fs: F,
    root: PathBuf,
}

impl<S: GraphStore, F: FileSystem> CloneUseCase<S, F> {
    pub fn new(store: S, fs: F, root: PathBuf) -> Self {
        Self { store, fs, root }
    }

    pub fn analyze(&self, config: &CloneConfig) -> Result<CloneAnalysis> {
        let symbols = self.store.all_symbols()?;
        let edges = self.store.all_edges()?;

        let fingerprints = compute_fingerprints(&symbols, &edges, config);
        let total_symbols = fingerprints.len();

        if total_symbols < 2 {
            return Ok(CloneAnalysis {
                clusters: Vec::new(),
                total_symbols_analyzed: total_symbols,
                symbols_in_clones: 0,
                duplication_pct: 0.0,
                most_duplicated: None,
            });
        }

        let buckets = group_into_buckets(&fingerprints);

        // Phase 2: pairwise comparison within buckets with file content cache
        let mut file_cache: HashMap<PathBuf, String> = HashMap::new();
        let mut all_matches: Vec<CloneMatch> = Vec::new();

        for (_key, bucket) in &buckets {
            if bucket.len() < 2 {
                continue;
            }
            let max_pairs = config.max_candidates_per_bucket;
            let mut pair_count = 0;

            for i in 0..bucket.len() {
                if pair_count >= max_pairs {
                    break;
                }
                for j in (i + 1)..bucket.len() {
                    if pair_count >= max_pairs {
                        break;
                    }
                    let fp_a = &bucket[i];
                    let fp_b = &bucket[j];
                    let cross_lang = fp_a.language != fp_b.language;

                    if cross_lang {
                        let mut m = compare_pair("", "", true, config.threshold).unwrap();
                        m.source = fp_a.qualified_name.clone();
                        m.target = fp_b.qualified_name.clone();
                        all_matches.push(m);
                    } else {
                        let body_a = self.read_body(&mut file_cache, fp_a);
                        let body_b = self.read_body(&mut file_cache, fp_b);
                        if let Some(mut m) =
                            compare_pair(&body_a, &body_b, false, config.threshold)
                        {
                            m.source = fp_a.qualified_name.clone();
                            m.target = fp_b.qualified_name.clone();
                            all_matches.push(m);
                        }
                    }
                    pair_count += 1;
                }
            }
        }

        let clusters = cluster_matches(&all_matches);
        let symbols_in_clones: usize = clusters.iter().map(|c| c.members.len()).sum();
        let duplication_pct = if total_symbols == 0 {
            0.0
        } else {
            symbols_in_clones as f64 / total_symbols as f64 * 100.0
        };

        let mut pair_counts: HashMap<&str, usize> = HashMap::new();
        for m in &all_matches {
            *pair_counts.entry(&m.source).or_default() += 1;
            *pair_counts.entry(&m.target).or_default() += 1;
        }
        let most_duplicated = pair_counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(name, _)| name.to_string());

        Ok(CloneAnalysis {
            clusters,
            total_symbols_analyzed: total_symbols,
            symbols_in_clones,
            duplication_pct,
            most_duplicated,
        })
    }

    fn read_body(
        &self,
        cache: &mut HashMap<PathBuf, String>,
        fp: &StructuralFingerprint,
    ) -> String {
        let file_content = cache
            .entry(fp.file.clone())
            .or_insert_with(|| {
                let abs_path = self.root.join(&fp.file);
                self.fs.read_file(&abs_path).unwrap_or_default()
            })
            .clone();

        // Extract only the symbol body lines (1-indexed line_start..=line_end)
        let lines: Vec<&str> = file_content.lines().collect();
        let start = fp.line_start.saturating_sub(1); // convert to 0-indexed
        let end = fp.line_end.min(lines.len());
        if start >= lines.len() || start >= end {
            return String::new();
        }
        lines[start..end].join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::test_support::{InMemoryGraphStore, MockFileSystem};
    use std::path::PathBuf;

    fn build_clone_store() -> (InMemoryGraphStore, MockFileSystem) {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(SymbolNode {
            name: "add".into(),
            qualified_name: "a.rs::add".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: PathBuf::from("a.rs"),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        store.insert_symbol(SymbolNode {
            name: "sum".into(),
            qualified_name: "b.rs::sum".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: PathBuf::from("b.rs"),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        store.insert_symbol(SymbolNode {
            name: "tiny".into(),
            qualified_name: "c.rs::tiny".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: PathBuf::from("c.rs"),
                line_start: 1,
                line_end: 3,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });

        let fs = MockFileSystem::new(vec![
            (PathBuf::from("/test/a.rs"), "fn add(x: i32, y: i32) -> i32 {\n    x + y\n}\n// pad\n// pad\n// pad\n// pad\n// pad\n// pad\n// pad".into()),
            (PathBuf::from("/test/b.rs"), "fn sum(a: i32, b: i32) -> i32 {\n    a + b\n}\n// pad\n// pad\n// pad\n// pad\n// pad\n// pad\n// pad".into()),
            (PathBuf::from("/test/c.rs"), "fn tiny() { 1 }".into()),
        ]);
        (store, fs)
    }

    #[test]
    fn analyze_detects_type2_clones() {
        let (store, fs) = build_clone_store();
        let uc = CloneUseCase::new(store, fs, PathBuf::from("/test"));
        let analysis = uc.analyze(&CloneConfig::default()).unwrap();
        assert!(!analysis.clusters.is_empty());
        assert!(analysis.duplication_pct > 0.0);
    }

    #[test]
    fn analyze_filters_by_min_lines() {
        let (store, fs) = build_clone_store();
        let uc = CloneUseCase::new(store, fs, PathBuf::from("/test"));
        let analysis = uc.analyze(&CloneConfig::default()).unwrap();
        let all_members: Vec<&str> = analysis
            .clusters
            .iter()
            .flat_map(|c| c.members.iter().map(|m| m.as_str()))
            .collect();
        assert!(!all_members.contains(&"c.rs::tiny"));
    }

    #[test]
    fn analyze_empty_graph() {
        let store = InMemoryGraphStore::new();
        let fs = MockFileSystem::new(vec![]);
        let uc = CloneUseCase::new(store, fs, PathBuf::from("/test"));
        let analysis = uc.analyze(&CloneConfig::default()).unwrap();
        assert!(analysis.clusters.is_empty());
        assert_eq!(analysis.duplication_pct, 0.0);
        assert!(analysis.most_duplicated.is_none());
    }

    #[test]
    fn analyze_single_symbol() {
        let mut store = InMemoryGraphStore::new();
        store.insert_symbol(SymbolNode {
            name: "only".into(),
            qualified_name: "a.rs::only".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: PathBuf::from("a.rs"),
                line_start: 1,
                line_end: 10,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        let fs = MockFileSystem::new(vec![]);
        let uc = CloneUseCase::new(store, fs, PathBuf::from("/test"));
        let analysis = uc.analyze(&CloneConfig::default()).unwrap();
        assert!(analysis.clusters.is_empty());
    }
}
