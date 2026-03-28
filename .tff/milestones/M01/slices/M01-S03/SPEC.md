# M01-S03: Tree-Sitter Parser Infrastructure

## Problem

The domain crate defines types (`SymbolNode`, `Edge`, `FileNode`) and port traits, and the storage crate persists them — but nothing can produce them from source code. Without a parser, there is no data to store or query. The parser crate is the second adapter crate and the core of the indexing pipeline.

## Approach

Create a `crates/parser` crate with the tree-sitter-based parsing infrastructure: core types (`ParseResult`, `RawImport`, `Export`, `ImportName`), the `LanguageParser` trait, a `ParserRegistry` with extension-based dispatch, and thread-local parser management for safe concurrent use. Include a **TypeScript/JavaScript parser** as proof-of-concept to validate the infrastructure end-to-end against real code.

TypeScript is chosen as the proof-of-concept because it's the most complex language target (TS + TSX + JS + JSX via two grammar entry points from one crate) — if the infrastructure supports it, simpler languages will work too.

## Scope

### In Scope
- Create `crates/parser` with Cargo.toml and add to workspace
- Core types: `ParseResult`, `RawImport`, `ImportName`, `Export`
- `LanguageParser` trait (`Send + Sync`, returns `ParseResult`)
- `ParserRegistry` with dynamic dispatch (`Box<dyn LanguageParser>`)
- `register_all()` to populate registry, `parser_for_file()` for extension-based lookup
- Thread-local `tree-sitter::Parser` management via `thread_local!` + `RefCell<Parser>`
- TypeScript/JavaScript parser implementing `LanguageParser`:
  - Symbol extraction: functions, classes, interfaces, type aliases, enums, consts/variables, methods, properties, React components
  - Structural edges: `Contains` (File → Symbol), `ChildOf` (Class → Method/Property)
  - `RawImport` extraction from `import` statements (unresolved — specifier + names only)
  - `Export` extraction from `export` statements
  - Qualified name construction per spec Section 3.5 (`file_path::symbol_path`)
  - Visibility: `export` keyword → Public, else Private
  - `is_async`, `is_test`, `is_exported`, `decorators`, `signature` extraction
- Graceful parse failure: return `CodeGraphError::Parse` on tree-sitter failure, don't panic
- Unit tests for all infrastructure types
- Integration tests for TypeScript parser against real code snippets
- `test_utils.rs` with helpers for constructing test fixtures

### Not In Scope
- Rust, Python, Go language parsers (S04)
- Import resolution / `resolver/` directory (S04)
- Cross-file call resolution (S04)
- `oxc_resolver` integration (S04)
- Barrel chain traversal (S04)
- Parallel file indexing orchestration (S05 — CLI/Index)
- `rayon` dependency — the parser provides `Send + Sync` traits; the caller parallelizes

## Design

### Crate Structure

```
crates/parser/
  Cargo.toml
  src/
    lib.rs              # re-exports, ParseResult, RawImport, Export, ImportName
    registry.rs         # ParserRegistry, register_all(), parser_for_file()
    typescript.rs       # TypeScript + TSX + JavaScript + JSX parser
    test_utils.rs       # test helpers
```

S04 will add: `rust_lang.rs`, `python.rs`, `go.rs`, `resolver/`.

### Dependencies

```toml
[dependencies]
domain = { path = "../domain" }
tree-sitter = "0.24"
tree-sitter-typescript = "0.23"
tree-sitter-javascript = "0.23"

[dev-dependencies]
serde_json = "1"   # for test assertions
```

**Note:** Exact versions TBD during research — the version matrix for tree-sitter core + grammar crates must be verified. The constraint is that all grammar crates must target the same `tree-sitter` core ABI.

### Core Types (lib.rs)

```rust
/// Output of parsing a single source file.
/// Phase 1 of two-phase "parse then resolve" — imports are unresolved.
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub symbols: Vec<SymbolNode>,
    pub edges: Vec<Edge>,
    pub imports: Vec<RawImport>,
    pub exports: Vec<Export>,
}

/// An unresolved import statement extracted from source code.
/// Converted to resolved edges (ImportsFrom, Calls, etc.) during the
/// resolution phase in S04.
#[derive(Debug, Clone)]
pub struct RawImport {
    pub specifier: String,           // e.g., "./utils", "@scope/pkg", "fs"
    pub names: Vec<ImportName>,      // named imports
    pub is_type_only: bool,          // `import type { ... }`
    pub is_side_effect: bool,        // `import "./polyfill"`
    pub is_namespace: bool,          // `import * as ns from "..."`
    pub line: usize,                 // source line for diagnostics
}

/// A single named import within an import statement.
#[derive(Debug, Clone)]
pub struct ImportName {
    pub name: String,                // original exported name
    pub alias: Option<String>,       // local alias (`as` rename)
    pub is_type: bool,               // individual `type` modifier
}

/// An export declaration extracted from source code.
/// Used during resolution (S04) to build ImportsFrom and ReExport edges.
#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,                // exported name ("default" for default export)
    pub local_name: Option<String>,  // local binding if different (`export { foo as bar }`)
    pub is_default: bool,
    pub is_type_only: bool,          // `export type { ... }`
    pub is_reexport: bool,           // `export { ... } from "..."`
    pub source_specifier: Option<String>,  // specifier if re-export
}
```

