# Research — M01-S04: Language Parsers & Import Resolution

## R1: Dependency Version Matrix

### Tree-Sitter Grammar Crates

All three grammar crates depend on `tree-sitter-language ^0.1` as their sole runtime dependency. This ABI bridge makes them compatible with our existing `tree-sitter 0.24.7`. No version conflicts will occur.

| Crate | Latest | Published | Runtime Dep | Export Constant | ABI Compatible |
|---|---|---|---|---|---|
| `tree-sitter-rust` | 0.24.1 | 2026-03-19 | `tree-sitter-language ^0.1` | `LANGUAGE: LanguageFn` | Yes |
| `tree-sitter-python` | 0.25.0 | 2025-09-11 | `tree-sitter-language ^0.1` | `LANGUAGE: LanguageFn` | Yes |
| `tree-sitter-go` | 0.25.0 | 2025-08-29 | `tree-sitter-language ^0.1` | `LANGUAGE: LanguageFn` | Yes |

**Current project state** (from Cargo.lock):
- `tree-sitter`: 0.24.7
- `tree-sitter-language`: 0.1.7
- `tree-sitter-javascript`: 0.23.1
- `tree-sitter-typescript`: 0.23.2

**Uniform API**: All three export `pub const LANGUAGE: LanguageFn`, identical to the pattern in `tree-sitter-javascript` and `tree-sitter-typescript`. Usage: `parser.set_language(&LANGUAGE.into())?;`

### Decision: Pin Grammar Versions

```toml
tree-sitter-rust = "0.24"
tree-sitter-python = "0.23"
tree-sitter-go = "0.23"
```

Use `"0.23"` for Python and Go (conservative, matching our JS/TS pattern) unless specific features in 0.25.0 are needed. Cargo will pick the latest compatible patch. Use `"0.24"` for Rust since that's the latest minor.

**Update:** After version verification, the actual minimum available versions that use `tree-sitter-language ^0.1` bridge need to be confirmed during implementation. If `0.23.x` releases don't exist for these crates, use the latest available.

### oxc_resolver

| Field | Value |
|---|---|
| Version | 11.19.1 |
| License | MIT |
| Total downloads | 2,367,284 |
| Recent downloads | 542,858 |
| Last updated | 2026-02-28 |

19 normal dependencies. Key unique additions beyond our existing dep tree: `simd-json`, `papaya`, `fast-glob`, `rustix`, `compact_str`. Most deps (`serde`, `serde_json`, `tracing`, `thiserror`) are already standard.

---

## R2: oxc_resolver vs Alternatives

### Option A: oxc_resolver (recommended)

**Core API:**
```rust
use oxc_resolver::{Resolver, ResolveOptions};

let resolver = Resolver::new(ResolveOptions {
    extensions: vec![".ts".into(), ".tsx".into(), ".js".into(), ".jsx".into(), ".json".into()],
    condition_names: vec!["import".into(), "require".into(), "node".into(), "default".into()],
    main_fields: vec!["module".into(), "main".into()],
    tsconfig: Some(TsconfigDiscovery::Auto),
    ..ResolveOptions::default()
});

let result = resolver.resolve("/path/to/containing/dir", "./utils/helper");
// result.path() -> resolved file path
// result.package_json() -> associated package.json if any
```

**Key configuration fields:**
- `extensions` — file extensions to probe
- `condition_names` — package.json `exports` conditions
- `main_fields` — package entry points (`module`, `main`)
- `tsconfig` — `TsconfigDiscovery::Auto` walks up to find tsconfig.json, reads `compilerOptions.paths`
- `extension_alias` — map `.js` → `.ts` for ESM imports
- `symlinks` — follow symlinks (default true)

| Pro | Con |
|---|---|
| Battle-tested (2.3M downloads) | 19 dependencies |
| Full Node.js resolution algorithm | Rapidly evolving API (v11.x) |
| tsconfig paths built-in | More than we need initially |
| package.json exports/imports support | |
| Concurrent cache built-in | |
| Actively maintained (OXC team) | |

