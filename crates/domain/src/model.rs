use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Borrow;
use std::fmt;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    TypeScript,
    JavaScript,
    Rust,
    Python,
    Go,
}

impl Language {
    /// Derive the language from a file path based on extension.
    /// Falls back to `Rust` for unknown extensions.
    pub fn from_path(path: &std::path::Path) -> Language {
        match path.extension().and_then(|e| e.to_str()) {
            Some("ts") | Some("tsx") => Language::TypeScript,
            Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Language::JavaScript,
            Some("rs") => Language::Rust,
            Some("py") => Language::Python,
            Some("go") => Language::Go,
            _ => Language::Rust,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Symbol,
    NonParsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Class,
    Interface,
    Struct,
    Trait,
    Enum,
    TypeAlias,
    Method,
    Property,
    Const,
    Macro,
    Variable,
    Component,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NonParsedKind {
    Doc,
    Config,
    CI,
    Asset,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Crate,
}

/// Variant declaration order is load-bearing for PartialOrd/Ord.
/// Structural < Low < Medium < High
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Confidence {
    Structural,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Contains,
    ChildOf,
    Calls,
    ImportsFrom,
    Extends,
    Implements,
    TestedBy,
    DependsOn,
    BarrelReExportAll,
    ConditionalImport,
    SideEffectImport,
    DotImport,
    HasDecorator,
    Embeds,
    TypeReference,
    ReExport,
}

impl EdgeKind {
    pub fn confidence(&self) -> Confidence {
        match self {
            EdgeKind::Calls | EdgeKind::Extends | EdgeKind::Implements | EdgeKind::Embeds => {
                Confidence::High
            }
            EdgeKind::ImportsFrom
            | EdgeKind::BarrelReExportAll
            | EdgeKind::ReExport
            | EdgeKind::TypeReference
            | EdgeKind::DotImport => Confidence::Medium,
            EdgeKind::DependsOn | EdgeKind::ConditionalImport | EdgeKind::SideEffectImport => {
                Confidence::Low
            }
            EdgeKind::Contains
            | EdgeKind::ChildOf
            | EdgeKind::HasDecorator
            | EdgeKind::TestedBy => Confidence::Structural,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Forward,
    Backward,
}

// ---------------------------------------------------------------------------
// Core structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub file: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub path: PathBuf,
    pub language: Language,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolNode {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub location: Location,
    pub visibility: Visibility,
    pub is_exported: bool,
    pub is_async: bool,
    pub is_test: bool,
    pub decorators: Vec<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonParsedNode {
    pub path: PathBuf,
    pub file_kind: NonParsedKind,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    File(FileNode),
    Symbol(SymbolNode),
    NonParsed(NonParsedNode),
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Node::File(f) => f.path.to_str().unwrap_or_default(),
            Node::Symbol(s) => &s.qualified_name,
            Node::NonParsed(n) => n.path.to_str().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub kind: EdgeKind,
    pub source: String,
    pub target: String,
    pub metadata: Option<String>,
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImpactTarget {
    File(PathBuf),
    Symbol(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalResult {
    pub node: String,
    pub depth: usize,
    pub path: Vec<String>,
    pub edge_kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub qualified_name: String,
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: PathBuf,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub symbol: String,
    pub edge_kind: EdgeKind,
    pub location: Option<Location>,
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(d)?;
        Ok(Duration::from_millis(millis))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub symbols_extracted: usize,
    pub edges_created: usize,
    #[serde(with = "duration_millis")]
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub files: usize,
    pub symbols: usize,
    pub edges: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_criticality: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clone_clusters: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplication_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub most_duplicated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_risk: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p90_risk: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub file: PathBuf,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedNode {
    pub qualified_name: String,
    pub depth: usize,
    pub confidence: Confidence,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactReport {
    pub targets: Vec<ImpactTarget>,
    pub affected: Vec<AffectedNode>,
    pub depth: usize,
    pub min_confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffImpactReport {
    pub changed_symbols: Vec<SymbolNode>,
    pub impact: ImpactReport,
}

// ---------------------------------------------------------------------------
// Flow analysis types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    pub qualified_name: String,
    pub kind: EntryPointKind,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryPointKind {
    Main,
    Test,
    HttpHandler,
    CliCommand,
    PublicRoot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionFlow {
    pub entry: String,
    pub path: Vec<String>,
    pub depth: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalityScore {
    pub qualified_name: String,
    pub betweenness: f64,
    pub flow_count: usize,
    pub is_entry_point: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowAnalysis {
    pub entry_points: Vec<EntryPoint>,
    pub flows: Vec<ExecutionFlow>,
    pub criticality: Vec<CriticalityScore>,
    pub stats: FlowStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStats {
    pub total_entry_points: usize,
    pub total_flows: usize,
    pub max_depth: usize,
    pub avg_depth: f64,
}

#[derive(Debug, Clone)]
pub struct FlowConfig {
    pub max_depth: usize,
    pub max_flows: usize,
    pub visit_budget: usize,
    pub max_public_roots: usize,
    pub extra_entry_points: Vec<String>,
    pub excluded_entry_points: Vec<String>,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            max_depth: 20,
            max_flows: 1000,
            visit_budget: 100_000,
            max_public_roots: 50,
            extra_entry_points: Vec::new(),
            excluded_entry_points: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Clone detection types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CloneType {
    Type1,
    Type2,
    StructuralOnly,
}

#[derive(Debug, Clone)]
pub struct StructuralFingerprint {
    pub qualified_name: String,
    pub symbol_kind: SymbolKind,
    pub callee_count: usize,
    pub caller_count: usize,
    pub edge_kind_set: u32,
    pub body_line_count: usize,
    pub child_count: usize,
    pub language: Language,
    pub file: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BucketKey {
    pub kind: SymbolKind,
    pub callee_bin: u8,
    pub caller_bin: u8,
    pub line_bin: u8,
    pub child_bin: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneMatch {
    pub source: String,
    pub target: String,
    pub similarity: f64,
    pub clone_type: CloneType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneCluster {
    pub id: usize,
    pub members: Vec<String>,
    pub avg_similarity: f64,
    pub clone_type: CloneType,
    pub representative: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub intra_matches: Vec<CloneMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneAnalysis {
    pub clusters: Vec<CloneCluster>,
    pub total_symbols_analyzed: usize,
    pub symbols_in_clones: usize,
    pub duplication_pct: f64,
    pub most_duplicated: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CloneConfig {
    pub threshold: f64,
    pub min_lines: usize,
    pub max_candidates_per_bucket: usize,
}

impl Default for CloneConfig {
    fn default() -> Self {
        Self {
            threshold: 0.7,
            min_lines: 5,
            max_candidates_per_bucket: 500,
        }
    }
}

// ---------------------------------------------------------------------------
// Risk scoring types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RiskWeights {
    pub criticality: f64,
    pub coupling: f64,
    pub test_gap: f64,
    pub sensitivity: f64,
}

impl Default for RiskWeights {
    fn default() -> Self {
        Self {
            criticality: 0.30,
            coupling: 0.25,
            test_gap: 0.25,
            sensitivity: 0.20,
        }
    }
}

impl RiskWeights {
    /// Normalize weights so they sum to 1.0, preserving relative proportions.
    pub fn normalized(&self) -> Self {
        let sum = self.criticality + self.coupling + self.test_gap + self.sensitivity;
        if sum == 0.0 {
            return Self::default();
        }
        Self {
            criticality: self.criticality / sum,
            coupling: self.coupling / sum,
            test_gap: self.test_gap / sum,
            sensitivity: self.sensitivity / sum,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RiskConfig {
    pub weights: RiskWeights,
    pub security_patterns: Vec<String>,
    pub min_score: f64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            weights: RiskWeights::default(),
            security_patterns: vec![
                "auth".into(),
                "password".into(),
                "secret".into(),
                "token".into(),
                "crypto".into(),
                "credential".into(),
                "sql".into(),
                "exec".into(),
                "eval".into(),
                "unsafe".into(),
            ],
            min_score: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactors {
    pub criticality: f64,
    pub coupling: f64,
    pub test_gap: f64,
    pub sensitivity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScore {
    pub qualified_name: String,
    pub composite: f64,
    pub factors: RiskFactors,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRiskScore {
    pub path: PathBuf,
    pub composite: f64,
    pub symbol_count: usize,
    pub highest_symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskStats {
    pub symbols_scored: usize,
    pub files_scored: usize,
    pub avg_risk: f64,
    pub median_risk: f64,
    pub p90_risk: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAnalysis {
    pub symbol_scores: Vec<RiskScore>,
    pub file_scores: Vec<FileRiskScore>,
    pub stats: RiskStats,
}

// ---------------------------------------------------------------------------
// QualifiedName newtype
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QualifiedName(String);

impl QualifiedName {
    pub fn parse(s: &str) -> crate::error::Result<Self> {
        if s.is_empty() {
            return Err(crate::error::CodeGraphError::Resolution(
                "qualified name must not be empty".into(),
            ));
        }
        let sep = "::";
        let idx = s.find(sep).ok_or_else(|| {
            crate::error::CodeGraphError::Resolution(format!(
                "qualified name must contain '::' separator: {s}"
            ))
        })?;
        let file = &s[..idx];
        let symbol = &s[idx + sep.len()..];
        if file.is_empty() {
            return Err(crate::error::CodeGraphError::Resolution(
                "file path part of qualified name must not be empty".into(),
            ));
        }
        if symbol.is_empty() {
            return Err(crate::error::CodeGraphError::Resolution(
                "symbol path part of qualified name must not be empty".into(),
            ));
        }
        Ok(QualifiedName(s.to_owned()))
    }

    pub fn file_path(&self) -> &str {
        self.0.split("::").next().unwrap_or_default()
    }

    pub fn symbol_path(&self) -> &str {
        self.0.split_once("::").map(|(_, s)| s).unwrap_or_default()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for QualifiedName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for QualifiedName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<QualifiedName> for String {
    fn from(qn: QualifiedName) -> String {
        qn.0
    }
}

impl Serialize for QualifiedName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for QualifiedName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        QualifiedName::parse(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_ordering() {
        assert!(Confidence::Structural < Confidence::Low);
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
    }

    #[test]
    fn all_16_edge_kinds_have_confidence() {
        let edges = [
            (EdgeKind::Calls, Confidence::High),
            (EdgeKind::Extends, Confidence::High),
            (EdgeKind::Implements, Confidence::High),
            (EdgeKind::Embeds, Confidence::High),
            (EdgeKind::ImportsFrom, Confidence::Medium),
            (EdgeKind::BarrelReExportAll, Confidence::Medium),
            (EdgeKind::ReExport, Confidence::Medium),
            (EdgeKind::TypeReference, Confidence::Medium),
            (EdgeKind::DotImport, Confidence::Medium),
            (EdgeKind::DependsOn, Confidence::Low),
            (EdgeKind::ConditionalImport, Confidence::Low),
            (EdgeKind::SideEffectImport, Confidence::Low),
            (EdgeKind::Contains, Confidence::Structural),
            (EdgeKind::ChildOf, Confidence::Structural),
            (EdgeKind::HasDecorator, Confidence::Structural),
            (EdgeKind::TestedBy, Confidence::Structural),
        ];
        for (kind, expected) in &edges {
            assert_eq!(
                kind.confidence(),
                *expected,
                "wrong confidence for {kind:?}"
            );
        }
        assert_eq!(edges.len(), 16, "expected 16 edge kinds");
    }

    #[test]
    fn qualified_name_parse_valid() {
        let qn = QualifiedName::parse("src/file.rs::MyStruct.method").unwrap();
        assert_eq!(qn.file_path(), "src/file.rs");
        assert_eq!(qn.symbol_path(), "MyStruct.method");
        assert_eq!(qn.as_str(), "src/file.rs::MyStruct.method");
    }

    #[test]
    fn qualified_name_rejects_empty() {
        assert!(QualifiedName::parse("").is_err());
    }

    #[test]
    fn qualified_name_rejects_missing_separator() {
        assert!(QualifiedName::parse("no_separator").is_err());
    }

    #[test]
    fn qualified_name_rejects_empty_file_path() {
        assert!(QualifiedName::parse("::symbol").is_err());
    }

    #[test]
    fn qualified_name_rejects_empty_symbol_path() {
        assert!(QualifiedName::parse("file::").is_err());
    }

    #[test]
    fn qualified_name_borrow_str_hashmap_lookup() {
        use std::collections::HashMap;
        let mut map: HashMap<QualifiedName, u32> = HashMap::new();
        let qn = QualifiedName::parse("src/lib.rs::foo").unwrap();
        map.insert(qn, 42);
        // Exercises the Borrow<str> impl: look up by &str, not QualifiedName
        assert_eq!(map.get("src/lib.rs::foo"), Some(&42));
    }

    #[test]
    fn qualified_name_serde_roundtrip() {
        let qn = QualifiedName::parse("src/lib.rs::Foo.bar").unwrap();
        let json = serde_json::to_string(&qn).unwrap();
        let qn2: QualifiedName = serde_json::from_str(&json).unwrap();
        assert_eq!(qn, qn2);
    }

    #[test]
    fn node_id_returns_correct_identifier() {
        let file = Node::File(FileNode {
            path: "src/main.rs".into(),
            language: Language::Rust,
            hash: "abc".into(),
        });
        assert_eq!(file.id(), "src/main.rs");

        let sym = Node::Symbol(SymbolNode {
            name: "foo".into(),
            qualified_name: "src/lib.rs::foo".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "src/lib.rs".into(),
                line_start: 1,
                line_end: 5,
                col_start: 0,
                col_end: 1,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        });
        assert_eq!(sym.id(), "src/lib.rs::foo");
    }

    #[test]
    fn serde_roundtrip_all_supporting_types() {
        macro_rules! assert_roundtrip {
            ($val:expr, $ty:ty) => {{
                let json = serde_json::to_string(&$val).unwrap();
                let _: $ty = serde_json::from_str(&json).unwrap();
            }};
        }

        // Enums
        assert_roundtrip!(Language::Rust, Language);
        assert_roundtrip!(NodeKind::Symbol, NodeKind);
        assert_roundtrip!(SymbolKind::Function, SymbolKind);
        assert_roundtrip!(NonParsedKind::Doc, NonParsedKind);
        assert_roundtrip!(Visibility::Public, Visibility);
        assert_roundtrip!(Confidence::High, Confidence);
        assert_roundtrip!(EdgeKind::Calls, EdgeKind);
        assert_roundtrip!(Direction::Forward, Direction);

        // Core types
        let loc = Location {
            file: "f".into(),
            line_start: 1,
            line_end: 2,
            col_start: 0,
            col_end: 10,
        };
        assert_roundtrip!(loc, Location);

        let file_node = FileNode {
            path: "f".into(),
            language: Language::Rust,
            hash: "h".into(),
        };
        assert_roundtrip!(file_node.clone(), FileNode);
        assert_roundtrip!(Node::File(file_node), Node);

        let sym = SymbolNode {
            name: "s".into(),
            qualified_name: "f::s".into(),
            kind: SymbolKind::Function,
            location: Location {
                file: "f".into(),
                line_start: 1,
                line_end: 2,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: true,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        };
        assert_roundtrip!(sym, SymbolNode);

        let np = NonParsedNode {
            path: "r.md".into(),
            file_kind: NonParsedKind::Doc,
            hash: "h".into(),
        };
        assert_roundtrip!(np, NonParsedNode);

        let edge = Edge {
            kind: EdgeKind::Calls,
            source: "a".into(),
            target: "b".into(),
            metadata: None,
        };
        assert_roundtrip!(edge, Edge);

        // Supporting types
        assert_roundtrip!(ImpactTarget::File("f".into()), ImpactTarget);
        assert_roundtrip!(ImpactTarget::Symbol("s".into()), ImpactTarget);
        assert_roundtrip!(
            TraversalResult {
                node: "n".into(),
                depth: 1,
                path: vec![],
                edge_kind: EdgeKind::Calls
            },
            TraversalResult
        );
        assert_roundtrip!(
            SearchResult {
                qualified_name: "f::s".into(),
                name: "s".into(),
                kind: SymbolKind::Function,
                file_path: "f".into(),
                score: 1.0
            },
            SearchResult
        );
        assert_roundtrip!(
            Reference {
                symbol: "s".into(),
                edge_kind: EdgeKind::Calls,
                location: None
            },
            Reference
        );
        assert_roundtrip!(
            IndexStats {
                files_indexed: 1,
                symbols_extracted: 2,
                edges_created: 3,
                duration: std::time::Duration::from_secs(1)
            },
            IndexStats
        );
        assert_roundtrip!(
            GraphStats {
                files: 1,
                symbols: 2,
                edges: 3,
                entry_point_count: None,
                avg_criticality: None,
                clone_clusters: None,
                duplication_pct: None,
                most_duplicated: None,
                avg_risk: None,
                p90_risk: None,
            },
            GraphStats
        );
        assert_roundtrip!(
            DiffHunk {
                file: "f".into(),
                old_start: 1,
                old_count: 2,
                new_start: 1,
                new_count: 3
            },
            DiffHunk
        );
        assert_roundtrip!(
            AffectedNode {
                qualified_name: "q".into(),
                depth: 1,
                confidence: Confidence::High,
                path: vec![]
            },
            AffectedNode
        );
        assert_roundtrip!(
            ImpactReport {
                targets: vec![],
                affected: vec![],
                depth: 3,
                min_confidence: Confidence::Structural
            },
            ImpactReport
        );
        assert_roundtrip!(
            DiffImpactReport {
                changed_symbols: vec![],
                impact: ImpactReport {
                    targets: vec![],
                    affected: vec![],
                    depth: 0,
                    min_confidence: Confidence::Structural
                }
            },
            DiffImpactReport
        );
    }

    #[test]
    fn flow_types_serde_roundtrip() {
        let entry = EntryPoint {
            qualified_name: "src/main.rs::main".into(),
            kind: EntryPointKind::Main,
            confidence: 1.0,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let _: EntryPoint = serde_json::from_str(&json).unwrap();

        let flow = ExecutionFlow {
            entry: "src/main.rs::main".into(),
            path: vec!["src/main.rs::main".into(), "src/db.rs::connect".into()],
            depth: 2,
            truncated: false,
        };
        let json = serde_json::to_string(&flow).unwrap();
        let _: ExecutionFlow = serde_json::from_str(&json).unwrap();

        let score = CriticalityScore {
            qualified_name: "src/db.rs::connect".into(),
            betweenness: 0.75,
            flow_count: 42,
            is_entry_point: false,
        };
        let json = serde_json::to_string(&score).unwrap();
        let _: CriticalityScore = serde_json::from_str(&json).unwrap();

        let config = FlowConfig::default();
        assert_eq!(config.max_depth, 20);
        assert_eq!(config.max_flows, 1000);
        assert_eq!(config.visit_budget, 100_000);
        assert_eq!(config.max_public_roots, 50);

        let analysis = FlowAnalysis {
            entry_points: vec![entry],
            flows: vec![flow],
            criticality: vec![score],
            stats: FlowStats {
                total_entry_points: 1,
                total_flows: 1,
                max_depth: 2,
                avg_depth: 2.0,
            },
        };
        let json = serde_json::to_string(&analysis).unwrap();
        let _: FlowAnalysis = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn graph_stats_optional_fields_default_none() {
        let stats = GraphStats {
            files: 10,
            symbols: 50,
            edges: 100,
            entry_point_count: None,
            avg_criticality: None,
            clone_clusters: None,
            duplication_pct: None,
            most_duplicated: None,
            avg_risk: None,
            p90_risk: None,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(!json.contains("entry_point_count"));
        assert!(!json.contains("avg_criticality"));
    }

    #[test]
    fn risk_weights_default_sum_to_one() {
        let w = RiskWeights::default();
        let sum = w.criticality + w.coupling + w.test_gap + w.sensitivity;
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "default weights must sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn risk_score_serde_roundtrip() {
        let score = RiskScore {
            qualified_name: "src/auth.rs::verify_token".into(),
            composite: 0.87,
            factors: RiskFactors {
                criticality: 0.90,
                coupling: 0.80,
                test_gap: 0.85,
                sensitivity: 0.95,
            },
        };
        let json = serde_json::to_string(&score).unwrap();
        let decoded: RiskScore = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.qualified_name, score.qualified_name);
        assert!((decoded.composite - score.composite).abs() < 1e-10);
        assert!((decoded.factors.criticality - score.factors.criticality).abs() < 1e-10);
        assert!((decoded.factors.coupling - score.factors.coupling).abs() < 1e-10);
        assert!((decoded.factors.test_gap - score.factors.test_gap).abs() < 1e-10);
        assert!((decoded.factors.sensitivity - score.factors.sensitivity).abs() < 1e-10);
    }

    #[test]
    fn risk_weights_normalized_preserves_proportions() {
        let w = RiskWeights {
            criticality: 3.0,
            coupling: 2.5,
            test_gap: 2.5,
            sensitivity: 2.0,
        };
        let n = w.normalized();
        let sum = n.criticality + n.coupling + n.test_gap + n.sensitivity;
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "normalized weights must sum to 1.0, got {sum}"
        );
        // Proportions should match: original sums to 10.0
        assert!((n.criticality - 0.30).abs() < 1e-10);
        assert!((n.coupling - 0.25).abs() < 1e-10);
        assert!((n.test_gap - 0.25).abs() < 1e-10);
        assert!((n.sensitivity - 0.20).abs() < 1e-10);
    }

    #[test]
    fn graph_stats_optional_fields_present() {
        let stats = GraphStats {
            files: 10,
            symbols: 50,
            edges: 100,
            entry_point_count: Some(5),
            avg_criticality: Some(0.034),
            clone_clusters: None,
            duplication_pct: None,
            most_duplicated: None,
            avg_risk: None,
            p90_risk: None,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["entry_point_count"], 5);
    }
}