These types live in the parser crate (not domain) — they are parser-specific intermediates. Domain types (`SymbolNode`, `Edge`) are the stable output; `RawImport`/`Export` are consumed and discarded during resolution.

### LanguageParser Trait

```rust
/// Trait for language-specific tree-sitter parsers.
/// Implementations must be Send + Sync so the registry can be shared across threads.
/// The actual tree-sitter Parser instance is managed via thread_local (not stored in the impl).
pub trait LanguageParser: Send + Sync {
    /// Which domain Language this parser handles.
    fn language(&self) -> Language;

    /// File extensions this parser handles (without leading dot).
    fn file_extensions(&self) -> &[&str];

    /// Parse source code and extract symbols, edges, imports, exports.
    /// `path` is the project-relative file path (used for qualified names).
    fn parse(&self, source: &[u8], path: &Path) -> domain::error::Result<ParseResult>;
}
```

### Thread-Local Parser Management

Each `LanguageParser` implementation stores a `tree_sitter::Language` (which IS Send + Sync — it's a pointer to compiled C grammar) and uses a thread-local `tree_sitter::Parser` for the actual parsing:

```rust
pub struct TypeScriptParser {
    ts_language: tree_sitter::Language,   // TypeScript grammar
    tsx_language: tree_sitter::Language,   // TSX grammar
    js_language: tree_sitter::Language,    // JavaScript grammar
}

// tree_sitter::Language is Send + Sync (opaque pointer to C static data)
// tree_sitter::Parser is NOT Send — must be thread-local

thread_local! {
    static TS_PARSER: RefCell<tree_sitter::Parser> = RefCell::new(tree_sitter::Parser::new());
}

impl LanguageParser for TypeScriptParser {
    fn parse(&self, source: &[u8], path: &Path) -> Result<ParseResult> {
        let lang = self.language_for_extension(path);
        TS_PARSER.with(|parser| {
            let mut parser = parser.borrow_mut();
            parser.set_language(&lang)?;
            let tree = parser.parse(source, None)
                .ok_or_else(|| CodeGraphError::Parse {
                    file: path.to_path_buf(),
                    message: "tree-sitter parse returned None".into(),
                })?;
            self.extract(source, path, &tree)
        })
    }
}
```

**Why thread-local, not fresh parser per call:** `Parser::new()` allocates internal buffers. Thread-local reuses these across files parsed on the same thread, avoiding repeated allocation during parallel indexing. The cost is a `RefCell` borrow, which is negligible.

### ParserRegistry (registry.rs)

```rust
pub struct ParserRegistry {
    parsers: Vec<Box<dyn LanguageParser>>,
    extension_map: HashMap<String, usize>,  // extension -> index into parsers vec
}

impl ParserRegistry {
    /// Create registry with all supported language parsers.
    pub fn new() -> Self {
        let mut registry = Self {
            parsers: Vec::new(),
            extension_map: HashMap::new(),
        };
        registry.register(Box::new(TypeScriptParser::new()));
        // S04 adds: RustParser, PythonParser, GoParser
        registry
    }

    fn register(&mut self, parser: Box<dyn LanguageParser>) {
        let idx = self.parsers.len();
        for ext in parser.file_extensions() {
            self.extension_map.insert(ext.to_string(), idx);
        }
        self.parsers.push(parser);
    }

    /// Get the parser for a file based on its extension.
    /// Returns None for unsupported file types.
    pub fn parser_for_file(&self, path: &Path) -> Option<&dyn LanguageParser> {
        let ext = path.extension()?.to_str()?;
        let idx = self.extension_map.get(ext)?;
        Some(self.parsers[*idx].as_ref())
    }

    /// Get the parser for a specific Language enum value.
    pub fn parser_for_language(&self, lang: Language) -> Option<&dyn LanguageParser> {
        self.parsers.iter().find(|p| p.language() == lang).map(|p| p.as_ref())
    }

    /// List all supported file extensions.
    pub fn supported_extensions(&self) -> Vec<&str> {
        self.extension_map.keys().map(|s| s.as_str()).collect()
    }
}

// ParserRegistry is Send + Sync because:
// - Vec<Box<dyn LanguageParser>> is Send + Sync (trait bound on LanguageParser)
// - HashMap<String, usize> is Send + Sync
```

### TypeScript Parser (typescript.rs)

Handles `.ts`, `.tsx`, `.js`, `.jsx` extensions. Reports `Language::TypeScript` for `.ts`/`.tsx` and `Language::JavaScript` for `.js`/`.jsx`.

**Grammar selection:**
- `.ts` → `tree_sitter_typescript::LANGUAGE_TYPESCRIPT`
- `.tsx`, `.jsx` → `tree_sitter_typescript::LANGUAGE_TSX`
- `.js` → `tree_sitter_javascript::LANGUAGE_JAVASCRIPT`

TSX grammar is a superset of TypeScript that also handles JSX syntax. JSX files use the TSX grammar (which supports JSX in JavaScript context). Plain `.js` files use the JavaScript grammar for accuracy.

**Symbol extraction via tree-sitter queries or cursor walking:**

The parser walks the tree-sitter CST and extracts:

| Node Type | SymbolKind | Notes |
|-----------|------------|-------|
| `function_declaration` | Function | Top-level or nested |
| `class_declaration` | Class | |
| `interface_declaration` | Interface | TS only |
| `type_alias_declaration` | TypeAlias | TS only |
| `enum_declaration` | Enum | TS only |
| `lexical_declaration` (const/let) | Variable or Const | Const if `const`, Variable if `let` |
| `method_definition` | Method | Inside class body |
| `public_field_definition` | Property | Inside class body |
| `arrow_function` (exported const) | Function | `export const foo = () => {}` |
| `function` (React component pattern) | Component | PascalCase + returns JSX |

**Qualified name construction:**
- Top-level: `{file_path}::{name}`
- Nested (class method): `{file_path}::{ClassName}.{methodName}`
- Default export: `{file_path}::default`
- Duplicate names: `{file_path}::{name}.1`, `{file_path}::{name}.2`

**Edge extraction:**
- `Contains`: File path (as source) → each top-level symbol (qualified_name as target)
- `ChildOf`: Nested symbol → parent symbol (e.g., method → class)

**Import extraction (RawImport):**
- `import { a, b } from "./mod"` → `RawImport { specifier: "./mod", names: [a, b], ... }`
- `import type { T } from "./types"` → `is_type_only: true`
- `import * as ns from "./ns"` → `is_namespace: true`
- `import "./polyfill"` → `is_side_effect: true`
- `import def from "./mod"` → `names: [ImportName { name: "default", alias: Some("def") }]`

**Export extraction:**
- `export function foo()` → `Export { name: "foo", is_default: false }`
- `export default class Bar` → `Export { name: "default", local_name: Some("Bar"), is_default: true }`
- `export { foo, bar as baz }` → two `Export` entries
- `export { foo } from "./mod"` → `Export { name: "foo", is_reexport: true, source_specifier: Some("./mod") }`
- `export * from "./mod"` → handled at file level (for BarrelReExportAll edge in S04)

**Decorator extraction:**
- `@decorator` above function/class → added to `decorators: Vec<String>`

**Signature extraction:**
- Function/method: parameter list + return type annotation if present
- e.g., `(x: number, y: string) -> boolean`

### Error Handling

- `tree-sitter::Parser::parse()` returns `None` on catastrophic failure → `CodeGraphError::Parse`
- Tree with `tree.root_node().has_error()` → parse succeeds but logs warning, extracts what it can from non-error nodes
- Individual node extraction failures → skip that symbol, continue with rest of file
- Invalid UTF-8 in source → `CodeGraphError::Parse`

### Extension → Language Mapping

The parser crate owns this mapping. The domain `Language` enum exists but doesn't know about file extensions — that's a parser concern:

| Extension | Language | Grammar |
|-----------|----------|---------|
| `.ts` | TypeScript | `tree_sitter_typescript::LANGUAGE_TYPESCRIPT` |
| `.tsx` | TypeScript | `tree_sitter_typescript::LANGUAGE_TSX` |
| `.js` | JavaScript | `tree_sitter_javascript::LANGUAGE_JAVASCRIPT` |
| `.jsx` | JavaScript | `tree_sitter_typescript::LANGUAGE_TSX` |
| `.rs` | Rust | (S04) |
| `.py` | Python | (S04) |
| `.go` | Go | (S04) |

## Acceptance Criteria

### Infrastructure
- AC1: `cargo build --workspace` succeeds with parser crate in workspace members
- AC2: `ParserRegistry::new()` returns a registry with TypeScript/JavaScript parser registered
- AC3: `parser_for_file("foo.ts")` returns `Some` with `language() == TypeScript`
- AC4: `parser_for_file("foo.tsx")` returns `Some` with `language() == TypeScript`
- AC5: `parser_for_file("foo.js")` returns `Some` with `language() == JavaScript`
- AC6: `parser_for_file("foo.jsx")` returns `Some` with `language() == JavaScript`
- AC7: `parser_for_file("foo.rs")` returns `None` (not yet registered)
- AC8: `parser_for_file("foo.txt")` returns `None` (unsupported)
- AC9: `ParserRegistry` satisfies `Send + Sync` (compile-time assertion)

### TypeScript Parser — Symbol Extraction
- AC10: Parses `function foo() {}` → SymbolNode with kind=Function, name="foo"
- AC11: Parses `class Bar { baz() {} }` → Class symbol + Method symbol with ChildOf edge
- AC12: Parses `interface IFoo { prop: string }` → Interface symbol + Property symbol
- AC13: Parses `type Alias = string` → TypeAlias symbol
- AC14: Parses `enum Color { Red, Green }` → Enum symbol
- AC15: Parses `export const handler = async () => {}` → Function symbol, is_async=true, is_exported=true, visibility=Public
- AC16: Parses `export default function main() {}` → Function symbol, is_exported=true
- AC17: Non-exported symbols have visibility=Private, is_exported=false

### TypeScript Parser — Structural Edges
- AC18: Each top-level symbol has a `Contains` edge from file path to symbol qualified_name
- AC19: Class methods have `ChildOf` edge from method qualified_name to class qualified_name
- AC20: Qualified names follow spec format: `file_path::SymbolName`, `file_path::ClassName.methodName`

### TypeScript Parser — Import Extraction
- AC21: `import { a, b } from "./mod"` → RawImport with specifier="./mod", 2 ImportNames
- AC22: `import type { T } from "./types"` → RawImport with is_type_only=true
- AC23: `import * as ns from "./ns"` → RawImport with is_namespace=true
- AC24: `import "./polyfill"` → RawImport with is_side_effect=true
- AC25: `import def from "./mod"` → RawImport with name="default", alias=Some("def")

### TypeScript Parser — Export Extraction
- AC26: `export function foo()` → Export with name="foo", is_default=false
- AC27: `export default class Bar` → Export with name="default", local_name=Some("Bar"), is_default=true
- AC28: `export { foo } from "./mod"` → Export with is_reexport=true, source_specifier=Some("./mod")

### Error Handling
- AC29: Parsing invalid/empty source returns `CodeGraphError::Parse`, does not panic
- AC30: Parsing source with syntax errors extracts what it can, returns partial ParseResult (not an error)

### Thread Safety
- AC31: Parsing from two threads concurrently does not panic or deadlock (thread-local isolation)

### Quality
- AC32: `cargo test -p parser` passes with all tests green
- AC33: `cargo clippy -p parser -- -Dwarnings` passes
- AC34: Parser crate depends only on: domain, tree-sitter, tree-sitter-typescript, tree-sitter-javascript

## Design Notes

- **`rayon` is NOT a parser dependency.** The parser provides `LanguageParser: Send + Sync` and `ParserRegistry: Send + Sync`. Parallel file iteration is an orchestration concern for the IndexUseCase / CLI (S05). Each rayon worker thread gets its own thread-local `tree-sitter::Parser` automatically.
- **Tree-sitter version matrix is a research item.** The exact versions of `tree-sitter`, `tree-sitter-typescript`, and `tree-sitter-javascript` must be verified for ABI compatibility during the research phase. The spec lists `tree-sitter = "0.24"` as a starting point.
- **TSX grammar handles JSX.** The `tree-sitter-typescript` crate provides both `LANGUAGE_TYPESCRIPT` and `LANGUAGE_TSX`. TSX is a superset. `.jsx` files use the TSX grammar because `tree-sitter-javascript` does not understand JSX syntax.
- **ParseResult types are parser-internal.** They do NOT go in the domain crate. They reference domain types (SymbolNode, Edge) but are intermediate structures consumed during the resolution phase (S04).
- **S04 will add remaining parsers and all resolution.** Rust, Python, Go parsers + the entire `resolver/` directory with import resolution for all languages.
- **Decorator and signature extraction are best-effort.** If a decorator or signature can't be cleanly extracted, the field is left empty/None rather than failing the parse.
- **Port traits are unchanged.** No domain modifications needed for S03.

## Non-Goals

- Implementing Rust, Python, or Go parsers (S04)
- Import resolution of any kind (S04)
- Cross-file call resolution (S04)
- Parallel file indexing (S05)
- Performance optimization or benchmarking
- Tree-sitter query DSL (cursor walking is sufficient for v0.1; queries can be adopted if perf warrants)
- JSDoc / TSDoc extraction