### Option B: Custom resolver (relative + extension probing)

```
resolve(from_dir, specifier):
  if specifier starts with '.':
    base = from_dir / specifier
    for ext in [".ts", ".tsx", ".js", ".jsx"]:
      if base.with_extension(ext) exists: return it
    for ext in [".ts", ".tsx", ".js"]:
      if base / "index{ext}" exists: return it
  return None  // punt on node_modules, tsconfig paths
```

| Pro | Con |
|---|---|
| Zero dependencies | No tsconfig paths |
| ~50 lines of code | No node_modules resolution |
| Total control | No package.json exports |
| | Must reimplement features over time |

### Option C: `node-resolve` crate

**Not viable.** Last release May 2018, 56K total downloads. No TS support, no ESM, dead project.

### Decision: Use oxc_resolver

The Code Graph is a code indexer — accurate import resolution is core, not a nice-to-have. A custom resolver would miss tsconfig paths, node_modules, and package.json exports. Pin to `oxc_resolver = "11"` for stability.

---

## R3: Rust CST Node Types for Extraction

### Declaration Node Kinds

| Source | `node.kind()` | Name Field | Notes |
|---|---|---|---|
| `fn foo() {}` | `function_item` | `name` → `identifier` | Top-level function |
| `struct Foo {}` | `struct_item` | `name` → `type_identifier` | |
| `enum Bar {}` | `enum_item` | `name` → `type_identifier` | |
| `trait Baz {}` | `trait_item` | `name` → `type_identifier` | |
| `type Alias = ...` | `type_item` | `name` → `type_identifier` | |
| `const X: i32 = 1` | `const_item` | `name` → `identifier` | |
| `static Y: &str` | `static_item` | `name` → `identifier` | |
| `macro_rules! m` | `macro_definition` | `name` → `identifier` | |
| `impl Foo {}` | `impl_item` | `type` → `type_identifier` | Methods inside → `function_item` |
| `impl Trait for Foo` | `impl_item` | `type` + `trait` fields | |
| `mod bar;` | `mod_item` | `name` → `identifier` | File module declaration |
| `mod bar { ... }` | `mod_item` | `name` → `identifier` | Inline module |
| `use crate::foo` | `use_declaration` | `argument` → various | See R4 below |

### `impl_item` Structure

```
impl_item
  ├── [visibility_modifier]     // pub impl
  ├── [type_parameters]         // generic params
  ├── [trait]                   // trait name (for trait impls)
  ├── type                      // implementing type
  ├── [where_clause]
  └── body: declaration_list
      ├── function_item          // methods
      ├── const_item             // associated constants
      └── type_item              // associated types
```

- **Inherent impl** (`impl Foo {}`): `trait` field is absent
- **Trait impl** (`impl Display for Foo {}`): `trait` field = "Display", `type` field = "Foo"
- Methods inside are regular `function_item` nodes
- `self` parameter: `self_parameter` node in `parameters` list

### Visibility Detection

- `visibility_modifier` child on declarations: `pub`, `pub(crate)`, `pub(super)`, `pub(self)`, `pub(in path)`
- No modifier → Private
- `pub` → Public
- `pub(crate)` → Crate visibility
- Check `child_by_field_name("visibility_modifier")` or iterate children for `kind() == "visibility_modifier"`

### Symbol Kind Mapping

| Rust Construct | SymbolKind | Notes |
|---|---|---|
| `fn` (top-level) | Function | |
| `fn` (in impl) | Method | ChildOf edge to parent type |
| `struct` | Struct | |
| `enum` | Enum | |
| `trait` | Trait | |
| `type X = ...` | TypeAlias | |
| `const` | Const | |
| `static` | Variable | Use Variable for statics |
| `macro_rules!` | Macro | |
| `mod` (file) | Not a symbol | Structural — maps to FileNode |

