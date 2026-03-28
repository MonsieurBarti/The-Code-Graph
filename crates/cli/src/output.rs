use std::io::Write;

use domain::model::{
    AffectedNode, CloneAnalysis, CloneCluster, CriticalityScore, DiffImpactReport, EntryPointKind,
    FlowAnalysis, GraphStats, ImpactReport, IndexStats, Reference, SearchResult, SymbolNode,
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
        let json = serde_json::to_string_pretty(&self).map_err(std::io::Error::other)?;
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
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: Vec<Reference>
// ---------------------------------------------------------------------------

impl Displayable for Vec<Reference> {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for r in self {
            writeln!(w, "{} ({:?})", r.symbol, r.edge_kind)?;
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Symbol | EdgeKind")?;
        writeln!(w, "-------+---------")?;
        for r in self {
            writeln!(w, "{} | {:?}", r.symbol, r.edge_kind)?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
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
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: GraphStats
// ---------------------------------------------------------------------------

impl Displayable for GraphStats {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        write!(
            w,
            "Files: {} | Symbols: {} | Edges: {}",
            self.files, self.symbols, self.edges
        )?;
        if let Some(ep) = self.entry_point_count {
            write!(w, "\nEntry points: {ep}")?;
            if let Some(ac) = self.avg_criticality {
                write!(w, " | Avg criticality: {ac:.3}")?;
            }
        }
        if let Some(cc) = self.clone_clusters {
            write!(w, "\nClone clusters: {cc}")?;
            if let Some(dp) = self.duplication_pct {
                write!(w, " | Duplication: {dp:.1}%")?;
            }
            if let Some(ref md) = self.most_duplicated {
                write!(w, " | Most duplicated: {md}")?;
            }
        }
        writeln!(w)
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Metric  | Count")?;
        writeln!(w, "--------+------")?;
        writeln!(w, "Files   | {}", self.files)?;
        writeln!(w, "Symbols | {}", self.symbols)?;
        writeln!(w, "Edges   | {}", self.edges)?;
        if let Some(ep) = self.entry_point_count {
            writeln!(w, "Entry pts | {ep}")?;
        }
        if let Some(ac) = self.avg_criticality {
            writeln!(w, "Avg crit  | {ac:.3}")?;
        }
        if let Some(cc) = self.clone_clusters {
            writeln!(w, "Clones    | {cc} clusters")?;
        }
        if let Some(dp) = self.duplication_pct {
            writeln!(w, "Dupl %    | {dp:.1}%")?;
        }
        if let Some(ref md) = self.most_duplicated {
            writeln!(w, "Most dupl | {md}")?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: FlowAnalysis
// ---------------------------------------------------------------------------

fn format_entry_point_summary(analysis: &FlowAnalysis) -> String {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for ep in &analysis.entry_points {
        let label = match ep.kind {
            EntryPointKind::Main => "main",
            EntryPointKind::Test => "test",
            EntryPointKind::HttpHandler => "http",
            EntryPointKind::CliCommand => "cli",
            EntryPointKind::PublicRoot => "public-root",
        };
        *counts.entry(label).or_default() += 1;
    }
    let parts: Vec<String> = counts.iter().map(|(k, v)| format!("{v} {k}")).collect();
    parts.join(", ")
}

impl Displayable for FlowAnalysis {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let summary = format_entry_point_summary(self);
        writeln!(
            w,
            "Entry points: {} detected ({})",
            self.stats.total_entry_points, summary
        )?;
        writeln!(
            w,
            "Flows: {} total, showing {}",
            self.stats.total_flows,
            self.flows.len()
        )?;
        writeln!(w)?;
        for (i, flow) in self.flows.iter().enumerate() {
            let path_str = flow.path.join(" -> ");
            let truncated = if flow.truncated { " [truncated]" } else { "" };
            writeln!(
                w,
                "[{}] {} (depth {}){}",
                i + 1,
                path_str,
                flow.depth,
                truncated
            )?;
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "# | Entry | Path | Depth | Truncated")?;
        writeln!(w, "--+-------+------+-------+----------")?;
        for (i, flow) in self.flows.iter().enumerate() {
            writeln!(
                w,
                "{} | {} | {} | {} | {}",
                i + 1,
                flow.entry,
                flow.path.join(" -> "),
                flow.depth,
                flow.truncated
            )?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: Vec<CriticalityScore>
// ---------------------------------------------------------------------------

impl Displayable for Vec<CriticalityScore> {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for (i, score) in self.iter().enumerate() {
            let entry = if score.is_entry_point { "yes" } else { "no" };
            writeln!(
                w,
                "{} {}  betweenness={:.3}  flows={}  entry={}",
                i + 1,
                score.qualified_name,
                score.betweenness,
                score.flow_count,
                entry
            )?;
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "# | Symbol | Betweenness | Flows | Entry?")?;
        writeln!(w, "--+--------+-------------+-------+-------")?;
        for (i, score) in self.iter().enumerate() {
            let entry = if score.is_entry_point { "yes" } else { "no" };
            writeln!(
                w,
                "{} | {} | {:.3} | {} | {}",
                i + 1,
                score.qualified_name,
                score.betweenness,
                score.flow_count,
                entry
            )?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: ImpactReport
// ---------------------------------------------------------------------------

/// Helper: format affected nodes grouped by confidence, sorted by depth.
fn fmt_affected_compact(affected: &[AffectedNode], w: &mut dyn Write) -> std::io::Result<()> {
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
fn fmt_affected_table(affected: &[AffectedNode], w: &mut dyn Write) -> std::io::Result<()> {
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
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
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
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: CloneAnalysis
// ---------------------------------------------------------------------------

impl Displayable for CloneAnalysis {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(
            w,
            "{} clone clusters, {:.1}% duplication ({}/{} symbols)",
            self.clusters.len(),
            self.duplication_pct,
            self.symbols_in_clones,
            self.total_symbols_analyzed
        )?;
        if let Some(ref most) = self.most_duplicated {
            writeln!(w, "most duplicated: {most}")?;
        }
        writeln!(w)?;
        for cluster in &self.clusters {
            writeln!(
                w,
                "#{} {:?}  members={}  avg_sim={:.2}  [{}]",
                cluster.id,
                cluster.clone_type,
                cluster.members.len(),
                cluster.avg_similarity,
                cluster.members.join(", ")
            )?;
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(
            w,
            "Clone Analysis: {} clusters, {:.1}% duplication",
            self.clusters.len(),
            self.duplication_pct
        )?;
        writeln!(w)?;
        writeln!(w, "# | Type | Members | Avg Similarity | Representative")?;
        writeln!(w, "--+------+---------+----------------+---------------")?;
        for c in &self.clusters {
            writeln!(
                w,
                "{} | {:?} | {} | {:.3} | {}",
                c.id,
                c.clone_type,
                c.members.len(),
                c.avg_similarity,
                c.representative
            )?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

// ---------------------------------------------------------------------------
// Displayable: Vec<CloneCluster>
// ---------------------------------------------------------------------------

impl Displayable for Vec<CloneCluster> {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for cluster in self {
            writeln!(
                w,
                "Cluster #{} ({:?}, avg_sim={:.2}):",
                cluster.id, cluster.clone_type, cluster.avg_similarity
            )?;
            for member in &cluster.members {
                // Extract file path from qualified name (e.g., "src/foo.rs::bar" -> "src/foo.rs")
                let file = member.split("::").next().unwrap_or(member);
                writeln!(w, "  {member}  ({file})")?;
            }
            if !cluster.intra_matches.is_empty() {
                writeln!(w, "  pairs:")?;
                for m in &cluster.intra_matches {
                    writeln!(
                        w,
                        "    {} <-> {}  sim={:.3} ({:?})",
                        m.source, m.target, m.similarity, m.clone_type
                    )?;
                }
            }
        }
        Ok(())
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for cluster in self {
            writeln!(
                w,
                "Cluster #{} — {:?} — avg similarity: {:.3}",
                cluster.id, cluster.clone_type, cluster.avg_similarity
            )?;
            writeln!(w, "Member | File")?;
            writeln!(w, "-------+-----")?;
            for member in &cluster.members {
                let file = member.split("::").next().unwrap_or(member);
                writeln!(w, "{member} | {file}")?;
            }
            if !cluster.intra_matches.is_empty() {
                writeln!(w)?;
                writeln!(w, "Source | Target | Similarity | Type")?;
                writeln!(w, "-------+--------+------------+-----")?;
                for m in &cluster.intra_matches {
                    writeln!(
                        w,
                        "{} | {} | {:.3} | {:?}",
                        m.source, m.target, m.similarity, m.clone_type
                    )?;
                }
            }
            writeln!(w)?;
        }
        Ok(())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
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
        assert_eq!(
            OutputFormat::from_flags(false, false),
            OutputFormat::Compact
        );
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
                symbol: "src/lib.rs::bar".into(),
                edge_kind: EdgeKind::Calls,
                location: None,
            },
            Reference {
                symbol: "src/lib.rs::baz".into(),
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
        assert_eq!(parsed[0]["symbol"], "src/lib.rs::bar");
        assert_eq!(parsed[0]["edge_kind"], "Calls");
    }

    #[test]
    fn reference_table_format() {
        let refs = sample_references();
        let mut buf = Vec::new();
        refs.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Symbol | EdgeKind"));
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
            entry_point_count: None,
            avg_criticality: None,
            clone_clusters: None,
            duplication_pct: None,
            most_duplicated: None,
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
        assert!(
            high_shallow_pos < high_deep_pos,
            "High confidence should come before lower"
        );
        assert!(
            high_deep_pos < low_deep_pos,
            "High confidence should come before Low"
        );
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

    // -----------------------------------------------------------------------
    // FlowAnalysis tests
    // -----------------------------------------------------------------------

    fn sample_flow_analysis() -> FlowAnalysis {
        use domain::model::{EntryPoint, ExecutionFlow, FlowStats};
        FlowAnalysis {
            entry_points: vec![EntryPoint {
                qualified_name: "main".into(),
                kind: EntryPointKind::Main,
                confidence: 1.0,
            }],
            flows: vec![ExecutionFlow {
                entry: "main".into(),
                path: vec!["main".into(), "db.connect".into()],
                depth: 2,
                truncated: false,
            }],
            criticality: vec![],
            stats: FlowStats {
                total_entry_points: 1,
                total_flows: 1,
                max_depth: 2,
                avg_depth: 2.0,
            },
        }
    }

    fn sample_criticality() -> Vec<CriticalityScore> {
        vec![CriticalityScore {
            qualified_name: "db.query".into(),
            betweenness: 0.847,
            flow_count: 312,
            is_entry_point: false,
        }]
    }

    #[test]
    fn flow_analysis_compact_format() {
        let analysis = sample_flow_analysis();
        let mut buf = Vec::new();
        analysis.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Entry points: 1"));
        assert!(s.contains("main"));
        assert!(s.contains("db.connect"));
    }

    #[test]
    fn flow_analysis_table_format() {
        let analysis = sample_flow_analysis();
        let mut buf = Vec::new();
        analysis.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Entry"));
        assert!(s.contains("Path"));
        assert!(s.contains("main"));
    }

    #[test]
    fn flow_analysis_json_format() {
        let analysis = sample_flow_analysis();
        let mut buf = Vec::new();
        analysis.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["stats"]["total_flows"], 1);
        assert_eq!(parsed["flows"][0]["entry"], "main");
    }

    #[test]
    fn criticality_compact_format() {
        let scores = sample_criticality();
        let mut buf = Vec::new();
        scores.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("db.query"));
        assert!(s.contains("0.847"));
    }

    #[test]
    fn criticality_table_format() {
        let scores = sample_criticality();
        let mut buf = Vec::new();
        scores.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Symbol"));
        assert!(s.contains("Betweenness"));
        assert!(s.contains("db.query"));
        assert!(s.contains("0.847"));
        assert!(s.contains("312"));
    }

    #[test]
    fn criticality_json_format() {
        let scores = sample_criticality();
        let mut buf = Vec::new();
        scores.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["qualified_name"], "db.query");
        assert_eq!(parsed[0]["betweenness"], 0.847);
    }

    // -----------------------------------------------------------------------
    // GraphStats with flow fields tests (T07)
    // -----------------------------------------------------------------------

    #[test]
    fn graph_stats_compact_with_flow_fields() {
        let stats = GraphStats {
            files: 234,
            symbols: 1892,
            edges: 5431,
            entry_point_count: Some(12),
            avg_criticality: Some(0.034),
            clone_clusters: None,
            duplication_pct: None,
            most_duplicated: None,
        };
        let mut buf = Vec::new();
        stats.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Entry points: 12"));
        assert!(s.contains("Avg criticality: 0.034"));
    }

    #[test]
    fn graph_stats_compact_without_flow_fields() {
        let stats = GraphStats {
            files: 10,
            symbols: 50,
            edges: 100,
            entry_point_count: None,
            avg_criticality: None,
            clone_clusters: None,
            duplication_pct: None,
            most_duplicated: None,
        };
        let mut buf = Vec::new();
        stats.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(!s.contains("Entry points"));
        assert!(!s.contains("Avg criticality"));
    }

    #[test]
    fn graph_stats_zero_symbols_shows_zero_flow_fields() {
        let stats = GraphStats {
            files: 0,
            symbols: 0,
            edges: 0,
            entry_point_count: Some(0),
            avg_criticality: Some(0.0),
            clone_clusters: None,
            duplication_pct: None,
            most_duplicated: None,
        };
        let mut buf = Vec::new();
        stats.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Entry points: 0"));
    }
}
