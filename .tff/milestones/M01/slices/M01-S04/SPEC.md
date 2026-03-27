# M01-S04: Language Parsers & Import Resolution

## Problem

The parser crate has infrastructure (trait, registry, thread-local management) and a TypeScript/JavaScript parser, but only extracts unresolved `RawImport`/`Export` data. Without additional language parsers and an import resolution pipeline, the graph has no cross-file edges (`ImportsFrom`, `ReExport`, `Extends`, `Implements`, `Embeds`, etc.) and cannot index Rust, Python, or Go codebases.

## Approach

Add three language parsers (Rust, Python, Go) implementing the existing `LanguageParser` trait, plus an import resolution pipeline (`resolver/` module) that converts `RawImport`/`Export` into resolved edges. Each resolver is language-specific: `oxc_resolver` for TS/JS, custom module tree walker for Rust, filesystem prober for Python, go.mod-based mapper for Go.

## Scope

### In Scope
- Rust parser (`rust_lang.rs`): functions, structs, enums, traits, type aliases, consts, macros, `impl` blocks, `mod` declarations, `use` declarations
- Python parser (`python.rs`): functions, classes, methods, decorators, imports (`import` + `from...import`), `TYPE_CHECKING` detection
- Go parser (`go.rs`): functions, methods (with receiver), structs, interfaces, consts/vars, imports (normal, blank, dot), struct embedding
- Import resolver trait (`resolver/mod.rs`): `ImportResolver` trait + `ResolverRegistry`
- TS/JS resolver (`resolver/typescript.rs`): `oxc_resolver` file resolution + barrel chain traversal
- Rust resolver (`resolver/rust_lang.rs`): module tree construction from `mod` declarations + `use` path resolution
- Python resolver (`resolver/python.rs`): relative import resolution + stdlib detection + `__init__.py` probing
- Go resolver (`resolver/go.rs`): `go.mod` parsing + module path prefix stripping
- Update `ParserRegistry` to register all new parsers
- Unit tests for each parser against real code snippets
- Integration tests for each resolver

### Not In Scope
- Cross-file call resolution (Section 3.9) — deferred to S05
- Parallel file indexing orchestration / `rayon` — deferred to S05 (CLI)
- Advanced search quality (custom BM25 weights, trigram) — later slice
- `#[path = "..."]` attribute in Rust (rare, log warning)
- Procedural macro resolution in Rust
- `#[cfg(...)]` conditional compilation in Rust
- Go implicit interface satisfaction detection
- Resolution of crates.io / PyPI / external Go dependencies (only local/workspace)
- `extern crate` declarations (Rust 2015 artifact)

## Design

### Crate Structure (additions to parser)

```
crates/parser/src/
  lib.rs              # (existing) add re-exports for new parsers
  registry.rs         # (modify) register RustParser, PythonParser, GoParser
  typescript.rs       # (existing, unchanged)
  rust_lang.rs        # NEW: Rust parser
  python.rs           # NEW: Python parser
  go.rs               # NEW: Go parser
  resolver/
    mod.rs            # NEW: ImportResolver trait, ResolverRegistry, ResolveContext
    typescript.rs     # NEW: oxc_resolver + barrel chain traversal
    rust_lang.rs      # NEW: module tree walker + use path resolution
    python.rs         # NEW: filesystem prober + stdlib set
    go.rs             # NEW: go.mod parser + module path resolution
  test_utils.rs       # (existing) add helpers for new languages
```

### New Dependencies

```toml
# Parser crate additions
tree-sitter-rust = "0.24"
tree-sitter-python = "0.23"
tree-sitter-go = "0.23"
oxc_resolver = "11"
toml = "0.8"          # Minimal Cargo.toml parsing for Rust resolver
```

### ImportResolver Trait (resolver/mod.rs)