---

## R4: Rust `use` Declaration Extraction

### `use_declaration` Node Structure

The `argument` field can be one of:
- `identifier` — `use foo`
- `scoped_identifier` — `use crate::auth::validate`
- `scoped_use_list` — `use foo::{A, B}`
- `use_as_clause` — `use foo as bar`
- `use_wildcard` — `use foo::*`
- `crate` / `self` / `super` — path anchors

### Flattening `scoped_identifier`

`use crate::auth::validate` produces nested nodes:
```
scoped_identifier
  path: scoped_identifier
    path: crate
    name: "auth"
  name: "validate"
```

Walk the `path` chain recursively to flatten: `["crate", "auth", "validate"]`.

### `pub use` Detection

Check for `visibility_modifier` child on `use_declaration`:
- Present → `pub use` → `ReExport` edge
- Absent → normal `use` → `ImportsFrom` edge

### RawImport Mapping

| Rust `use` Pattern | RawImport Fields |
|---|---|
| `use crate::auth::validate` | specifier: `"crate::auth::validate"`, names: `[{name: "validate"}]` |
| `use crate::auth::*` | specifier: `"crate::auth"`, is_namespace: true |
| `use crate::{A, B}` | specifier: `"crate"`, names: `[{name: "A"}, {name: "B"}]` |
| `use foo as bar` | specifier: `"foo"`, names: `[{name: "foo", alias: Some("bar")}]` |
| `pub use self::greetings::hello` | Same as above + `is_reexport` flag (or metadata) |

---

## R5: Rust Module Resolution Algorithm

### Module Tree Construction

1. Find crate root: `src/lib.rs` (library) or `src/main.rs` (binary)
2. Parse crate root, collect all `mod foo;` declarations
3. For each `mod` declaration, resolve to file:
   - Check `src/foo.rs` (flat style, preferred since Rust 2018)
   - Check `src/foo/mod.rs` (legacy style)
   - Exactly one must exist (compiler enforces this)
4. Recursively parse found files for their `mod` declarations
5. Build mapping: `module_path → file_path`
   - `"crate::auth"` → `src/auth.rs`
   - `"crate::auth::middleware"` → `src/auth/middleware.rs`

### Use Path Resolution

For `use crate::auth::validate`:
1. Walk path: `"crate"` → crate root, `"auth"` → lookup in module tree → `src/auth.rs`
2. Check if `"validate"` is a symbol in `src/auth.rs` → `ImportsFrom` edge
3. If `"validate"` is a submodule, continue walking

For `use self::foo`: resolve `"self"` to current module path, then proceed.
For `use super::foo`: resolve `"super"` to parent module path, then proceed.

### Cargo Workspace Awareness

For cross-crate imports (`use other_crate::something`):
- Parse root `Cargo.toml` for `[workspace].members` and `[dependencies].*.path`
- Map crate names to directories (hyphens → underscores: `my-utils` → `my_utils`)
- Find target crate's `src/lib.rs`, walk module tree from there
- **v0.1 scope:** Only resolve workspace/path dependencies. Skip crates.io deps.

### What to Skip in v0.1

- `#[path = "..."]` attribute overrides (rare, log warning)
- `extern crate` declarations (2015 edition artifact)
- Procedural macro resolution (requires running the compiler)
- `#[cfg(...)]` conditional compilation
- crates.io dependency resolution
- `use foo as _` underscore imports

### Complexity Estimate

- Rust parser (`rust_lang.rs`): ~600-800 lines
- Rust resolver (`resolver/rust_lang.rs`): ~200-300 lines

---

## R6: Python CST Node Types for Extraction

### Declaration Node Kinds

