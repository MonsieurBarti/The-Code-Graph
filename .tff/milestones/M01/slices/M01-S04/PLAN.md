# Plan — M01-S04: Language Parsers & Import Resolution

> For agentic workers: execute task-by-task with TDD.

**Goal:** Add Rust, Python, Go parsers + import resolution pipeline for all 5 languages, converting raw parse output into a connected graph with cross-file edges.

**Architecture:** Extends the parser crate (hexagonal adapter). New parsers implement `LanguageParser`. New resolvers implement `ImportResolver`. No new crate boundaries.

**Tech Stack:** tree-sitter-rust 0.24, tree-sitter-python 0.23, tree-sitter-go 0.23, oxc_resolver 11, toml 0.8

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/parser/Cargo.toml` | Modify | Add new grammar + resolver dependencies |
| `crates/parser/src/lib.rs` | Modify | Add module declarations + re-exports |
| `crates/parser/src/registry.rs` | Modify | Register Rust, Python, Go parsers |
| `crates/parser/src/rust_lang.rs` | Create | Rust parser (symbols, edges, imports, mod decls) |
| `crates/parser/src/python.rs` | Create | Python parser (symbols, edges, imports, decorators) |
| `crates/parser/src/go.rs` | Create | Go parser (symbols, edges, imports, embedding) |
| `crates/parser/src/resolver/mod.rs` | Create | ImportResolver trait, ResolverRegistry, ResolveContext |
| `crates/parser/src/resolver/typescript.rs` | Create | oxc_resolver + barrel chain traversal |
| `crates/parser/src/resolver/rust_lang.rs` | Create | Module tree walker + use path resolution |
| `crates/parser/src/resolver/python.rs` | Create | Filesystem prober + stdlib set |
| `crates/parser/src/resolver/go.rs` | Create | go.mod parser + module path resolution |
| `crates/parser/src/test_utils.rs` | Modify | Add helpers for Rust, Python, Go test fixtures |

---

## Wave 0 — Dependencies & Scaffold

### T01: Add dependencies and resolver module scaffold
**AC coverage:** AC1
**Files:** `crates/parser/Cargo.toml`, `crates/parser/src/lib.rs`, `crates/parser/src/resolver/mod.rs`

1. Add to `crates/parser/Cargo.toml`:
   ```toml
   tree-sitter-rust = "0.24"
   tree-sitter-python = "0.23"
   tree-sitter-go = "0.23"
   oxc_resolver = "11"
   toml = "0.8"
   ```
2. Add module declarations in `lib.rs`:
   ```rust
   mod rust_lang;
   mod python;
   mod go;
   pub mod resolver;
   ```
   Re-export new parser types.
3. Create `resolver/mod.rs` with:
   - `ImportResolver` trait (languages, resolve)
   - `ResolveContext` struct (project_root, parsed_files, file_tree)
   - `ResolverRegistry` struct (new, resolver_for_language, resolve_file)
   - Placeholder `register_all()` that starts empty
4. `cargo build -p parser` succeeds (AC1)

---

## Wave 1 — Language Parsers (parallel — all 3 are independent)

### T02: Rust parser — symbol extraction + structural edges
**AC coverage:** AC3, AC6, AC7, AC8, AC9, AC10, AC14, AC49 (Rust), AC50 (Rust)
**Files:** `crates/parser/src/rust_lang.rs`

1. Write tests first:
   - `fn foo() {}` → Function, name="foo" (AC6)
   - `struct Bar { x: i32 }` → Struct (AC7)
   - `impl Foo { fn bar(&self) {} }` → Method + ChildOf edge (AC8)
   - `trait Baz { fn required(); }` → Trait (AC9)
   - `enum Color { Red, Green }` → Enum (AC10)
   - `const X: i32 = 42;` → Const
   - `type Alias = Vec<u8>;` → TypeAlias
   - `macro_rules! m { () => {} }` → Macro
   - `pub fn` → Public, `pub(crate) fn` → Crate, `fn` → Private (AC14)
   - `impl Display for Foo {}` → Implements edge (same file)
   - Contains edges: file → each top-level symbol
   - Qualified names: `file::Name`, `file::Struct.method`
   - Empty/invalid source → CodeGraphError::Parse (AC49)
   - Source with errors → partial extraction (AC50)
2. Implement `RustParser` struct:
   - Store `lang: LanguageFn` from `tree_sitter_rust::LANGUAGE`
   - Thread-local parser via shared `thread_local!`
   - `LanguageParser` impl: language() → Rust, file_extensions() → `["rs"]`
3. Implement extraction:
   - Walk root named_children: `function_item`, `struct_item`, `enum_item`, `trait_item`, `type_item`, `const_item`, `static_item`, `macro_definition`, `impl_item`
   - For `impl_item`: extract receiver type from `type` field, iterate `declaration_list` for methods
   - Build Contains edges (file → symbol), ChildOf edges (method → type)
   - Build Implements edge for trait impls (same file)
   - Extract visibility from `visibility_modifier` child

### T03: Rust parser — use/mod declaration extraction
**AC coverage:** AC11, AC12, AC13
**Files:** `crates/parser/src/rust_lang.rs`
**Depends on:** T02

1. Write tests first:
   - `use crate::auth::validate;` → RawImport, specifier="crate::auth::validate" (AC11)
   - `pub use self::greetings::hello;` → RawImport marked reexport (AC12)
   - `use foo::{A, B};` → RawImport with 2 names
   - `use foo::*;` → RawImport with is_namespace=true
   - `use foo as bar;` → RawImport with alias
   - `mod submodule;` → captured in a new field or side structure (AC13)
2. Implement `use_declaration` extraction:
   - Recursively flatten `scoped_identifier` chains
   - Handle `scoped_use_list`, `use_as_clause`, `use_wildcard`
   - Detect `pub use` via `visibility_modifier`
   - Store `mod` declarations separately (needed by resolver)
3. Add `mod_declarations: Vec<ModDeclaration>` to `ParseResult` or as a separate return
   - Actually: store mod names in RawImport with a distinguishing specifier prefix like `"mod::{name}"` or add a new `mods: Vec<String>` field to ParseResult

### T04: Python parser — symbol extraction + imports
**AC coverage:** AC4, AC15, AC16, AC17, AC18, AC19, AC20, AC21, AC49 (Python), AC50 (Python)
**Files:** `crates/parser/src/python.rs`

1. Write tests first:
   - `def foo(): pass` → Function (AC15)
   - `class Bar:\n  def method(self): pass` → Class + Method + ChildOf (AC16)
   - `async def foo(): pass` → Function, is_async=true (AC19)
   - `@decorator\ndef foo(): pass` → decorators=["@decorator"] (AC20)
   - `@property\ndef prop(self): pass` → Property
   - `class Foo(Bar):` → Extends edge (same file)
   - `from .models import User` → RawImport, specifier=".models" (AC17)
   - `import os.path` → RawImport, specifier="os.path" (AC18)
   - `from .. import utils` → RawImport with relative dots
   - `from foo import *` → is_namespace=true
   - TYPE_CHECKING detection (AC21)
   - `_private` → Private, `public` → Public visibility
   - Empty/invalid source → error/partial (AC49, AC50)
2. Implement `PythonParser` struct:
   - Store `lang: LanguageFn` from `tree_sitter_python::LANGUAGE`
   - Thread-local parser
   - `LanguageParser` impl: language() → Python, file_extensions() → `["py"]`
3. Implement extraction:
   - Walk root: `function_definition`, `class_definition`, `decorated_definition`, `import_statement`, `import_from_statement`, `expression_statement` (for assignments)
   - For classes: recurse into body block for methods
   - Detect `async` keyword on function_definition
   - Extract decorators from `decorated_definition`
   - Import extraction: handle `dotted_name`, `relative_import`, `import_prefix`, `wildcard_import`
   - TYPE_CHECKING: detect `if_statement` with condition matching "TYPE_CHECKING"

### T05: Go parser — symbol extraction + imports + embedding
**AC coverage:** AC5, AC22, AC23, AC24, AC25, AC26, AC27, AC28, AC49 (Go), AC50 (Go)
**Files:** `crates/parser/src/go.rs`

1. Write tests first:
   - `func Foo() {}` → Function (AC22)
   - `func (r *Bar) Method() {}` → Method + ChildOf edge (AC23)
   - `type Foo struct { Bar }` → Struct + Embeds edge (AC24)
   - `type Baz interface { Method() }` → Interface (AC25)
   - `const X = 1` → Const
   - `var Y string` → Variable
   - `import _ "lib/pq"` → SideEffectImport (AC26)
   - `import . "fmt"` → DotImport (AC27)
   - `import "fmt"` → normal import
   - `import alias "pkg"` → aliased import
   - Capitalized → Public, lowercase → Private (AC28)
   - Empty/invalid source → error/partial (AC49, AC50)
2. Implement `GoParser` struct:
   - Store `lang: LanguageFn` from `tree_sitter_go::LANGUAGE`
   - Thread-local parser
   - `LanguageParser` impl: language() → Go, file_extensions() → `["go"]`
3. Implement extraction:
   - Walk root: `function_declaration`, `method_declaration`, `type_declaration`, `const_declaration`, `var_declaration`, `import_declaration`
   - For `method_declaration`: extract receiver type from `parameter_list`
   - For `type_declaration` → `type_spec`: check underlying type (`struct_type`, `interface_type`)
   - Struct embedding: `field_declaration` with no `name` field → Embeds edge
   - Import: `import_spec` → check `name` field for `blank_identifier` (side-effect) or `dot` (dot import)
   - Visibility: first char of identifier

---

## Wave 2 — Registry Update + Resolver Implementations (parallel — resolvers are independent)

### T06: Update ParserRegistry + test_utils for all languages
**AC coverage:** AC2, AC3, AC4, AC5, AC51
**Files:** `crates/parser/src/registry.rs`, `crates/parser/src/test_utils.rs`, `crates/parser/src/lib.rs`
**Depends on:** T02, T04, T05

1. Write tests first:
   - `parser_for_file("foo.rs")` → Some, language == Rust (AC3)
   - `parser_for_file("foo.py")` → Some, language == Python (AC4)
   - `parser_for_file("foo.go")` → Some, language == Go (AC5)
   - `ParserRegistry::new()` has 5 parsers registered (AC2)
   - `supported_extensions()` includes rs, py, go
   - Thread safety: parse from 2 threads concurrently, each language (AC51)
2. Register `RustParser`, `PythonParser`, `GoParser` in `ParserRegistry::new()`
3. Add test helpers to `test_utils.rs`:
   - `parse_rust(source) -> ParseResult`
   - `parse_python(source) -> ParseResult`
   - `parse_go(source) -> ParseResult`

### T07: TS/JS import resolver with oxc_resolver + barrel chain
**AC coverage:** AC29, AC30, AC31, AC32, AC33, AC34
**Files:** `crates/parser/src/resolver/typescript.rs`
**Depends on:** T01

1. Write tests first:
   - Resolve `import { foo } from "./utils"` → target file (AC29)
   - Barrel chain traversal through `index.ts` re-exports (AC30)
   - Circular barrel → graceful termination, no panic (AC31)
   - `ImportsFrom` edge created (AC32)
   - `export * from "./mod"` → `BarrelReExportAll` edge (AC33)
   - `export { X } from "./mod"` → `ReExport` edge (AC34)
2. Implement `TypeScriptResolver`:
   - Construct `oxc_resolver::Resolver` with configured options
   - For each RawImport: resolve specifier → file path
   - For each resolved import: check if target has re-exports, trace barrel chain
   - Barrel chain: recursive with visited set (max 10 hops)
   - Create edges: `ImportsFrom` (file → file), `ReExport`, `BarrelReExportAll`
3. Register in `ResolverRegistry`

### T08: Rust import resolver — module tree + use path resolution
**AC coverage:** AC35, AC36, AC37, AC38, AC39
**Files:** `crates/parser/src/resolver/rust_lang.rs`
**Depends on:** T01, T03

1. Write tests first:
   - Build module tree from test fixture with `mod` declarations (AC35)
   - Resolve `use crate::auth::validate` → `src/auth.rs` (AC36)
   - Resolve `use self::sub` → relative to current module (AC37)
   - `pub use` → `ReExport` edge (AC38)
   - Both `foo.rs` and `foo/mod.rs` naming (AC39)
2. Implement `RustResolver`:
   - `build_module_tree(root_file, parsed_files)` → `HashMap<String, PathBuf>` (module_path → file_path)
   - Walk crate root's `mod` declarations recursively
   - Check both `{name}.rs` and `{name}/mod.rs`
   - Minimal Cargo.toml parsing: extract `[package].name`, `[workspace].members`, `[dependencies].*.path`
   - `resolve_use(use_path, module_tree)` → target file path
   - Create `ImportsFrom` or `ReExport` edges based on visibility
3. Register in `ResolverRegistry`

### T09: Python import resolver — filesystem prober + stdlib
**AC coverage:** AC40, AC41, AC42, AC43
**Files:** `crates/parser/src/resolver/python.rs`
**Depends on:** T01

1. Write tests first:
   - `from .models import User` → resolves to sibling `models.py` (AC40)
   - `from ..utils import helper` → walks up 2 levels (AC41)
   - `import os` → stdlib, no edge (AC42)
   - TYPE_CHECKING import → `ConditionalImport` edge (AC43)
2. Implement `PythonResolver`:
   - `STDLIB_MODULES: HashSet<&str>` — ~150 top-level module names
   - `resolve_import(specifier, current_file, project_root)`:
     - Relative: count dots, walk up, probe `{module}.py` or `{module}/__init__.py`
     - Stdlib check: first segment in STDLIB_MODULES → None
     - Absolute: probe project tree
   - Create `ImportsFrom` or `ConditionalImport` edges
3. Register in `ResolverRegistry`

### T10: Go import resolver — go.mod + module path
**AC coverage:** AC44, AC45, AC46, AC47, AC48
**Files:** `crates/parser/src/resolver/go.rs`
**Depends on:** T01

1. Write tests first:
   - Parse go.mod → extract module path (AC44)
   - Local import → strip prefix, resolve to directory (AC45)
   - Stdlib (`import "fmt"`) → no edge (AC46)
   - Blank import → `SideEffectImport` edge (AC47)
   - Dot import → `DotImport` edge (AC48)
2. Implement `GoResolver`:
   - `parse_go_mod(project_root)` → `Option<String>` (module path)
   - `resolve_import(import_path, module_path, project_root)`:
     - No dots in first element → stdlib, skip
     - Starts with module path → local, strip prefix
     - Otherwise → external, skip
   - Create appropriate edge type based on import kind
3. Register in `ResolverRegistry`

---

## Wave 3 — Integration + Polish

### T11: Integration tests, thread safety, clippy, final verification
**AC coverage:** AC51, AC52, AC53
**Files:** Various test files
**Depends on:** T06, T07, T08, T09, T10

1. Write integration tests:
   - Multi-construct Rust file (struct + impl + use + mod) → complete ParseResult
   - Multi-construct Python file (class + methods + imports + decorators) → complete ParseResult
   - Multi-construct Go file (struct + methods + interface + imports) → complete ParseResult
   - TS/JS resolver end-to-end with mock file tree
   - Rust resolver end-to-end with mock module tree
   - Python resolver end-to-end with mock file tree
   - Go resolver end-to-end with mock go.mod
2. Thread safety: parse from 2+ threads, each language (AC51)
3. `cargo test -p parser` passes all tests (AC52)
4. `cargo clippy -p parser -- -Dwarnings` passes (AC53)
5. `cargo build --workspace` succeeds
6. `cargo test --workspace` passes

---

## Task Dependency Graph

```
T01 (scaffold) ──┬──► T02 (Rust parser) ──┬──► T03 (Rust use/mod) ──┐
                 │                         │                          │
                 ├──► T04 (Python parser) ─┤                          │
                 │                         │                          │
                 ├──► T05 (Go parser) ─────┤                          │
                 │                         │                          │
                 │                         └──► T06 (registry) ───────┤
                 │                                                    │
                 ├──► T07 (TS/JS resolver) ───────────────────────────┤
                 │                                                    │
                 ├──► T08 (Rust resolver) ← T03 ─────────────────────┤
                 │                                                    │
                 ├──► T09 (Python resolver) ──────────────────────────┤
                 │                                                    │
                 └──► T10 (Go resolver) ──────────────────────────────┤
                                                                      │
                                                                      └──► T11 (integration)
