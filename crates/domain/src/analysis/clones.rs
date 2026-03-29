use crate::model::{
    BucketKey, CloneCluster, CloneConfig, CloneMatch, CloneType, Confidence, Edge, EdgeKind,
    Language, StructuralFingerprint, SymbolNode,
};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// T03 — Structural fingerprinting
// ---------------------------------------------------------------------------

/// Maps each `EdgeKind` variant to a unique bit index (0..15).
fn edge_kind_index(kind: &EdgeKind) -> u32 {
    match kind {
        EdgeKind::Contains => 0,
        EdgeKind::ChildOf => 1,
        EdgeKind::Calls => 2,
        EdgeKind::ImportsFrom => 3,
        EdgeKind::Extends => 4,
        EdgeKind::Implements => 5,
        EdgeKind::TestedBy => 6,
        EdgeKind::DependsOn => 7,
        EdgeKind::BarrelReExportAll => 8,
        EdgeKind::ConditionalImport => 9,
        EdgeKind::SideEffectImport => 10,
        EdgeKind::DotImport => 11,
        EdgeKind::HasDecorator => 12,
        EdgeKind::Embeds => 13,
        EdgeKind::TypeReference => 14,
        EdgeKind::ReExport => 15,
    }
}

/// Compute structural fingerprints for all symbols that pass the `min_lines`
/// filter in `config`.
///
/// Complexity: O(S + E) — one pass to build adjacency maps, one pass over
/// symbols.
pub fn compute_fingerprints(
    symbols: &[SymbolNode],
    edges: &[Edge],
    config: &CloneConfig,
) -> Vec<StructuralFingerprint> {
    // Build outgoing adjacency map: source -> Vec<&Edge>
    let mut outgoing: HashMap<&str, Vec<&Edge>> = HashMap::new();
    // Build incoming adjacency map: target -> Vec<&Edge>
    let mut incoming: HashMap<&str, Vec<&Edge>> = HashMap::new();

    for edge in edges {
        outgoing.entry(edge.source.as_str()).or_default().push(edge);
        incoming.entry(edge.target.as_str()).or_default().push(edge);
    }

    let mut fingerprints = Vec::with_capacity(symbols.len());

    for sym in symbols {
        let body_line_count = sym
            .location
            .line_end
            .saturating_sub(sym.location.line_start)
            + 1;

        if body_line_count < config.min_lines {
            continue;
        }

        let empty = Vec::new();
        let out_edges = outgoing.get(sym.qualified_name.as_str()).unwrap_or(&empty);
        let in_edges = incoming.get(sym.qualified_name.as_str()).unwrap_or(&empty);

        // Callee count: outgoing edges with confidence >= High
        let callee_count = out_edges
            .iter()
            .filter(|e| e.kind.confidence() >= Confidence::High)
            .count();

        // Caller count: incoming edges with confidence >= High
        let caller_count = in_edges
            .iter()
            .filter(|e| e.kind.confidence() >= Confidence::High)
            .count();

        // Child count: outgoing Contains edges
        let child_count = out_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .count();

        // Edge kind bitset: one bit per EdgeKind present in outgoing edges
        let mut edge_kind_set: u32 = 0;
        for e in out_edges.iter() {
            edge_kind_set |= 1u32 << edge_kind_index(&e.kind);
        }

        let language = Language::from_path(&sym.location.file);

        fingerprints.push(StructuralFingerprint {
            qualified_name: sym.qualified_name.clone(),
            symbol_kind: sym.kind,
            callee_count,
            caller_count,
            edge_kind_set,
            body_line_count,
            child_count,
            language,
            file: sym.location.file.clone(),
            line_start: sym.location.line_start,
            line_end: sym.location.line_end,
        });
    }

    fingerprints
}

/// Split source code into tokens.
/// - Strips line comments (`//` and `#`)
/// - Splits on whitespace
/// - Splits tokens further on punctuation boundaries (keeps punctuation as separate tokens)
/// - Skips empty tokens
pub fn tokenize(source: &str) -> Vec<String> {
    let mut tokens = Vec::new();

    for line in source.lines() {
        // Strip line comments: // and #
        let stripped = if let Some(pos) = line.find("//") {
            &line[..pos]
        } else if let Some(pos) = line.find('#') {
            &line[..pos]
        } else {
            line
        };

        for word in stripped.split_whitespace() {
            // Split word on punctuation boundaries, keeping punctuation as separate tokens
            split_on_punctuation(word, &mut tokens);
        }
    }

    tokens
}