| Source | `node.kind()` | Name Field | Notes |
|---|---|---|---|
| `def foo():` | `function_definition` | `name` → `identifier` | |
| `async def foo():` | `function_definition` | `name` → `identifier` | Check for `async` keyword |
| `class Bar:` | `class_definition` | `name` → `identifier` | |
| `@decorator` | `decorated_definition` | Wraps function/class | |
| `x = 1` | `expression_statement` → `assignment` | Left side → `identifier` | Top-level only |
| `X: int = 1` | `expression_statement` → `assignment` | With type annotation | |

### Import Node Kinds

| Source | `node.kind()` | Key Fields |
|---|---|---|
| `import foo` | `import_statement` | `name` → `dotted_name` or `aliased_import` |
| `from foo import bar` | `import_from_statement` | `module_name` → `dotted_name`; `name` → imports |
| `from foo import *` | `import_from_statement` | child: `wildcard_import` |
| `from . import foo` | `import_from_statement` | `module_name` → `relative_import` → `import_prefix` + `dotted_name` |

**`import_prefix`**: `repeat1('.')` — count dots for relative level (1 = current package, 2 = parent).

### Class Members

- Methods are `function_definition` nodes inside `class_definition.body` (a `block`)
- `self` is the first parameter (by convention, not enforced by tree-sitter)
- `@staticmethod` / `@classmethod` detected via `decorated_definition` wrapping
- `@property` marks a Property symbol

### Decorator Extraction

```
decorated_definition
  ├── decorator
  │   └── identifier OR call (for @decorator(args))
  └── function_definition OR class_definition
```

### Visibility Rules

- `_prefix` → Private
- `__prefix` (without `__suffix__`) → Private (name-mangled)
- `__dunder__` → Public (special methods)
- No prefix → Public

### Python-Specific RawImport Fields

| Pattern | Fields |
|---|---|
| `import os` | specifier: `"os"`, names: `[{name: "os"}]` |
| `from os.path import join` | specifier: `"os.path"`, names: `[{name: "join"}]` |
| `from . import models` | specifier: `"."`, names: `[{name: "models"}]` |
| `from ..utils import helper` | specifier: `".."`, names: `[{name: "helper"}]` (with relative_level metadata) |

---

## R7: Python Import Resolution Algorithm

### Resolution Rules

```
resolve_python_import(raw_import, current_file, project_root):
  specifier = raw_import.specifier

  if is_relative(specifier):  // starts with dots
    dot_count = count_leading_dots(specifier)
    module_path = strip_dots(specifier)
    base_dir = parent_dir(current_file)
    for _ in 1..dot_count: base_dir = parent_dir(base_dir)
    candidate = base_dir / module_path.replace('.', '/')
    return try_resolve(candidate)

  if is_stdlib(first_segment(specifier)):
    return None  // skip

  // Absolute local import
  candidate = project_root / specifier.replace('.', '/')
  return try_resolve(candidate)  // .py or /__init__.py
```

**`try_resolve(path)`**: Check `{path}.py`, then `{path}/__init__.py`. Return first match.

### Standard Library Detection

Maintain a ~150-entry set of Python stdlib top-level module names (steal from ruff/isort). Check `first_segment(specifier)` against this set.

### Special Cases

**TYPE_CHECKING blocks** → `ConditionalImport` edge:
- Detect `if_statement` where condition is `identifier` named `"TYPE_CHECKING"` or `attribute` ending in `.TYPE_CHECKING`
- All imports inside the `consequence` block are conditional

**try/except ImportError** → `ConditionalImport` edge:
- Detect `try_statement` with `except_clause` catching `ImportError` or `ModuleNotFoundError`
- All imports inside the `body` are conditional

**`__all__`** — not critical for import resolution but useful for `is_exported` determination.

### Complexity Estimate

- Python parser (`python.rs`): ~500-600 lines
- Python resolver (`resolver/python.rs`): ~100-150 lines

---

## R8: Go CST Node Types for Extraction

### Declaration Node Kinds

