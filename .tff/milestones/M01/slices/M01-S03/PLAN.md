# Plan — M01-S03: Tree-Sitter Parser Infrastructure

## Wave 1: Scaffold + Types

### T1: Create parser crate with core types and LanguageParser trait
**AC coverage:** AC1, AC34 (partial)
**Files:** `Cargo.toml` (root), `crates/parser/Cargo.toml`, `crates/parser/src/lib.rs`

1. Create `crates/parser/` directory and `Cargo.toml` with dependencies:
   - `domain = { path = "../domain" }`
   - `tree-sitter = "0.24"`
   - `tree-sitter-typescript = "0.23"`
   - `tree-sitter-javascript = "0.23"`
   - dev: `serde_json = "1"`
2. Add `"crates/parser"` to workspace members in root `Cargo.toml`
3. Define in `lib.rs`:
   - `ParseResult { symbols, edges, imports, exports }`
   - `RawImport { specifier, names, is_type_only, is_side_effect, is_namespace, line }`
   - `ImportName { name, alias, is_type }`
   - `Export { name, local_name, is_default, is_type_only, is_reexport, source_specifier }`
   - `LanguageParser` trait (`language()`, `file_extensions()`, `parse()`)
4. `cargo build -p parser` succeeds
5. Unit tests for type construction and Default impls

---

## Wave 2: Infrastructure (parallel)

### T2: ParserRegistry with extension-based dispatch
**AC coverage:** AC2-AC9
**Files:** `crates/parser/src/registry.rs`, `crates/parser/src/lib.rs` (re-export)

1. Write tests first:
   - `parser_for_file("foo.ts")` → Some, language == TypeScript
   - `parser_for_file("foo.tsx")` → Some, language == TypeScript
   - `parser_for_file("foo.js")` → Some, language == JavaScript
   - `parser_for_file("foo.jsx")` → Some, language == JavaScript
   - `parser_for_file("foo.rs")` → None
   - `parser_for_file("foo.txt")` → None
   - `ParserRegistry` is Send + Sync (compile-time assertion)
2. Implement `ParserRegistry`:
   - `new()` → registers TypeScriptParser
   - `register(Box<dyn LanguageParser>)` → builds extension map
   - `parser_for_file(&Path) -> Option<&dyn LanguageParser>`
   - `parser_for_language(Language) -> Option<&dyn LanguageParser>`
   - `supported_extensions() -> Vec<&str>`
3. Re-export from `lib.rs`

### T3: TypeScriptParser skeleton with thread-local management
**AC coverage:** AC3-AC6 (via registry integration), AC31 (partial)
**Files:** `crates/parser/src/typescript.rs`, `crates/parser/src/lib.rs` (re-export)
**Depends on:** T1 (types exist)

1. Write tests first:
   - Construct TypeScriptParser, verify language() and file_extensions()
   - Parse empty `.ts` file → Ok(ParseResult) with empty vecs
   - Parse empty `.js` file → Ok(ParseResult) with empty vecs
2. Implement `TypeScriptParser`:
   - Struct stores `ts_lang: LanguageFn`, `tsx_lang: LanguageFn`, `js_lang: LanguageFn`
   - `new()` constructor
   - `language_for_path(&Path) -> Language` (extension dispatch to LanguageFn)
   - Thread-local: `thread_local! { static PARSER: RefCell<Parser> = ... }`
   - `parse()` skeleton: parse tree, return empty ParseResult
   - Private `extract()` method stub
3. Implement `LanguageParser` trait for `TypeScriptParser`

---

## Wave 3: Extraction (parallel)

### T4: TypeScript symbol extraction with edges and metadata
**AC coverage:** AC10-AC20
**Files:** `crates/parser/src/typescript.rs`
**Depends on:** T3

1. Write tests first (one test per AC):
   - `function foo() {}` → Function symbol, name="foo"
   - `class Bar { baz() {} }` → Class + Method + ChildOf edge
   - `interface IFoo { prop: string }` → Interface + Property
   - `type Alias = string` → TypeAlias
   - `enum Color { Red, Green }` → Enum
   - `export const handler = async () => {}` → Function, is_async=true, is_exported=true, visibility=Public
   - `export default function main() {}` → Function, is_exported=true
   - Non-exported → visibility=Private, is_exported=false
   - Contains edge from file path to each top-level symbol
   - ChildOf edge from method to class
   - Qualified names: `file_path::Name`, `file_path::Class.method`
2. Implement `extract_symbols()` helper:
   - Walk root named_children with TreeCursor
   - Match on node kinds: `function_declaration`, `class_declaration`, `abstract_class_declaration`, `interface_declaration`, `type_alias_declaration`, `enum_declaration`, `lexical_declaration`, `export_statement`
   - For each declaration: extract name, kind, location, visibility, metadata
   - For class/interface: recurse into body for method_definition, public_field_definition, property_signature, method_signature
   - Build Contains edges (file → top-level symbol)
   - Build ChildOf edges (member → parent)
   - Construct qualified names per spec format