/// Split a word on punctuation boundaries, keeping punctuation as separate tokens.
fn split_on_punctuation(word: &str, out: &mut Vec<String>) {
    let mut current = String::new();

    for ch in word.chars() {
        if ch.is_ascii_punctuation() {
            // Flush current alphanumeric token
            if !current.is_empty() {
                out.push(current.clone());
                current.clear();
            }
            // Push punctuation as its own token
            out.push(ch.to_string());
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
}

/// Replace identifiers with positional placeholders.
/// - Keywords, operators/punctuation, and numeric literals are kept as-is.
/// - Identifiers are replaced with `_1`, `_2`, etc. (same identifier → same placeholder everywhere).
pub fn normalize_identifiers(tokens: &[String]) -> Vec<String> {
    let keywords: HashSet<&str> = [
        // Rust
        "fn",
        "let",
        "mut",
        "const",
        "struct",
        "enum",
        "impl",
        "trait",
        "pub",
        "use",
        "mod",
        "if",
        "else",
        "match",
        "for",
        "while",
        "loop",
        "return",
        "break",
        "continue",
        "where",
        "async",
        "await",
        "move",
        "ref",
        "type",
        "self",
        "super",
        "crate",
        // TypeScript/JS
        "function",
        "var",
        "class",
        "interface",
        "export",
        "import",
        "from",
        "default",
        "extends",
        "implements",
        "new",
        "this",
        "typeof",
        "instanceof",
        "void",
        "null",
        "undefined",
        "true",
        "false",
        "try",
        "catch",
        "throw",
        "finally",
        // Python
        "def",
        "class",
        "import",
        "from",
        "return",
        "if",
        "elif",
        "else",
        "for",
        "while",
        "with",
        "as",
        "try",
        "except",
        "raise",
        "pass",
        "None",
        "True",
        "False",
        "lambda",
        // Go
        "func",
        "package",
        "import",
        "type",
        "struct",
        "interface",
        "map",
        "chan",
        "go",
        "defer",
        "select",
        "case",
        "range",
        "nil",
    ]
    .iter()
    .copied()
    .collect();

    let mut identifier_map: HashMap<String, usize> = HashMap::new();
    let mut result = Vec::with_capacity(tokens.len());

    for token in tokens {
        // Keep keywords as-is
        if keywords.contains(token.as_str()) {
            result.push(token.clone());
            continue;
        }

        // Keep purely punctuation tokens as-is (single or multi-char operator/punct)
        if token.chars().all(|c| c.is_ascii_punctuation()) {
            result.push(token.clone());
            continue;
        }

        // Keep numeric literals as-is
        if token
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            result.push(token.clone());
            continue;
        }

        // It's an identifier — replace with positional placeholder
        let next_id = identifier_map.len() + 1;
        let idx = identifier_map.entry(token.clone()).or_insert(next_id);
        result.push(format!("_{}", idx));
    }

    result
}

/// Jaccard similarity on token multisets.
/// - intersection = sum of min(count_a, count_b) for each token
/// - union = sum of max(count_a, count_b) for each token
/// - Returns intersection / union (both empty → 1.0, one empty → 0.0)
pub fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let mut counts_a: HashMap<&str, usize> = HashMap::new();
    let mut counts_b: HashMap<&str, usize> = HashMap::new();

    for token in a {
        *counts_a.entry(token.as_str()).or_insert(0) += 1;
    }
    for token in b {
        *counts_b.entry(token.as_str()).or_insert(0) += 1;
    }

    // Collect all unique tokens
    let all_tokens: HashSet<&str> = counts_a.keys().chain(counts_b.keys()).copied().collect();

    let mut intersection = 0usize;
    let mut union = 0usize;

    for token in &all_tokens {
        let ca = counts_a.get(token).copied().unwrap_or(0);
        let cb = counts_b.get(token).copied().unwrap_or(0);
        intersection += ca.min(cb);
        union += ca.max(cb);
    }

    if union == 0 {
        return 1.0;
    }

    intersection as f64 / union as f64
}

// ---------------------------------------------------------------------------
// T04 — Quantize-and-concatenate bucketing
// ---------------------------------------------------------------------------

/// Quantize a call count into a 2-bit bin.
/// 0 → 0, 1–2 → 1, 3–5 → 2, 6+ → 3
fn count_bin(count: usize) -> u8 {
    match count {
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        _ => 3,
    }
}

