use std::io::Write;

use domain::model::{
    AffectedNode, DiffImpactReport, GraphStats, ImpactReport, IndexStats, Reference, SearchResult,
    SymbolNode,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct FindResult {
    pub symbol: SymbolNode,
    pub callers: Vec<String>,
    pub callees: Vec<String>,
    pub tested_by: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Compact,
    Table,
    Json,
}

impl OutputFormat {
    pub fn from_flags(json: bool, table: bool) -> Self {
        if json {
            Self::Json
        } else if table {
            Self::Table
        } else {
            Self::Compact
        }
    }
}

pub trait Displayable {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()>;
    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()>;
    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()>;
}

pub fn print<T: Displayable>(value: &T, format: OutputFormat) {
    let stdout = std::io::stdout();
    let mut w = stdout.lock();
    match format {
        OutputFormat::Compact => value.fmt_compact(&mut w),
        OutputFormat::Table => value.fmt_table(&mut w),
        OutputFormat::Json => value.fmt_json(&mut w),
    }
    .expect("failed to write to stdout");
}

impl Displayable for IndexStats {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(
            w,
            "Indexed {} files, {} symbols, {} edges in {:.1}s",
            self.files_indexed,
            self.symbols_extracted,
            self.edges_created,
            self.duration.as_secs_f64()
        )
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Metric         | Count")?;
        writeln!(w, "---------------+----------")?;
        writeln!(w, "Files indexed  | {}", self.files_indexed)?;
        writeln!(w, "Symbols        | {}", self.symbols_extracted)?;
        writeln!(w, "Edges          | {}", self.edges_created)?;
        writeln!(w, "Duration       | {:.1}s", self.duration.as_secs_f64())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self)
            .map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: Vec<FindResult>
// ---------------------------------------------------------------------------

impl Displayable for Vec<FindResult> {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for fr in self {
            let s = &fr.symbol;
            let loc = &s.location;
            writeln!(
                w,
                "{} {:?} {}:{}-{}",
                s.name,
                s.kind,
                loc.file.display(),
                loc.line_start,
                loc.line_end
            )?;
            if !fr.callees.is_empty() {
                writeln!(w, "  -> calls: {}", fr.callees.join(", "))?;
            }
            if !fr.tested_by.is_empty() {
                writeln!(w, "  -> tested_by: {}", fr.tested_by.join(", "))?;
            }
            if !fr.callers.is_empty() {
                writeln!(w, "  <- callers: {}", fr.callers.join(", "))?;
            }
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Name | Kind | File | Lines | Callers | Callees")?;
        writeln!(w, "-----+------+------+-------+---------+--------")?;
        for fr in self {
            let s = &fr.symbol;
            let loc = &s.location;
            writeln!(
                w,
                "{} | {:?} | {} | {}-{} | {} | {}",
                s.name,
                s.kind,
                loc.file.display(),
                loc.line_start,
                loc.line_end,
                fr.callers.join(", "),
                fr.callees.join(", ")
            )?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json =
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: Vec<Reference>
// ---------------------------------------------------------------------------

impl Displayable for Vec<Reference> {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for r in self {
            writeln!(w, "{} ({:?})", r.source, r.edge_kind)?;
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Source | EdgeKind")?;
        writeln!(w, "-------+---------")?;
        for r in self {
            writeln!(w, "{} | {:?}", r.source, r.edge_kind)?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json =
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: Vec<SearchResult>
// ---------------------------------------------------------------------------

impl Displayable for Vec<SearchResult> {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for sr in self {
            writeln!(
                w,
                "{} {:?} {} score={:.2}",
                sr.qualified_name,
                sr.kind,
                sr.file_path.display(),
                sr.score
            )?;
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "QualifiedName | Kind | File | Score")?;
        writeln!(w, "--------------+------+------+------")?;
        for sr in self {
            writeln!(
                w,
                "{} | {:?} | {} | {:.2}",
                sr.qualified_name,
                sr.kind,
                sr.file_path.display(),
                sr.score
            )?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json =
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: GraphStats
// ---------------------------------------------------------------------------

impl Displayable for GraphStats {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(
            w,
            "Files: {} | Symbols: {} | Edges: {}",
            self.files, self.symbols, self.edges
        )
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Metric  | Count")?;
        writeln!(w, "--------+------")?;
        writeln!(w, "Files   | {}", self.files)?;
        writeln!(w, "Symbols | {}", self.symbols)?;
        writeln!(w, "Edges   | {}", self.edges)
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json =
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: ImpactReport
// ---------------------------------------------------------------------------

/// Helper: format affected nodes grouped by confidence, sorted by depth.
fn fmt_affected_compact(
    affected: &[AffectedNode],
    w: &mut dyn Write,
) -> std::io::Result<()> {
    let mut sorted: Vec<&AffectedNode> = affected.iter().collect();
    sorted.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(a.depth.cmp(&b.depth)));
    for node in sorted {
        if node.path.is_empty() {
            writeln!(
                w,
                "  [{:?}] {} (depth {})",
                node.confidence, node.qualified_name, node.depth
            )?;
        } else {
            writeln!(
                w,
                "  [{:?}] {} (depth {} via {})",
                node.confidence,
                node.qualified_name,
                node.depth,
                node.path.join(" -> ")
            )?;
        }
    }
    Ok(())
}

/// Helper: format affected nodes as table rows.
fn fmt_affected_table(
    affected: &[AffectedNode],
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(w, "QualifiedName | Depth | Confidence | Path")?;
    writeln!(w, "--------------+-------+------------+-----")?;
    let mut sorted: Vec<&AffectedNode> = affected.iter().collect();
    sorted.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(a.depth.cmp(&b.depth)));
    for node in sorted {
        writeln!(
            w,
            "{} | {} | {:?} | {}",
            node.qualified_name,
            node.depth,
            node.confidence,
            node.path.join(" -> ")
        )?;
    }
    Ok(())
}

impl Displayable for ImpactReport {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(
            w,
            "Impact: {} affected symbols (depth: {}, min_confidence: {:?})",
            self.affected.len(),
            self.depth,
            self.min_confidence
        )?;
        fmt_affected_compact(&self.affected, w)
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        fmt_affected_table(&self.affected, w)
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json =
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: DiffImpactReport
// ---------------------------------------------------------------------------

impl Displayable for DiffImpactReport {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Changed symbols ({}):", self.changed_symbols.len())?;
        for s in &self.changed_symbols {
            let loc = &s.location;
            writeln!(
                w,
                "  {} {:?} {}:{}-{}",
                s.name,
                s.kind,
                loc.file.display(),
                loc.line_start,
                loc.line_end
            )?;
        }
        writeln!(w, "Impact:")?;
        writeln!(
            w,
            "  {} affected symbols (depth: {}, min_confidence: {:?})",
            self.impact.affected.len(),
            self.impact.depth,
            self.impact.min_confidence
        )?;
        fmt_affected_compact(&self.impact.affected, w)
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Changed Symbols:")?;
        writeln!(w, "Name | Kind | File | Lines")?;
        writeln!(w, "-----+------+------+------")?;
        for s in &self.changed_symbols {
            let loc = &s.location;
            writeln!(
                w,
                "{} | {:?} | {} | {}-{}",
                s.name,
                s.kind,
                loc.file.display(),
                loc.line_start,
                loc.line_end
            )?;
        }
        writeln!(w)?;
        writeln!(w, "Impact:")?;
        fmt_affected_table(&self.impact.affected, w)
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json =
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::model::{Confidence, EdgeKind, Location, SymbolKind, Visibility};
    use std::time::Duration;

    fn sample_stats() -> IndexStats {
        IndexStats {
            files_indexed: 42,
            symbols_extracted: 128,
            edges_created: 256,
            duration: Duration::from_secs_f64(1.5),
        }
    }

    #[test]
    fn output_format_from_flags_json() {
        assert_eq!(OutputFormat::from_flags(true, false), OutputFormat::Json);
    }

    #[test]
    fn output_format_from_flags_table() {
        assert_eq!(OutputFormat::from_flags(false, true), OutputFormat::Table);
    }

    #[test]
    fn output_format_from_flags_compact() {
        assert_eq!(OutputFormat::from_flags(false, false), OutputFormat::Compact);
    }

    #[test]
    fn output_format_json_takes_precedence() {
        assert_eq!(OutputFormat::from_flags(true, true), OutputFormat::Json);
    }

    #[test]
    fn index_stats_compact_format() {
        let stats = sample_stats();
        let mut buf = Vec::new();
        stats.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("42 files"));
        assert!(s.contains("128 symbols"));
        assert!(s.contains("256 edges"));
        assert!(s.contains("1.5s"));
    }

    #[test]
    fn index_stats_json_format() {
        let stats = sample_stats();
        let mut buf = Vec::new();
        stats.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["files_indexed"], 42);
        assert_eq!(parsed["symbols_extracted"], 128);
        assert_eq!(parsed["edges_created"], 256);
    }

    #[test]
    fn index_stats_table_format() {
        let stats = sample_stats();
        let mut buf = Vec::new();
        stats.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Files indexed"));
        assert!(s.contains("42"));
        assert!(s.contains("Symbols"));
        assert!(s.contains("128"));
    }

    // -----------------------------------------------------------------------
    // Helpers for new types
    // -----------------------------------------------------------------------

    fn sample_symbol() -> SymbolNode {
        SymbolNode {
            name: "foo".into(),
            qualified_name: "src/lib.rs::foo".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/lib.rs".into(),
                line_start: 10,
                line_end: 20,
                col_start: 0,
                col_end: 1,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    fn sample_find_results() -> Vec<FindResult> {
        vec![FindResult {
            symbol: sample_symbol(),
            callers: vec!["bar".into()],
            callees: vec!["baz".into(), "qux".into()],
            tested_by: vec!["test_foo".into()],
        }]
    }

    // -----------------------------------------------------------------------
    // Vec<FindResult> tests
    // -----------------------------------------------------------------------

    #[test]
    fn find_result_compact_format() {
        let results = sample_find_results();
        let mut buf = Vec::new();
        results.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("foo Function src/lib.rs:10-20"));
        assert!(s.contains("-> calls: baz, qux"));
        assert!(s.contains("-> tested_by: test_foo"));
        assert!(s.contains("<- callers: bar"));
    }

    #[test]
    fn find_result_json_format() {
        let results = sample_find_results();
        let mut buf = Vec::new();
        results.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["symbol"]["name"], "foo");
        assert_eq!(parsed[0]["callers"][0], "bar");
        assert_eq!(parsed[0]["callees"][0], "baz");
    }

    #[test]
    fn find_result_table_format() {
        let results = sample_find_results();
        let mut buf = Vec::new();
        results.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Name | Kind | File | Lines | Callers | Callees"));
        assert!(s.contains("foo"));
        assert!(s.contains("bar"));
        assert!(s.contains("baz, qux"));
    }

    #[test]
    fn find_result_compact_empty_relations() {
        let results = vec![FindResult {
            symbol: sample_symbol(),
            callers: vec![],
            callees: vec![],
            tested_by: vec![],
        }];
        let mut buf = Vec::new();
        results.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("foo Function src/lib.rs:10-20"));
        assert!(!s.contains("-> calls:"));
        assert!(!s.contains("-> tested_by:"));
        assert!(!s.contains("<- callers:"));
    }

    // -----------------------------------------------------------------------
    // Vec<Reference> tests
    // -----------------------------------------------------------------------

    fn sample_references() -> Vec<Reference> {
        vec![
            Reference {
                source: "src/lib.rs::bar".into(),
                edge_kind: EdgeKind::Calls,
                location: None,
            },
            Reference {
                source: "src/lib.rs::baz".into(),
                edge_kind: EdgeKind::ImportsFrom,
                location: None,
            },
        ]
    }

    #[test]
    fn reference_compact_format() {
        let refs = sample_references();
        let mut buf = Vec::new();
        refs.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("src/lib.rs::bar (Calls)"));
        assert!(s.contains("src/lib.rs::baz (ImportsFrom)"));
    }

    #[test]
    fn reference_json_format() {
        let refs = sample_references();
        let mut buf = Vec::new();
        refs.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["source"], "src/lib.rs::bar");
        assert_eq!(parsed[0]["edge_kind"], "Calls");
    }

    #[test]
    fn reference_table_format() {
        let refs = sample_references();
        let mut buf = Vec::new();
        refs.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Source | EdgeKind"));
        assert!(s.contains("src/lib.rs::bar | Calls"));
    }

    // -----------------------------------------------------------------------
    // Vec<SearchResult> tests
    // -----------------------------------------------------------------------

    fn sample_search_results() -> Vec<SearchResult> {
        vec![SearchResult {
            qualified_name: "src/lib.rs::foo".into(),
            name: "foo".into(),
            kind: SymbolKind::Function,
            file_path: "src/lib.rs".into(),
            score: 0.95,
        }]
    }

    #[test]
    fn search_result_compact_format() {
        let results = sample_search_results();
        let mut buf = Vec::new();
        results.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("src/lib.rs::foo Function src/lib.rs score=0.95"));
    }

    #[test]
    fn search_result_json_format() {
        let results = sample_search_results();
        let mut buf = Vec::new();
        results.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["qualified_name"], "src/lib.rs::foo");
        assert_eq!(parsed[0]["score"], 0.95);
    }

    #[test]
    fn search_result_table_format() {
        let results = sample_search_results();
        let mut buf = Vec::new();
        results.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("QualifiedName | Kind | File | Score"));
        assert!(s.contains("src/lib.rs::foo | Function"));
    }

    // -----------------------------------------------------------------------
    // GraphStats tests
    // -----------------------------------------------------------------------

    fn sample_graph_stats() -> GraphStats {
        GraphStats {
            files: 10,
            symbols: 50,
            edges: 100,
        }
    }

    #[test]
    fn graph_stats_compact_format() {
        let stats = sample_graph_stats();
        let mut buf = Vec::new();
        stats.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Files: 10"));
        assert!(s.contains("Symbols: 50"));
        assert!(s.contains("Edges: 100"));
    }

    #[test]
    fn graph_stats_json_format() {
        let stats = sample_graph_stats();
        let mut buf = Vec::new();
        stats.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["files"], 10);
        assert_eq!(parsed["symbols"], 50);
        assert_eq!(parsed["edges"], 100);
    }

    #[test]
    fn graph_stats_table_format() {
        let stats = sample_graph_stats();
        let mut buf = Vec::new();
        stats.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Metric"));
        assert!(s.contains("Files"));
        assert!(s.contains("10"));
        assert!(s.contains("Symbols"));
        assert!(s.contains("50"));
    }

    // -----------------------------------------------------------------------
    // ImpactReport tests
    // -----------------------------------------------------------------------

    fn sample_impact_report() -> ImpactReport {
        ImpactReport {
            targets: vec![],
            affected: vec![
                AffectedNode {
                    qualified_name: "src/a.rs::alpha".into(),
                    depth: 1,
                    confidence: Confidence::High,
                    path: vec!["foo".into(), "alpha".into()],
                },
                AffectedNode {
                    qualified_name: "src/b.rs::beta".into(),
                    depth: 2,
                    confidence: Confidence::Medium,
                    path: vec![],
                },
            ],
            depth: 3,
            min_confidence: Confidence::Medium,
        }
    }

    #[test]
    fn impact_report_compact_format() {
        let report = sample_impact_report();
        let mut buf = Vec::new();
        report.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Impact: 2 affected symbols (depth: 3, min_confidence: Medium)"));
        assert!(s.contains("[High] src/a.rs::alpha (depth 1 via foo -> alpha)"));
        assert!(s.contains("[Medium] src/b.rs::beta (depth 2)"));
    }

    #[test]
    fn impact_report_json_format() {
        let report = sample_impact_report();
        let mut buf = Vec::new();
        report.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["depth"], 3);
        assert_eq!(parsed["affected"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["affected"][0]["qualified_name"], "src/a.rs::alpha");
    }

    #[test]
    fn impact_report_table_format() {
        let report = sample_impact_report();
        let mut buf = Vec::new();
        report.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("QualifiedName | Depth | Confidence | Path"));
        assert!(s.contains("src/a.rs::alpha | 1 | High"));
        assert!(s.contains("src/b.rs::beta | 2 | Medium"));
    }

    #[test]
    fn impact_report_sorted_by_confidence_then_depth() {
        let report = ImpactReport {
            targets: vec![],
            affected: vec![
                AffectedNode {
                    qualified_name: "low_deep".into(),
                    depth: 5,
                    confidence: Confidence::Low,
                    path: vec![],
                },
                AffectedNode {
                    qualified_name: "high_shallow".into(),
                    depth: 1,
                    confidence: Confidence::High,
                    path: vec![],
                },
                AffectedNode {
                    qualified_name: "high_deep".into(),
                    depth: 3,
                    confidence: Confidence::High,
                    path: vec![],
                },
            ],
            depth: 5,
            min_confidence: Confidence::Low,
        };
        let mut buf = Vec::new();
        report.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let high_shallow_pos = s.find("high_shallow").unwrap();
        let high_deep_pos = s.find("high_deep").unwrap();
        let low_deep_pos = s.find("low_deep").unwrap();
        assert!(high_shallow_pos < high_deep_pos, "High confidence should come before lower");
        assert!(high_deep_pos < low_deep_pos, "High confidence should come before Low");
    }

    // -----------------------------------------------------------------------
    // DiffImpactReport tests
    // -----------------------------------------------------------------------

    fn sample_diff_impact_report() -> DiffImpactReport {
        DiffImpactReport {
            changed_symbols: vec![sample_symbol()],
            impact: sample_impact_report(),
        }
    }

    #[test]
    fn diff_impact_report_compact_format() {
        let report = sample_diff_impact_report();
        let mut buf = Vec::new();
        report.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Changed symbols (1):"));
        assert!(s.contains("foo Function src/lib.rs:10-20"));
        assert!(s.contains("Impact:"));
        assert!(s.contains("2 affected symbols"));
        assert!(s.contains("[High] src/a.rs::alpha"));
    }

    #[test]
    fn diff_impact_report_json_format() {
        let report = sample_diff_impact_report();
        let mut buf = Vec::new();
        report.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["changed_symbols"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["changed_symbols"][0]["name"], "foo");
        assert_eq!(parsed["impact"]["depth"], 3);
    }

    #[test]
    fn diff_impact_report_table_format() {
        let report = sample_diff_impact_report();
        let mut buf = Vec::new();
        report.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Changed Symbols:"));
        assert!(s.contains("Name | Kind | File | Lines"));
        assert!(s.contains("foo"));
        assert!(s.contains("Impact:"));
        assert!(s.contains("QualifiedName | Depth | Confidence | Path"));
    }
}