```

## Complexity Estimate

| Task | Size | Notes |
|------|------|-------|
| T01 | S | Deps + scaffold, straightforward |
| T02 | L | ~600 lines — 10+ node types, impl blocks, visibility |
| T03 | M | ~200 lines — use declaration flattening, mod extraction |
| T04 | L | ~500 lines — class/function extraction, decorators, relative imports |
| T05 | M-L | ~400 lines — structs, methods with receivers, embedding |
| T06 | S | Registry update + test helpers |
| T07 | L | ~400 lines — oxc_resolver integration, barrel chain algorithm |
| T08 | M-L | ~300 lines — module tree walker, Cargo.toml parsing |
| T09 | M | ~200 lines — filesystem probing, stdlib set |
| T10 | S-M | ~100 lines — go.mod parsing, prefix stripping |
| T11 | M | Integration tests, clippy, thread safety |

**Total estimated:** ~2,700-3,200 lines of new code + tests

## AC Traceability Matrix

| AC | Task | Verified By |
|----|------|-------------|
| AC1 | T01 | `cargo build --workspace` |
| AC2 | T06 | Test: registry has 5 parsers |
| AC3 | T06 | Test: `parser_for_file("foo.rs")` |
| AC4 | T06 | Test: `parser_for_file("foo.py")` |
| AC5 | T06 | Test: `parser_for_file("foo.go")` |
| AC6 | T02 | Test: Rust function extraction |
| AC7 | T02 | Test: Rust struct extraction |
| AC8 | T02 | Test: Rust impl method + ChildOf |
| AC9 | T02 | Test: Rust trait extraction |
| AC10 | T02 | Test: Rust enum extraction |
| AC11 | T03 | Test: Rust use declaration |
| AC12 | T03 | Test: Rust pub use reexport |
| AC13 | T03 | Test: Rust mod declaration |
| AC14 | T02 | Test: Rust visibility levels |
| AC15 | T04 | Test: Python function |
| AC16 | T04 | Test: Python class + methods |
| AC17 | T04 | Test: Python relative import |
| AC18 | T04 | Test: Python absolute import |
| AC19 | T04 | Test: Python async function |
| AC20 | T04 | Test: Python decorators |
| AC21 | T04 | Test: Python TYPE_CHECKING |
| AC22 | T05 | Test: Go function |
| AC23 | T05 | Test: Go receiver method |
| AC24 | T05 | Test: Go struct embedding |
| AC25 | T05 | Test: Go interface |
| AC26 | T05 | Test: Go blank import |
| AC27 | T05 | Test: Go dot import |
| AC28 | T05 | Test: Go visibility |
| AC29 | T07 | Test: TS/JS import resolution |
| AC30 | T07 | Test: barrel chain traversal |
| AC31 | T07 | Test: circular barrel |
| AC32 | T07 | Test: ImportsFrom edge |
| AC33 | T07 | Test: BarrelReExportAll edge |
| AC34 | T07 | Test: ReExport edge |
| AC35 | T08 | Test: module tree construction |
| AC36 | T08 | Test: crate:: path resolution |
| AC37 | T08 | Test: self:: resolution |
| AC38 | T08 | Test: pub use → ReExport |
| AC39 | T08 | Test: foo.rs vs foo/mod.rs |
| AC40 | T09 | Test: Python relative resolve |
| AC41 | T09 | Test: Python double-dot resolve |
| AC42 | T09 | Test: Python stdlib skip |
| AC43 | T09 | Test: ConditionalImport edge |
| AC44 | T10 | Test: go.mod parsing |
| AC45 | T10 | Test: local import resolution |
| AC46 | T10 | Test: stdlib skip |
| AC47 | T10 | Test: SideEffectImport edge |
| AC48 | T10 | Test: DotImport edge |
| AC49 | T02, T04, T05 | Tests: error handling per language |
| AC50 | T02, T04, T05 | Tests: partial extraction |
| AC51 | T06, T11 | Tests: thread safety |
| AC52 | T11 | `cargo test -p parser` |
| AC53 | T11 | `cargo clippy -p parser -- -Dwarnings` |