```rust
/// Trait for language-specific import resolvers.
/// Called after all files are parsed (Phase 2 of parse-then-resolve).
pub trait ImportResolver: Send + Sync {
    /// Which languages this resolver handles.
    fn languages(&self) -> &[Language];

    /// Resolve raw imports from a single file into graph edges.
    fn resolve(
        &self,
        file_path: &Path,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>>;
}

/// Shared context for resolution — all parsed files and their results.
pub struct ResolveContext {
    pub project_root: PathBuf,
    pub parsed_files: HashMap<PathBuf, ParseResult>,
    pub file_tree: Vec<PathBuf>,
}

pub struct ResolverRegistry {
    resolvers: Vec<Box<dyn ImportResolver>>,
}

impl ResolverRegistry {
    pub fn new(project_root: &Path) -> Self;
    pub fn resolver_for_language(&self, lang: Language) -> Option<&dyn ImportResolver>;
    pub fn resolve_file(
        &self,
        file_path: &Path,
        lang: Language,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>>;
}
```

### Rust Parser (rust_lang.rs)

**Symbol extraction:**

| Construct | SymbolKind | Qualified Name |
|---|---|---|
| `fn foo()` (top-level) | Function | `file::foo` |
| `fn bar()` (in `impl Foo`) | Method | `file::Foo.bar` |
| `struct Foo` | Struct | `file::Foo` |
| `enum Bar` | Enum | `file::Bar` |
| `trait Baz` | Trait | `file::Baz` |
| `type Alias = ...` | TypeAlias | `file::Alias` |
| `const X` | Const | `file::X` |
| `static Y` | Variable | `file::Y` |
| `macro_rules! m` | Macro | `file::m` |

**Edges:**
- `Contains`: file → each top-level symbol
- `ChildOf`: impl method → parent type (same file)
- `Implements`: type → trait (from `impl Trait for Type`, same file only in v0.1)

**Imports (RawImport):**
- `use crate::auth::validate` → specifier: `"crate::auth::validate"`, names: `[{name: "validate"}]`
- `use foo::*` → is_namespace: true
- `use foo::{A, B}` → names: `[{name: "A"}, {name: "B"}]`
- `pub use` → metadata: `"reexport"` (resolver creates `ReExport` edge)

**Visibility:**
- `pub` → Public, `pub(crate)` → Crate, no modifier → Private

### Python Parser (python.rs)

**Symbol extraction:**

| Construct | SymbolKind | Qualified Name |
|---|---|---|
| `def foo():` | Function | `file::foo` |
| `async def foo():` | Function (is_async=true) | `file::foo` |
| `class Bar:` | Class | `file::Bar` |
| `def method(self):` (in class) | Method | `file::Bar.method` |
| `@property` method | Property | `file::Bar.prop` |
| Top-level `X = ...` | Variable | `file::X` |

**Edges:**
- `Contains`: file → each top-level symbol
- `ChildOf`: method → class
- `Extends`: class → base class (from `class Foo(Bar):`, same file)

**Imports:**
- `from .models import User` → specifier: `".models"`, names: `[{name: "User"}]`
- `import os.path` → specifier: `"os.path"`
- `from typing import TYPE_CHECKING` inside `if TYPE_CHECKING:` → is_type_only: true

**Visibility:**
- `_prefix` / `__prefix` → Private, else → Public

### Go Parser (go.rs)

**Symbol extraction:**

| Construct | SymbolKind | Qualified Name |
|---|---|---|
| `func Foo()` | Function | `file::Foo` |
| `func (r *Bar) Method()` | Method | `file::Bar.Method` |
| `type Foo struct {}` | Struct | `file::Foo` |
| `type Bar interface {}` | Interface | `file::Bar` |
| `const X = 1` | Const | `file::X` |
| `var Y string` | Variable | `file::Y` |
| `type Alias = Other` | TypeAlias | `file::Alias` |

**Edges:**
- `Contains`: file → each top-level symbol
- `ChildOf`: receiver method → struct
- `Embeds`: struct → embedded type (from `type Foo struct { Bar }`)