| Source | `node.kind()` | Name Field | Notes |
|---|---|---|---|
| `func foo() {}` | `function_declaration` | `name` → `identifier` | Package-level function |
| `func (r *Foo) Bar()` | `method_declaration` | `name` → `field_identifier` | Receiver method |
| `type Foo struct {}` | `type_declaration` → `type_spec` | `name` → `type_identifier` | |
| `type Bar interface {}` | `type_declaration` → `type_spec` | `name` → `type_identifier` | |
| `const X = 1` | `const_declaration` | Inside `const_spec`: `name` → `identifier` | |
| `var Y string` | `var_declaration` | Inside `var_spec`: `name` → `identifier` | |
| `type Alias = Other` | `type_declaration` → `type_alias` | `name` → `type_identifier` | |

### Import Node Kinds

| Source | `node.kind()` | Key Fields |
|---|---|---|
| `import "fmt"` | `import_declaration` → `import_spec` | `path` → string literal |
| `import _ "lib/pq"` | `import_spec` | `name` → `blank_identifier` → **SideEffectImport** |
| `import . "fmt"` | `import_spec` | `name` → `dot` → **DotImport** |
| `import alias "pkg"` | `import_spec` | `name` → `package_identifier` → aliased import |

### Struct Embedding Detection

```
field_declaration node:
  - Has 'name' field (field_identifier) → normal field, not embedding
  - No 'name' field, only 'type' field → EMBEDDING → Embeds edge
  - Type can be: type_identifier, qualified_type (pkg.Bar), pointer_type (*Bar)
```

### Method Receiver Association

```
method_declaration
  ├── receiver: parameter_list
  │   └── parameter_declaration
  │       ├── [name]: identifier    // receiver variable name
  │       └── type: type_identifier OR pointer_type(*Foo)
  ├── name: field_identifier        // method name
  ├── parameters: parameter_list
  └── [result]
```

Extract receiver type name → create `ChildOf` edge from `file::ReceiverType.MethodName` to `file::ReceiverType`.

### Visibility Rules

- First character of identifier is uppercase → `Visibility::Public`
- First character is lowercase → `Visibility::Private`
- Simple `char::is_uppercase()` check

---

## R9: Go Import Resolution Algorithm

### go.mod Parsing

Only the `module` line matters:
```
module github.com/user/myapp
```
Parse: find line starting with `module `, extract the path. ~5 lines of code.

### Resolution Rules

```
resolve_go_import(import_path, module_path, project_root):
  path = strip_quotes(import_path)

  // Standard library: first element has no dots
  if !first_element(path).contains('.'):
    return None  // stdlib, skip

  // Local module: starts with our module path
  if path.starts_with(module_path):
    relative = strip_prefix(path, module_path)
    dir = project_root / relative
    return Some(dir)  // resolve to package directory

  return None  // external, skip
```

### Package = Directory

All `.go` files in the same directory are the same package. When creating `ImportsFrom` edges, point to the directory (or all `.go` files in it).

### Interface Satisfaction

Go interfaces are implicit — no `implements` keyword. Detection requires full type information across packages. **Skip for v0.1.** This is a known limitation.

### Complexity Estimate

- Go parser (`go.rs`): ~400-500 lines
- Go resolver (`resolver/go.rs`): ~50 lines

---

## R10: Barrel Chain Traversal Strategy (TS/JS)

### Problem

When resolving `import { UserService } from "./services"`, the target `services/index.ts` may be a barrel file that re-exports from deeper modules:

```typescript
// services/index.ts
export { UserService } from "./user";
export { AuthService } from "./auth";
export * from "./common";
```

We need to trace `UserService` through the re-export chain to find its origin file.

### Algorithm