/// Quantize a line count into a 2-bit bin.
/// 0–5 → 0, 6–15 → 1, 16–50 → 2, 51+ → 3
fn line_bin(lines: usize) -> u8 {
    match lines {
        0..=5 => 0,
        6..=15 => 1,
        16..=50 => 2,
        _ => 3,
    }
}

/// Quantize a child count into a 2-bit bin.
/// 0 → 0, 1–3 → 1, 4+ → 2
fn child_bin(count: usize) -> u8 {
    match count {
        0 => 0,
        1..=3 => 1,
        _ => 2,
    }
}

/// Compute the coarse bucket key for a structural fingerprint.
pub fn bucket_key(fp: &StructuralFingerprint) -> BucketKey {
    BucketKey {
        kind: fp.symbol_kind,
        callee_bin: count_bin(fp.callee_count),
        caller_bin: count_bin(fp.caller_count),
        line_bin: line_bin(fp.body_line_count),
        child_bin: child_bin(fp.child_count),
    }
}

/// Group a slice of fingerprints by their bucket key.
pub fn group_into_buckets(
    fingerprints: &[StructuralFingerprint],
) -> HashMap<BucketKey, Vec<&StructuralFingerprint>> {
    let mut map: HashMap<BucketKey, Vec<&StructuralFingerprint>> = HashMap::new();
    for fp in fingerprints {
        map.entry(bucket_key(fp)).or_default().push(fp);
    }
    map
}

// ---------------------------------------------------------------------------
// T06 — Candidate comparison and connected-component clustering
// ---------------------------------------------------------------------------

/// Compare two source code snippets and return a `CloneMatch` if they are
/// similar enough, or `None` otherwise.
///
/// - `cross_language`: structural-only path — immediately returns a
///   `StructuralOnly` match with similarity 1.0.
/// - Otherwise: raw Jaccard ≥ 0.95 → `Type1`; normalized Jaccard ≥ threshold
///   → `Type2`; below threshold → `None`.
/// - The `source` and `target` fields of the returned match are left empty;
///   the caller is expected to fill them in.
pub fn compare_pair(
    source_code_a: &str,
    source_code_b: &str,
    cross_language: bool,
    threshold: f64,
) -> Option<CloneMatch> {
    if cross_language {
        return Some(CloneMatch {
            source: String::new(),
            target: String::new(),
            similarity: 1.0,
            clone_type: CloneType::StructuralOnly,
        });
    }

    let tokens_a = tokenize(source_code_a);
    let tokens_b = tokenize(source_code_b);

    if tokens_a.is_empty() || tokens_b.is_empty() {
        return None;
    }

    let raw_sim = jaccard_similarity(&tokens_a, &tokens_b);
    if raw_sim >= 0.95 {
        return Some(CloneMatch {
            source: String::new(),
            target: String::new(),
            similarity: raw_sim,
            clone_type: CloneType::Type1,
        });
    }

    let norm_a = normalize_identifiers(&tokens_a);
    let norm_b = normalize_identifiers(&tokens_b);
    let norm_sim = jaccard_similarity(&norm_a, &norm_b);
    if norm_sim >= threshold {
        return Some(CloneMatch {
            source: String::new(),
            target: String::new(),
            similarity: norm_sim,
            clone_type: CloneType::Type2,
        });
    }

    None
}