**Imports:**
- `import "fmt"` → specifier: `"fmt"` (stdlib, resolver skips)
- `import _ "lib/pq"` → is_side_effect: true
- `import . "fmt"` → is_namespace: true (dot import)

**Visibility:**
- First char uppercase → Public, lowercase → Private

### TS/JS Resolver (resolver/typescript.rs)

Uses `oxc_resolver` for file-level resolution:
```rust
let resolver = Resolver::new(ResolveOptions {
    extensions: vec![".ts", ".tsx", ".js", ".jsx", ".json", ".mjs", ".mts"],
    condition_names: vec!["import", "require", "node", "default"],
    main_fields: vec!["module", "main"],
    tsconfig: Some(TsconfigDiscovery::Auto),
    ..ResolveOptions::default()
});
```

For each `RawImport`:
1. Resolve specifier to file path via `oxc_resolver`
2. For each imported name, trace through barrel chain if target is a barrel file
3. Create `ImportsFrom` edge (file → file)
4. Create `ReExport` edges for `export { X } from "..."` and `export * from "..."`
5. Create `BarrelReExportAll` edge for star re-exports

**Barrel chain traversal:**
- Max depth: 10 hops
- Cycle detection: `HashSet<PathBuf>` visited set
- Named re-exports checked before star re-exports

### Rust Resolver (resolver/rust_lang.rs)

Two-phase:
1. **Build module tree**: Walk `mod` declarations from crate root (`src/lib.rs` or `src/main.rs`), mapping `module_path → file_path`
2. **Resolve `use` paths**: For each `use` statement, walk the module tree to find the target file

Cargo.toml parsing (minimal): extract `[package].name`, `[workspace].members`, `[dependencies].*.path` for workspace awareness.

### Python Resolver (resolver/python.rs)

- Relative imports: count dots, walk up directories, probe `{module}.py` or `{module}/__init__.py`
- Absolute imports: check against hardcoded stdlib set (~150 entries), then probe project tree
- `TYPE_CHECKING` imports → `ConditionalImport` edge

### Go Resolver (resolver/go.rs)

- Parse `go.mod` for module path
- Local imports: strip module prefix, map to directory
- Stdlib: first path element has no dots → skip
- External: different module prefix → skip

## Acceptance Criteria

### Infrastructure
- AC1: `cargo build --workspace` succeeds with new dependencies
- AC2: `ParserRegistry::new()` returns registry with all 5 language parsers (TS, JS, Rust, Python, Go)
- AC3: `parser_for_file("foo.rs")` returns `Some` with `language() == Rust`
- AC4: `parser_for_file("foo.py")` returns `Some` with `language() == Python`
- AC5: `parser_for_file("foo.go")` returns `Some` with `language() == Go`

### Rust Parser
- AC6: Parses `fn foo() {}` → Function symbol, name="foo"
- AC7: Parses `struct Bar {}` → Struct symbol
- AC8: Parses `impl Foo { fn bar(&self) {} }` → Method symbol with ChildOf edge to Foo
- AC9: Parses `trait Baz {}` → Trait symbol
- AC10: Parses `enum Color { Red, Green }` → Enum symbol
- AC11: Parses `use crate::auth::validate;` → RawImport with specifier "crate::auth::validate"
- AC12: Parses `pub use self::greetings::hello;` → RawImport marked as reexport
- AC13: Parses `mod submodule;` → mod declaration captured for resolution
- AC14: `pub fn` → Visibility::Public, `pub(crate) fn` → Visibility::Crate, `fn` → Visibility::Private

### Python Parser
- AC15: Parses `def foo():` → Function symbol
- AC16: Parses `class Bar:` with methods → Class + Method symbols with ChildOf edges
- AC17: Parses `from .models import User` → RawImport with specifier=".models"
- AC18: Parses `import os.path` → RawImport with specifier="os.path"
- AC19: Parses `async def foo():` → Function with is_async=true
- AC20: Parses decorated functions/classes → decorators field populated
- AC21: Detects `if TYPE_CHECKING:` imports as conditional (is_type_only=true)