```
trace_barrel_chain(name, target_file, parsed_files, visited):
  if target_file in visited: return None  // cycle
  visited.add(target_file)
  if visited.len() > MAX_BARREL_DEPTH: return None  // too deep

  exports = parsed_files[target_file].exports

  // Check if name is defined locally
  if name in local_symbols(target_file): return target_file

  // Check named re-exports
  for export in exports where export.is_reexport:
    if export.name == name:
      resolved_source = oxc_resolver.resolve(target_dir, export.source_specifier)
      return trace_barrel_chain(name, resolved_source, parsed_files, visited)

  // Check star re-exports (export * from "...")
  for export in exports where export.is_star_reexport:
    resolved_source = oxc_resolver.resolve(target_dir, export.source_specifier)
    result = trace_barrel_chain(name, resolved_source, parsed_files, visited)
    if result.is_some(): return result

  return None  // not found in this chain
```

### Design Decisions

- **Max depth:** 10 hops (covers any real-world project; deeper chains are pathological)
- **Cycle detection:** `HashSet<PathBuf>` of visited files, passed through recursion
- **Star re-export ordering:** Check named re-exports first (deterministic), then star re-exports in declaration order
- **`BarrelReExportAll` edge:** Created for each `export * from "..."` statement (File → File edge)
- **Performance:** Barrel traversal is per-import-name, but most imports resolve in 1-2 hops. Cache results per (file, name) pair.

---

## R11: rayon Ownership Decision

### Background

Design spec (Section 2.2) lists `rayon` as a parser dependency. S03 explicitly excluded it: "the parser provides Send + Sync traits; the caller parallelizes."

### Analysis

The parser crate has two parallelism surfaces:
1. **File parsing** — parsing N files in parallel (embarrassingly parallel)
2. **Import resolution** — resolving imports across files (needs shared state: all ParseResults)

Both require access to the `ParserRegistry` (Send + Sync ✓) and the resolved file list.

### Options

| Option | Where rayon Lives | Pros | Cons |
|---|---|---|---|
| A: CLI owns rayon | `cli` crate | Clean separation; parser stays a pure library | CLI must orchestrate the two-phase pipeline |
| B: Parser owns rayon | `parser` crate | Resolution pipeline is self-contained | Couples parallelism to the parser |
| C: Both | Both crates | Each layer parallelizes its own concern | Two rayon thread pools, complex |

### Decision: CLI owns rayon (Option A)

The parser crate should remain a library of pure functions/traits. The orchestration of "parse all files in parallel → resolve all imports" is a higher-level concern belonging to `IndexUseCase` (called from CLI). The parser provides:
- `LanguageParser::parse()` — single-file, pure, Send + Sync
- `ImportResolver::resolve()` — takes all ParseResults, produces resolved edges

The CLI/IndexUseCase calls these via rayon's `par_iter()`. This matches S03's design and the hexagonal architecture.

**Consequence:** `rayon` is NOT a parser dependency. It goes in `cli` (or wherever `IndexUseCase` is wired).

---

## R12: Import Resolver Trait Design

### Proposed Interface

```rust
/// Trait for language-specific import resolvers.
/// Called after all files are parsed (Phase 2 of parse-then-resolve).
pub trait ImportResolver: Send + Sync {
    /// Which languages this resolver handles.
    fn languages(&self) -> &[Language];

    /// Resolve raw imports from a single file into graph edges.
    /// Has access to all parsed results for cross-file resolution.
    fn resolve(
        &self,
        file_path: &Path,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> Vec<Edge>;
}

/// Shared context for resolution — all parsed files and their results.
pub struct ResolveContext {
    pub project_root: PathBuf,
    pub parsed_files: HashMap<PathBuf, ParseResult>,
    pub file_tree: Vec<PathBuf>,  // all files in project
}
```

### Resolver Implementations

| File | Languages | Key Dependencies |
|---|---|---|
| `resolver/typescript.rs` | TypeScript, JavaScript | `oxc_resolver` for file resolution, barrel chain traversal |
| `resolver/rust_lang.rs` | Rust | Module tree walker, Cargo.toml parsing (minimal) |
| `resolver/python.rs` | Python | Filesystem probing, stdlib set |
| `resolver/go.rs` | Go | `go.mod` parsing (string ops only) |