/// Cluster a slice of `CloneMatch` records into connected components using
/// Union-Find, then return a sorted `Vec<CloneCluster>` (largest first).
pub fn cluster_matches(matches: &[CloneMatch]) -> Vec<CloneCluster> {
    if matches.is_empty() {
        return Vec::new();
    }

    // Collect all unique node names and assign integer IDs.
    let mut name_to_id: HashMap<String, usize> = HashMap::new();
    let mut id_to_name: Vec<String> = Vec::new();

    for m in matches {
        for name in [m.source.as_str(), m.target.as_str()] {
            if !name_to_id.contains_key(name) {
                let id = id_to_name.len();
                name_to_id.insert(name.to_string(), id);
                id_to_name.push(name.to_string());
            }
        }
    }

    let n = id_to_name.len();

    // Union-Find with path compression and union by rank.
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<usize> = vec![0; n];

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }

    fn union(parent: &mut [usize], rank: &mut [usize], x: usize, y: usize) {
        let rx = find(parent, x);
        let ry = find(parent, y);
        if rx == ry {
            return;
        }
        match rank[rx].cmp(&rank[ry]) {
            std::cmp::Ordering::Less => parent[rx] = ry,
            std::cmp::Ordering::Greater => parent[ry] = rx,
            std::cmp::Ordering::Equal => {
                parent[ry] = rx;
                rank[rx] += 1;
            }
        }
    }

    for m in matches {
        let a = name_to_id[m.source.as_str()];
        let b = name_to_id[m.target.as_str()];
        union(&mut parent, &mut rank, a, b);
    }

    // Group nodes by root.
    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        components.entry(root).or_default().push(i);
    }

    // Build one CloneCluster per component.
    let mut clusters: Vec<CloneCluster> = components
        .values()
        .map(|member_ids| {
            // Collect member names, sorted alphabetically.
            let mut members: Vec<String> = member_ids
                .iter()
                .map(|&id| id_to_name[id].to_string())
                .collect();
            members.sort();

            let member_set: HashSet<&str> = members.iter().map(String::as_str).collect();

            // Intra-cluster matches: both endpoints are in this component.
            let intra: Vec<&CloneMatch> = matches
                .iter()
                .filter(|m| {
                    member_set.contains(m.source.as_str()) && member_set.contains(m.target.as_str())
                })
                .collect();

            let avg_similarity = if intra.is_empty() {
                0.0
            } else {
                intra.iter().map(|m| m.similarity).sum::<f64>() / intra.len() as f64
            };

            // Dominant clone type: Type1 > Type2 > StructuralOnly.
            let dominant_type = if intra.iter().any(|m| m.clone_type == CloneType::Type1) {
                CloneType::Type1
            } else if intra.iter().any(|m| m.clone_type == CloneType::Type2) {
                CloneType::Type2
            } else {
                CloneType::StructuralOnly
            };

            let representative = members[0].clone(); // first alphabetically

            let intra_matches: Vec<CloneMatch> = intra.into_iter().cloned().collect();

            CloneCluster {
                id: 0, // assigned after sorting
                members,
                avg_similarity,
                clone_type: dominant_type,
                representative,
                intra_matches,
            }
        })
        .collect();

    // Sort: largest first; break ties by representative name.
    clusters.sort_by(|a, b| {
        b.members
            .len()
            .cmp(&a.members.len())
            .then_with(|| a.representative.cmp(&b.representative))
    });

    // Assign 1-indexed IDs.
    for (i, cluster) in clusters.iter_mut().enumerate() {
        cluster.id = i + 1;
    }

    clusters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_identical_tokens() {
        let a = vec!["fn".to_string(), "foo".into(), "(".into(), ")".into()];
        let b = a.clone();
        assert!((jaccard_similarity(&a, &b) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_disjoint_tokens() {
        let a = vec!["fn".to_string(), "foo".into()];
        let b = vec!["class".to_string(), "Bar".into()];
        assert!((jaccard_similarity(&a, &b) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_partial_overlap() {
        let a = vec!["fn".to_string(), "foo".into(), "(".into()];
        let b = vec!["fn".to_string(), "bar".into(), "(".into()];
        // intersection: fn, ( = 2; union: fn, foo, bar, ( = 4; jaccard = 0.5
        assert!((jaccard_similarity(&a, &b) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn normalize_replaces_identifiers_with_positional_placeholders() {
        let tokens = vec![
            "fn".to_string(),
            "add".into(),
            "(".into(),
            "a".into(),
            ",".into(),
            "b".into(),
            ")".into(),
            "{".into(),
            "a".into(),
            "+".into(),
            "b".into(),
            "}".into(),
        ];
        let normalized = normalize_identifiers(&tokens);
        assert_eq!(normalized[0], "fn"); // keyword kept
        assert_eq!(normalized[1], "_1"); // add -> _1
        assert_eq!(normalized[3], "_2"); // a -> _2
        assert_eq!(normalized[5], "_3"); // b -> _3
        assert_eq!(normalized[8], "_2"); // a again -> _2
        assert_eq!(normalized[10], "_3"); // b again -> _3
    }

    #[test]
    fn type2_clones_detected_after_normalization() {
        let tokens_a = vec![
            "fn".to_string(),
            "add".into(),
            "(".into(),
            "x".into(),
            ",".into(),
            "y".into(),
            ")".into(),
            "{".into(),
            "x".into(),
            "+".into(),
            "y".into(),
            "}".into(),
        ];
        let tokens_b = vec![
            "fn".to_string(),
            "sum".into(),
            "(".into(),
            "a".into(),
            ",".into(),
            "b".into(),
            ")".into(),
            "{".into(),
            "a".into(),
            "+".into(),
            "b".into(),
            "}".into(),
        ];
        let raw_score = jaccard_similarity(&tokens_a, &tokens_b);
        assert!(raw_score < 0.95);
        let norm_a = normalize_identifiers(&tokens_a);
        let norm_b = normalize_identifiers(&tokens_b);
        let norm_score = jaccard_similarity(&norm_a, &norm_b);
        assert!((norm_score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tokenize_strips_comments_and_splits() {
        let source = "fn foo() { // comment\n  let x = 1;\n}";
        let tokens = tokenize(source);
        assert!(!tokens.contains(&"comment".to_string()));
        assert!(tokens.contains(&"fn".to_string()));
        assert!(tokens.contains(&"foo".to_string()));
    }

    #[test]
    fn tokenize_empty_source() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn jaccard_both_empty() {
        assert!((jaccard_similarity(&[], &[]) - 1.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // T04 bucketing tests
    // -----------------------------------------------------------------------

    use crate::model::{Language, SymbolKind};
    use std::path::PathBuf;

    #[test]
    fn bucket_key_groups_similar_symbols() {
        let fp1 = StructuralFingerprint {
            qualified_name: "a::foo".into(),
            symbol_kind: SymbolKind::Function,
            callee_count: 2,
            caller_count: 1,
            edge_kind_set: 0,
            body_line_count: 10,
            child_count: 0,
            language: Language::Rust,
            file: PathBuf::from("a.rs"),
            line_start: 1,
            line_end: 10,
        };
        let fp2 = StructuralFingerprint {
            qualified_name: "b::bar".into(),
            symbol_kind: SymbolKind::Function,
            callee_count: 1,
            caller_count: 1,
            edge_kind_set: 0,
            body_line_count: 10,
            child_count: 0,
            language: Language::Rust,
            file: PathBuf::from("b.rs"),
            line_start: 1,
            line_end: 10,
        };
        assert_eq!(bucket_key(&fp1), bucket_key(&fp2)); // both callee in bin 1 (1-2)
    }

    #[test]
    fn bucket_key_separates_different_kinds() {
        let fp1 = StructuralFingerprint {
            qualified_name: "a::foo".into(),
            symbol_kind: SymbolKind::Function,
            callee_count: 0,
            caller_count: 0,
            edge_kind_set: 0,
            body_line_count: 10,
            child_count: 0,
            language: Language::Rust,
            file: PathBuf::from("a.rs"),
            line_start: 1,
            line_end: 10,
        };
        let fp2 = StructuralFingerprint {
            qualified_name: "a::bar".into(),
            symbol_kind: SymbolKind::Method,
            callee_count: 0,
            caller_count: 0,
            edge_kind_set: 0,
            body_line_count: 10,
            child_count: 0,
            language: Language::Rust,
            file: PathBuf::from("a.rs"),
            line_start: 1,
            line_end: 10,
        };
        assert_ne!(bucket_key(&fp1), bucket_key(&fp2));
    }

    #[test]
    fn group_into_buckets_correct_grouping() {
        let fps = vec![
            StructuralFingerprint {
                qualified_name: "a::f1".into(),
                symbol_kind: SymbolKind::Function,
                callee_count: 1,
                caller_count: 0,
                edge_kind_set: 0,
                body_line_count: 10,
                child_count: 0,
                language: Language::Rust,
                file: PathBuf::from("a.rs"),
                line_start: 1,
                line_end: 10,
            },
            StructuralFingerprint {
                qualified_name: "b::f2".into(),
                symbol_kind: SymbolKind::Function,
                callee_count: 2,
                caller_count: 0,
                edge_kind_set: 0,
                body_line_count: 12,
                child_count: 0,
                language: Language::Rust,
                file: PathBuf::from("b.rs"),
                line_start: 1,
                line_end: 12,
            },
            StructuralFingerprint {
                qualified_name: "c::f3".into(),
                symbol_kind: SymbolKind::Class,
                callee_count: 1,
                caller_count: 0,
                edge_kind_set: 0,
                body_line_count: 10,
                child_count: 0,
                language: Language::Rust,
                file: PathBuf::from("c.rs"),
                line_start: 1,
                line_end: 10,
            },
        ];
        let buckets = group_into_buckets(&fps);
        assert_eq!(buckets.len(), 2); // f1,f2 in one bucket; f3 in another
        let max_bucket = buckets.values().map(|v| v.len()).max().unwrap();
        assert_eq!(max_bucket, 2);
    }

    #[test]
    fn count_bin_boundaries() {
        assert_eq!(count_bin(0), 0);
        assert_eq!(count_bin(1), 1);
        assert_eq!(count_bin(2), 1);
        assert_eq!(count_bin(3), 2);
        assert_eq!(count_bin(5), 2);
        assert_eq!(count_bin(6), 3);
        assert_eq!(count_bin(100), 3);
    }

    // -----------------------------------------------------------------------
    // T03 fingerprinting tests
    // -----------------------------------------------------------------------

    use crate::model::{Location, Visibility};

    fn make_symbol(
        name: &str,
        kind: SymbolKind,
        line_start: usize,
        line_end: usize,
    ) -> crate::model::SymbolNode {
        crate::model::SymbolNode {
            name: name.to_string(),
            qualified_name: format!("test.rs::{name}"),
            kind,
            location: Location {
                file: PathBuf::from("test.rs"),
                line_start,
                line_end,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    fn make_edge(source: &str, target: &str, kind: EdgeKind) -> Edge {
        Edge {
            kind,
            source: format!("test.rs::{source}"),
            target: format!("test.rs::{target}"),
            metadata: None,
        }
    }

    #[test]
    fn fingerprint_captures_symbol_structure() {
        let symbols = vec![make_symbol("foo", SymbolKind::Function, 1, 20)];
        let edges = vec![
            make_edge("foo", "bar", EdgeKind::Calls),
            make_edge("foo", "baz", EdgeKind::Calls),
            make_edge("qux", "foo", EdgeKind::Calls),
        ];
        let config = CloneConfig {
            min_lines: 1,
            ..CloneConfig::default()
        };

        let fps = compute_fingerprints(&symbols, &edges, &config);

        assert_eq!(fps.len(), 1);
        let fp = &fps[0];
        assert_eq!(fp.callee_count, 2, "should count 2 outgoing Calls edges");
        assert_eq!(fp.caller_count, 1, "should count 1 incoming Calls edge");
        assert_eq!(fp.body_line_count, 20, "line_end - line_start + 1 = 20");
        assert_eq!(fp.child_count, 0, "no Contains edges");
    }

    #[test]
    fn fingerprint_filters_by_min_lines() {
        // Symbol with only 3 lines — below default min_lines of 5
        let symbols = vec![make_symbol("tiny", SymbolKind::Function, 1, 3)];
        let edges = vec![];
        let config = CloneConfig::default(); // min_lines = 5

        let fps = compute_fingerprints(&symbols, &edges, &config);

        assert_eq!(fps.len(), 0, "symbol with 3 lines should be filtered out");
    }

    #[test]
    fn fingerprint_counts_contains_edges_as_children() {
        let symbols = vec![make_symbol("MyClass", SymbolKind::Class, 1, 50)];
        let edges = vec![
            make_edge("MyClass", "method_a", EdgeKind::Contains),
            make_edge("MyClass", "method_b", EdgeKind::Contains),
            make_edge("MyClass", "field_c", EdgeKind::Contains),
        ];
        let config = CloneConfig {
            min_lines: 1,
            ..CloneConfig::default()
        };

        let fps = compute_fingerprints(&symbols, &edges, &config);

        assert_eq!(fps.len(), 1);
        let fp = &fps[0];
        assert_eq!(
            fp.child_count, 3,
            "should count 3 Contains edges as children"
        );
        assert_eq!(
            fp.callee_count, 0,
            "Contains has Structural confidence, not High"
        );
    }

    #[test]
    fn fingerprint_empty_input() {
        let fps = compute_fingerprints(&[], &[], &CloneConfig::default());
        assert_eq!(fps.len(), 0);
    }

    #[test]
    fn fingerprint_edge_kind_bitset() {
        let symbols = vec![make_symbol("node", SymbolKind::Function, 1, 10)];
        let edges = vec![
            make_edge("node", "a", EdgeKind::Calls),
            make_edge("node", "b", EdgeKind::Extends),
            make_edge("node", "c", EdgeKind::Contains),
        ];
        let config = CloneConfig {
            min_lines: 1,
            ..CloneConfig::default()
        };

        let fps = compute_fingerprints(&symbols, &edges, &config);
        assert_eq!(fps.len(), 1);

        let fp = &fps[0];
        let calls_bit = 1u32 << edge_kind_index(&EdgeKind::Calls);
        let extends_bit = 1u32 << edge_kind_index(&EdgeKind::Extends);
        let contains_bit = 1u32 << edge_kind_index(&EdgeKind::Contains);

        assert_ne!(fp.edge_kind_set & calls_bit, 0, "Calls bit should be set");
        assert_ne!(
            fp.edge_kind_set & extends_bit,
            0,
            "Extends bit should be set"
        );
        assert_ne!(
            fp.edge_kind_set & contains_bit,
            0,
            "Contains bit should be set"
        );

        // A variant not in the edges should NOT be set
        let imports_bit = 1u32 << edge_kind_index(&EdgeKind::ImportsFrom);
        assert_eq!(
            fp.edge_kind_set & imports_bit,
            0,
            "ImportsFrom bit should NOT be set"
        );
    }

    // -----------------------------------------------------------------------
    // T06 compare_pair and cluster_matches tests
    // -----------------------------------------------------------------------

    use crate::model::{CloneMatch, CloneType};

    #[test]
    fn compare_pair_type1_exact_match() {
        let src = "fn foo(x: i32, y: i32) -> i32 { x + y }";
        let result = compare_pair(src, src, false, 0.7);
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.clone_type, CloneType::Type1);
        assert!(m.similarity >= 0.95);
    }

    #[test]
    fn compare_pair_type2_renamed_vars() {
        let src_a = "fn add(x: i32, y: i32) -> i32 { x + y }";
        let src_b = "fn sum(a: i32, b: i32) -> i32 { a + b }";
        let result = compare_pair(src_a, src_b, false, 0.7);
        assert!(result.is_some());
        assert_eq!(result.unwrap().clone_type, CloneType::Type2);
    }

    #[test]
    fn compare_pair_cross_language_structural_only() {
        let result = compare_pair("", "", true, 0.7);
        assert!(result.is_some());
        assert_eq!(result.unwrap().clone_type, CloneType::StructuralOnly);
    }

    #[test]
    fn compare_pair_below_threshold_returns_none() {
        let src_a = "fn add(x: i32) -> i32 { x + 1 }";
        let src_b = "class Foo { bar: string; baz(): void { console.log(this.bar); } }";
        let result = compare_pair(src_a, src_b, false, 0.7);
        assert!(result.is_none());
    }

    #[test]
    fn cluster_transitive() {
        let matches = vec![
            CloneMatch {
                source: "a".into(),
                target: "b".into(),
                similarity: 0.8,
                clone_type: CloneType::Type2,
            },
            CloneMatch {
                source: "b".into(),
                target: "c".into(),
                similarity: 0.75,
                clone_type: CloneType::Type2,
            },
        ];
        let clusters = cluster_matches(&matches);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].members.len(), 3);
        assert_eq!(clusters[0].id, 1);
    }

    #[test]
    fn cluster_separate_components() {
        let matches = vec![
            CloneMatch {
                source: "a".into(),
                target: "b".into(),
                similarity: 0.8,
                clone_type: CloneType::Type1,
            },
            CloneMatch {
                source: "c".into(),
                target: "d".into(),
                similarity: 0.9,
                clone_type: CloneType::Type1,
            },
        ];
        let clusters = cluster_matches(&matches);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn cluster_empty_matches() {
        assert!(cluster_matches(&[]).is_empty());
    }

    #[test]
    fn cluster_ids_descending_by_size() {
        let matches = vec![
            CloneMatch {
                source: "a".into(),
                target: "b".into(),
                similarity: 0.8,
                clone_type: CloneType::Type2,
            },
            CloneMatch {
                source: "c".into(),
                target: "d".into(),
                similarity: 0.9,
                clone_type: CloneType::Type1,
            },
            CloneMatch {
                source: "c".into(),
                target: "e".into(),
                similarity: 0.85,
                clone_type: CloneType::Type1,
            },
        ];
        let clusters = cluster_matches(&matches);
        assert_eq!(clusters[0].members.len(), 3); // c,d,e
        assert_eq!(clusters[0].id, 1);
        assert_eq!(clusters[1].members.len(), 2); // a,b
        assert_eq!(clusters[1].id, 2);
    }
}
