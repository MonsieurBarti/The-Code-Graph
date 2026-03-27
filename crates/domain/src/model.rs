use std::path::PathBuf;
use std::borrow::Borrow;
use std::fmt;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
            EdgeKind::DependsOn
            | EdgeKind::ConditionalImport
            | EdgeKind::SideEffectImport => Confidence::Low,
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
    pub source: String,
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
            assert_eq!(kind.confidence(), *expected, "wrong confidence for {kind:?}");
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
        // This requires Borrow<str> implementation on QualifiedName
        // so that we can look up by &str
        let qn_lookup = QualifiedName::parse("src/lib.rs::foo").unwrap();
        assert_eq!(map.get(&qn_lookup), Some(&42));
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
                line_start: 1, line_end: 5, col_start: 0, col_end: 1,
            },
            visibility: Visibility::Public,
            is_exported: true, is_async: false, is_test: false,
            decorators: vec![], signature: None,
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
        let loc = Location { file: "f".into(), line_start: 1, line_end: 2, col_start: 0, col_end: 10 };
        assert_roundtrip!(loc, Location);

        let file_node = FileNode { path: "f".into(), language: Language::Rust, hash: "h".into() };
        assert_roundtrip!(file_node.clone(), FileNode);
        assert_roundtrip!(Node::File(file_node), Node);

        let sym = SymbolNode {
            name: "s".into(), qualified_name: "f::s".into(), kind: SymbolKind::Function,
            location: Location { file: "f".into(), line_start: 1, line_end: 2, col_start: 0, col_end: 0 },
            visibility: Visibility::Public, is_exported: true, is_async: false, is_test: false,
            decorators: vec![], signature: None,
        };
        assert_roundtrip!(sym, SymbolNode);

        let np = NonParsedNode { path: "r.md".into(), file_kind: NonParsedKind::Doc, hash: "h".into() };
        assert_roundtrip!(np, NonParsedNode);

        let edge = Edge { kind: EdgeKind::Calls, source: "a".into(), target: "b".into(), metadata: None };
        assert_roundtrip!(edge, Edge);

        // Supporting types
        assert_roundtrip!(ImpactTarget::File("f".into()), ImpactTarget);
        assert_roundtrip!(ImpactTarget::Symbol("s".into()), ImpactTarget);
        assert_roundtrip!(TraversalResult { node: "n".into(), depth: 1, path: vec![], edge_kind: EdgeKind::Calls }, TraversalResult);
        assert_roundtrip!(SearchResult { qualified_name: "f::s".into(), name: "s".into(), kind: SymbolKind::Function, file_path: "f".into(), score: 1.0 }, SearchResult);
        assert_roundtrip!(Reference { source: "s".into(), edge_kind: EdgeKind::Calls, location: None }, Reference);
        assert_roundtrip!(IndexStats { files_indexed: 1, symbols_extracted: 2, edges_created: 3, duration: std::time::Duration::from_secs(1) }, IndexStats);
        assert_roundtrip!(GraphStats { files: 1, symbols: 2, edges: 3 }, GraphStats);
        assert_roundtrip!(DiffHunk { file: "f".into(), old_start: 1, old_count: 2, new_start: 1, new_count: 3 }, DiffHunk);
        assert_roundtrip!(AffectedNode { qualified_name: "q".into(), depth: 1, confidence: Confidence::High, path: vec![] }, AffectedNode);
        assert_roundtrip!(ImpactReport { targets: vec![], affected: vec![], depth: 3, min_confidence: Confidence::Structural }, ImpactReport);
        assert_roundtrip!(DiffImpactReport { changed_symbols: vec![], impact: ImpactReport { targets: vec![], affected: vec![], depth: 0, min_confidence: Confidence::Structural } }, DiffImpactReport);
    }
}