### Go Parser
- AC22: Parses `func Foo() {}` → Function symbol
- AC23: Parses `func (r *Bar) Method()` → Method symbol with ChildOf edge to Bar
- AC24: Parses `type Foo struct { Bar }` → Struct + Embeds edge for embedded Bar
- AC25: Parses `type Baz interface { Method() }` → Interface symbol
- AC26: Parses `import _ "lib/pq"` → RawImport with is_side_effect=true
- AC27: Parses `import . "fmt"` → RawImport with is_namespace=true
- AC28: Capitalized names → Visibility::Public, lowercase → Visibility::Private

### Import Resolution — TS/JS
- AC29: Resolves `import { foo } from "./utils"` to target file path via oxc_resolver
- AC30: Barrel chain: `import { X } from "./services"` traces through `services/index.ts` re-exports
- AC31: Circular barrel chain terminates gracefully (no infinite loop, no panic)
- AC32: Creates `ImportsFrom` edge from importing file to resolved target file
- AC33: Creates `BarrelReExportAll` edge for `export * from "..."`
- AC34: Creates `ReExport` edge for `export { X } from "..."`

### Import Resolution — Rust
- AC35: Builds module tree from `mod` declarations starting at crate root
- AC36: Resolves `use crate::auth::validate` to file containing the `auth` module
- AC37: Resolves `use self::foo` relative to current module
- AC38: Creates `ReExport` edge for `pub use` statements
- AC39: Handles both `foo.rs` and `foo/mod.rs` module naming conventions

### Import Resolution — Python
- AC40: Resolves `from .models import User` to sibling `models.py` file
- AC41: Resolves `from ..utils import helper` by walking up directories
- AC42: Skips stdlib imports (e.g., `import os`) — no edge created
- AC43: Creates `ConditionalImport` edge for `TYPE_CHECKING` imports

### Import Resolution — Go
- AC44: Parses `go.mod` to extract module path
- AC45: Resolves local import by stripping module prefix and mapping to directory
- AC46: Skips stdlib imports (no dots in first path element)
- AC47: Creates `SideEffectImport` edge for blank imports (`import _`)
- AC48: Creates `DotImport` edge for dot imports (`import .`)

### Error Handling
- AC49: Invalid/empty source for each language returns `CodeGraphError::Parse`, no panic
- AC50: Source with syntax errors → partial ParseResult extraction

### Thread Safety
- AC51: Each new parser's thread-local management works from multiple threads

### Quality
- AC52: `cargo test -p parser` passes with all tests green
- AC53: `cargo clippy -p parser -- -Dwarnings` passes

## Design Notes

- **Cross-file call resolution (Section 3.9) is NOT in scope.** S04 resolves imports (file → file edges). Call resolution (symbol → symbol) requires the full index pipeline and is deferred to S05.
- **`rayon` is NOT a parser dependency.** Parallelism is the caller's concern (CLI/IndexUseCase in S05).
- **Go implicit interface satisfaction is skipped.** Detecting `Implements` edges in Go requires full type analysis — deferred to v0.2.
- **Rust cross-file `impl` blocks:** Same-file `impl` creates `ChildOf` edges directly. Cross-file `impl` deferred to post-processing in S05.
- **oxc_resolver is pinned to v11.** The API is stable in its core shape. Pin to avoid surprise breakage from rapid OXC development.
- **Cargo.toml parsing is minimal.** Only `[package].name`, `[workspace].members`, and `[dependencies].*.path`. Uses `toml` crate for correctness.
- **Python stdlib set is hardcoded.** ~150 top-level module names. Updated manually when Python versions add new stdlib modules.

## Non-Goals

- Cross-file call resolution (S05)
- Parallel indexing orchestration / rayon (S05)
- CLI commands, output formatting (S05+)
- Advanced search quality (later slice)
- Performance optimization or benchmarking
- Resolution of external dependencies (crates.io, PyPI, Go modules)