3. Implement metadata extraction helpers:
   - `is_async()` — unnamed "async" child token
   - `is_exported()` — parent is export_statement
   - `extract_visibility()` — exported → Public, else Private
   - `extract_decorators()` — named "decorator" children
   - `extract_signature()` — parameters text + return_type
   - `extract_name()` — child_by_field_name("name"), handle type_identifier vs identifier vs property_identifier
   - `detect_test()` — name starts with "test" or has test decorator

### T5: Import statement extraction
**AC coverage:** AC21-AC25
**Files:** `crates/parser/src/typescript.rs`
**Depends on:** T3

1. Write tests first:
   - `import { a, b } from "./mod"` → RawImport, specifier="./mod", 2 ImportNames
   - `import type { T } from "./types"` → is_type_only=true
   - `import * as ns from "./ns"` → is_namespace=true
   - `import "./polyfill"` → is_side_effect=true
   - `import def from "./mod"` → name="default", alias=Some("def")
   - Mixed: `import def, { a } from "./mod"` → 2 ImportNames
2. Implement `extract_imports()` helper:
   - Walk root children for `import_statement` nodes
   - Extract `source` field → strip quotes → specifier
   - Detect `type` unnamed child → is_type_only
   - If no `import_clause` → side_effect import
   - If `import_clause` has `namespace_import` → is_namespace
   - If `import_clause` has direct `identifier` → default import (name="default", alias=Some(binding))
   - If `import_clause` has `named_imports` → iterate `import_specifier` children (name, alias)

### T6: Export statement extraction
**AC coverage:** AC26-AC28
**Files:** `crates/parser/src/typescript.rs`
**Depends on:** T3

1. Write tests first:
   - `export function foo() {}` → Export, name="foo", is_default=false
   - `export default class Bar {}` → name="default", local_name=Some("Bar"), is_default=true
   - `export { foo } from "./mod"` → is_reexport=true, source_specifier=Some("./mod")
   - `export { foo as bar }` → name="bar", local_name=Some("foo")
   - `export * from "./barrel"` → is_reexport=true, name="*"
   - `export type { Foo }` → is_type_only=true
   - `export default 42` → name="default", is_default=true
2. Implement `extract_exports()` helper:
   - Walk root children for `export_statement` nodes
   - Detect `default` unnamed child → is_default
   - Detect `type` unnamed child → is_type_only
   - If `declaration` field exists → export of declaration (name from declaration)
   - If `export_clause` → iterate `export_specifier` children (name, alias)
   - Check `source` field → re-export with source_specifier
   - Detect `*` unnamed child → star re-export

---

## Wave 4: Polish

### T7: Error handling, thread safety, integration tests, clippy
**AC coverage:** AC29, AC30, AC31, AC32, AC33, AC34
**Files:** `crates/parser/src/typescript.rs`, `crates/parser/src/test_utils.rs`
**Depends on:** T4, T5, T6

1. Write tests first:
   - Invalid/empty source → CodeGraphError::Parse, no panic (AC29)
   - Source with syntax errors → partial ParseResult, not error (AC30)
   - Parse from two std::thread::spawn threads concurrently → no panic (AC31)
2. Implement error handling in parse():
   - `parser.parse()` returns None → CodeGraphError::Parse
   - `tree.root_node().has_error()` → continue extraction, skip error nodes
   - Catch panics in individual node extraction → skip symbol
3. Create `test_utils.rs`:
   - `parse_ts(source: &str) -> ParseResult` — shortcut for tests
   - `parse_js(source: &str) -> ParseResult` — shortcut for tests
   - `find_symbol(result: &ParseResult, name: &str) -> &SymbolNode`
   - `find_import(result: &ParseResult, specifier: &str) -> &RawImport`
   - `find_export(result: &ParseResult, name: &str) -> &Export`
4. Write integration test with multi-construct TypeScript file:
   - File with functions, classes, interfaces, imports, exports combined
   - Verify complete ParseResult structure
5. `cargo clippy -p parser -- -Dwarnings` → clean
6. Verify AC34: parser depends only on domain, tree-sitter, tree-sitter-typescript, tree-sitter-javascript

---

## Task Dependency Graph

```
T1 ─┬─► T2 ─────────────────────┐
    │                            │
    └─► T3 ─┬─► T4 (symbols) ──►│
            │                    ├─► T7 (polish)
            ├─► T5 (imports) ───►│
            │                    │
            └─► T6 (exports) ───►┘
```

## Complexity Estimate

| Task | Estimate | Notes |
|------|----------|-------|
| T1 | S | Scaffold + types, straightforward |
| T2 | S | Registry is simple HashMap-based dispatch |
| T3 | S | Struct + thread-local + empty parse |
| T4 | L | Largest task — ~10 node types, metadata extraction, edge construction |
| T5 | M | 6 import forms, moderate tree walking |
| T6 | M | 7 export forms, similar complexity to T5 |
| T7 | M | Error handling, concurrency test, integration test |