### ResolverRegistry (in resolver/mod.rs)

```rust
pub struct ResolverRegistry {
    resolvers: Vec<Box<dyn ImportResolver>>,
}

impl ResolverRegistry {
    pub fn new(project_root: &Path) -> Self;
    pub fn resolve_all(&self, context: &ResolveContext) -> Vec<Edge>;
}
```

---

## R13: RawImport Adequacy Check

The existing `RawImport` struct from S03 needs minor extensions for non-TS/JS languages:

| Field | TS/JS | Rust | Python | Go | Action |
|---|---|---|---|---|---|
| `specifier` | `"./utils"` | `"crate::auth::validate"` | `"..utils"` | `"github.com/user/pkg"` | Works as-is |
| `names` | Named imports | Use tree items | `from X import a,b` | N/A (package-level) | Works as-is |
| `is_type_only` | `import type` | N/A | TYPE_CHECKING | N/A | Works |
| `is_side_effect` | `import "./polyfill"` | N/A | N/A | `import _` | Works |
| `is_namespace` | `import * as ns` | `use foo::*` | `from foo import *` | `import . "fmt"` | Works |
| `line` | Line number | Line number | Line number | Line number | Works |

**New field needed:** None. The existing struct covers all patterns. Language-specific semantics (e.g., Rust's `crate::` prefix, Python's dot counting) are handled by each resolver, not encoded in the struct.

**Consideration:** Should we add a `is_reexport` field? No — `pub use` in Rust is detected at parse time via visibility modifier. TS/JS re-exports are already in the `Export` struct. The resolver creates the correct edge kind (`ReExport` vs `ImportsFrom`) based on this context.

---

## R14: SPEC Corrections from Research

1. **Grammar crate versions** — Use `tree-sitter-rust = "0.24"`, `tree-sitter-python = "0.23"`, `tree-sitter-go = "0.23"`. Verify minimum available versions during implementation.

2. **Go implicit interfaces** — Skip `Implements` edge detection for Go in v0.1. Requires full type analysis.

3. **Rust cross-file `impl`** — Same-file: create `ChildOf` directly. Cross-file: defer to post-processing pass after all files parsed.

4. **Barrel chain** — Max 10 hops, visited set for cycle detection, named re-exports checked before star re-exports.

5. **rayon not a parser dep** — Goes in CLI/IndexUseCase, not parser crate.

6. **Cross-file call resolution** — Deferred to S05 per discussion decision. S04 handles import resolution only.

---

## Summary of Decisions

| Question | Decision | Rationale |
|---|---|---|
| Grammar versions | rust 0.24, python 0.23, go 0.23 | Conservative, ABI compatible via tree-sitter-language bridge |
| TS/JS resolver | oxc_resolver | Battle-tested, tsconfig paths built-in, no viable alternative |
| Rust resolver | Custom mod tree walker | ~200-300 lines, no suitable crate exists |
| Python resolver | Custom filesystem prober | ~100-150 lines, no suitable crate exists |
| Go resolver | Custom go.mod parser | ~50 lines, trivially simple |
| rayon ownership | CLI (not parser) | Parser stays pure library; orchestration is CLI concern |
| Barrel chain depth | Max 10 hops + visited set | Covers real-world projects, prevents infinite loops |
| Cross-file calls | Deferred to S05 | Needs full index pipeline, not just resolution |
| Go Implements | Skip in v0.1 | Requires full type analysis across packages |
| Rust cross-file impl | Post-processing pass | After all files parsed, resolve type references |
| Cargo.toml parsing | Minimal (name, workspace, path deps) | Just enough for crate name → directory mapping |
| Python stdlib | Hardcoded ~150-entry set | Steal from ruff/isort |
