# Research — M01-S03: Tree-Sitter Parser Infrastructure

## R1: Dependency Version Matrix

### Architecture Change: tree-sitter-language bridge

Starting at tree-sitter 0.23, a bridge crate `tree-sitter-language` was introduced. Grammar crates depend only on `tree-sitter-language ^0.1` at runtime (a tiny crate exporting `LanguageFn`). The core `tree-sitter` crate also depends on `tree-sitter-language ^0.1`. This decouples grammars from core — **all grammar crates targeting `tree-sitter-language ^0.1` are ABI-compatible with tree-sitter 0.23-0.26**.

### Latest Versions (March 2026)

| Crate | Latest | Notes |
|---|---|---|
| `tree-sitter` | 0.26.7 | |
| `tree-sitter-javascript` | 0.25.0 | |
| `tree-sitter-typescript` | 0.23.2 | Nov 2024, no newer release |
| `tree-sitter-language` | 0.1.7 | Transitive, no direct dep needed |

### Decision: Use latest stable versions

```toml
[dependencies]
domain = { path = "../domain" }
tree-sitter = "0.24"
tree-sitter-typescript = "0.23"
tree-sitter-javascript = "0.23"
```

Rationale: Pin `tree-sitter` to `"0.24"` (not `"0.26"`) for conservative stability — the API we need hasn't changed since 0.24, and this avoids pulling in newer features we don't use. Cargo's semver resolution will pick the latest compatible 0.24.x. Grammar crates at `"0.23"` are the latest for TypeScript and a well-tested baseline for JavaScript (0.25.0 would also work but no benefit for our use case).

No need to add `tree-sitter-language` to Cargo.toml — it's pulled in transitively.

---

## R2: tree-sitter Parser API

### Parser Type

```rust
// Creation
let mut parser = Parser::new();

// Language setting — takes &Language, returns Result
parser.set_language(&language)?;  // Result<(), LanguageError>

// Parsing — takes impl AsRef<[u8]>, returns Option<Tree>
let tree: Option<Tree> = parser.parse(source, None);

// Parser IS Send + Sync (explicit unsafe impls in crate)
// But requires &mut self for parse(), so concurrent access needs coordination
```

**Correction to SPEC assumption**: `Parser` IS `Send + Sync` (via explicit `unsafe impl`). Thread-local is not strictly required for safety, but it IS the right pattern for performance — avoids `Arc<Mutex<Parser>>` contention and reuses allocated buffers.

### Language Type

Grammar crates export `LanguageFn` constants (not functions):

```rust
// Grammar crate exports
tree_sitter_typescript::LANGUAGE_TYPESCRIPT  // type: LanguageFn
tree_sitter_typescript::LANGUAGE_TSX         // type: LanguageFn
tree_sitter_javascript::LANGUAGE             // type: LanguageFn

// Convert to Language
let lang: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
// OR
let lang = Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT);
```

`LanguageFn` is `Copy + Clone + Send + Sync`.
`Language` is `Clone + Send + Sync` (not Copy — has Drop).

### Node API

```rust
node.kind() -> &'static str           // CST node type string
node.utf8_text(source) -> Result<&str, Utf8Error>
node.start_position() -> Point        // Point { row: usize, column: usize } (0-based)
node.end_position() -> Point
node.has_error() -> bool
node.is_named() -> bool
node.child_by_field_name("name") -> Option<Node>
node.child(i: u32) -> Option<Node>
node.child_count() -> usize
node.parent() -> Option<Node>

// IMPORTANT: children() requires a TreeCursor
node.children(&mut cursor) -> impl Iterator<Item = Node>
node.named_children(&mut cursor) -> impl Iterator<Item = Node>
// Create cursor via: node.walk() or tree.walk()
```

### Tree Type

```rust
tree.root_node() -> Node<'_>    // lifetime tied to Tree
// Tree is Clone + Send + Sync
// Tree owns all node data — nodes borrow from Tree
```

### Thread-Local Pattern (Validated)

```rust
use std::cell::RefCell;
use tree_sitter::Parser;

thread_local! {
    static TS_PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

fn parse_with_language(source: &[u8], lang: &Language) -> Option<tree_sitter::Tree> {
    TS_PARSER.with(|p| {
        let mut parser = p.borrow_mut();
        parser.set_language(lang).ok()?;
        parser.parse(source, None)
    })
}
```

**Key points:**
- `Tree` returned by `parse()` is owned, no lifetime dependency on `Parser` — safely escapes the `with()` closure
- `Node<'tree>` borrows from `Tree`, not from `Parser` — borrow checker enforces this
- Language switching per file via `set_language()` is cheap (just sets a pointer)
- `RefCell::borrow_mut()` will panic on re-entrant parsing on same thread — not a risk in our design (no recursion through parser)

---

## R3: TypeScript/JavaScript CST Node Types

### Declaration Node Kinds

| Source | `node.kind()` | Name Field |
|---|---|---|
| `function foo() {}` | `"function_declaration"` | `child_by_field_name("name")` → identifier |
| `class Foo {}` | `"class_declaration"` | `child_by_field_name("name")` → type_identifier |
| `abstract class Foo {}` | `"abstract_class_declaration"` | same |
| `interface Foo {}` | `"interface_declaration"` | `child_by_field_name("name")` → type_identifier |
| `type Foo = ...` | `"type_alias_declaration"` | `child_by_field_name("name")` → type_identifier |
| `enum Foo {}` | `"enum_declaration"` | `child_by_field_name("name")` → identifier |
| `const foo = 1` | `"lexical_declaration"` | children: `variable_declarator`(s) |
| `let foo = 1` | `"lexical_declaration"` | same |
| `var foo = 1` | `"variable_declaration"` | children: `variable_declarator`(s) |

### Variable Declarator

`"variable_declarator"` — inside lexical/variable_declaration:
- `child_by_field_name("name")` → identifier
- `child_by_field_name("value")` → expression (arrow_function, function_expression, etc.)
- `child_by_field_name("type")` → type_annotation (TS only)

**Arrow function detection**: When `value` is `"arrow_function"` or `"function_expression"`, treat the variable as a Function symbol.

### Class Members

| Source | `node.kind()` | Name Field |
|---|---|---|
| `method() {}` | `"method_definition"` | `child_by_field_name("name")` → property_identifier |
| `prop: string` (TS class field) | `"public_field_definition"` | `child_by_field_name("name")` → property_identifier |
| `prop = 1` (JS class field) | `"field_definition"` | `child_by_field_name("property")` → property_identifier |

### Interface Members (TS only)

| Source | `node.kind()` |
|---|---|
| `prop: string` | `"property_signature"` |
| `method(): void` | `"method_signature"` |

### Import Statements

All imports are `"import_statement"`. Structure:

```
import_statement
  ├── "import" (unnamed token)
  ├── ["type"] (unnamed token, if type-only import)
  ├── import_clause (if not side-effect)
  │   ├── identifier (default import)
  │   ├── named_imports
  │   │   └── import_specifier (name, alias?)
  │   └── namespace_import
  │       └── identifier
  └── source: string (the specifier, including quotes)
```

**Detecting `import type`**: Iterate unnamed children of `import_statement`, check for `child.kind() == "type"`.

**Side-effect import** (`import "./polyfill"`): No `import_clause` child — only `source`.

**Default import** (`import foo from "./mod"`): `import_clause` has a direct `identifier` child (the default binding name).

**Source specifier**: `child_by_field_name("source")` → string node. Use `utf8_text(source)` and strip quotes.

### Export Statements

All exports are `"export_statement"`. Structure:

```
export_statement
  ├── "export" (unnamed token)
  ├── ["default"] (unnamed token, if default export)
  ├── ["type"] (unnamed token, if type-only export)
  ├── declaration (function_declaration, class_declaration, etc.)
  ├── export_clause
  │   └── export_specifier (name, alias?)
  ├── namespace_export (for `export * as ns from "..."`)
  │   └── identifier
  └── source: string (if re-export)
```

**Detecting `export default`**: Iterate unnamed children for `kind() == "default"`.
**Detecting `export type`**: Iterate unnamed children for `kind() == "type"`.
**Re-export detection**: `child_by_field_name("source")` is `Some`.
**Star export** (`export * from "..."`): Unnamed `"*"` token child + `source`.

### Async Detection

`"async"` is an unnamed child token on function_declaration, arrow_function, method_definition, function_expression:

```rust
fn is_async(node: &Node, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| !child.is_named() && child.kind() == "async")
}
```

Same pattern for `static`, `get`, `set`, `readonly`, `abstract`, `declare`, `default`, `type`.

### Decorator Detection

Decorators are **named children** of class/method/field declarations:

```
class_declaration
  ├── decorator          ← named node, kind = "decorator"
  │   └── call_expression OR identifier OR member_expression
  ├── name: type_identifier
  └── body: class_body
```

Extract decorator text: `decorator_node.utf8_text(source)` (includes `@` prefix).

### Identifier Node Kinds

| Context | Node Kind |
|---|---|
| Variable / function name | `"identifier"` |
| Type name (class, interface, type alias) | `"type_identifier"` |
| Property / method name | `"property_identifier"` |
| Private field (`#foo`) | `"private_property_identifier"` |

### JS vs TS Differences

| Construct | JavaScript | TypeScript |
|---|---|---|
| Class field | `"field_definition"` (field: `property`) | `"public_field_definition"` (field: `name`) |
| Interface | N/A | `"interface_declaration"` |
| Type alias | N/A | `"type_alias_declaration"` |
| Enum | N/A | `"enum_declaration"` |
| Abstract class | N/A | `"abstract_class_declaration"` |
| Type annotation | N/A | `"type_annotation"` |
| Parameters | identifier / pattern directly | `"required_parameter"` / `"optional_parameter"` |
| `import type` | N/A | unnamed `"type"` token |

**Implication for shared extraction**: The TypeScript grammar is a superset of JavaScript. Most extraction logic can be shared — TS-only node types simply won't appear in JS parse trees. However, for `.js` files we use `tree-sitter-javascript` (not TSX grammar), so class field names use `"field_definition"` with field `property` instead of `"public_field_definition"` with field `name`. The extractor must handle both.

---

## R4: Design Refinements from Research

### 4.1 TypeScriptParser stores LanguageFn, not Language

Since `LanguageFn` is `Copy + Send + Sync` and lightweight (function pointer), store it directly:

```rust
pub struct TypeScriptParser {
    ts_language: LanguageFn,    // LANGUAGE_TYPESCRIPT
    tsx_language: LanguageFn,   // LANGUAGE_TSX
    js_language: LanguageFn,    // LANGUAGE (JavaScript)
}
```

Convert to `Language` at parse time: `let lang: Language = self.ts_language.into();`

### 4.2 Single thread-local Parser, language switched per file

One `thread_local! { static PARSER: RefCell<Parser> }` shared across all language parsers. Language is set via `parser.set_language()` before each parse call. This is the simplest design and `set_language()` is cheap (pointer assignment).

### 4.3 TreeCursor requirement for children iteration

Every extraction function needs a `TreeCursor`. Create it from the node: `let mut cursor = node.walk();`. The cursor is stack-allocated and cheap. Pattern:

```rust
fn extract_children(node: &Node, source: &[u8]) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "function_declaration" => { /* ... */ }
            "class_declaration" | "abstract_class_declaration" => { /* ... */ }
            // ...
            _ => {}
        }
    }
}
```

### 4.4 Signature extraction strategy

For functions/methods, extract the parameter list text + return type annotation:

```rust
fn extract_signature(node: &Node, source: &[u8]) -> Option<String> {
    let params = node.child_by_field_name("parameters")?;
    let params_text = params.utf8_text(source).ok()?;
    let return_type = node.child_by_field_name("return_type")
        .and_then(|n| n.utf8_text(source).ok());
    match return_type {
        Some(rt) => Some(format!("{params_text}{rt}")),
        None => Some(params_text.to_string()),
    }
}
```

This produces signatures like `(x: number, y: string): boolean`.

### 4.5 Source specifier quote stripping

The `source` field on import/export statements is a string node including quotes. Strip with:

```rust
fn strip_quotes(text: &str) -> &str {
    text.trim_matches(|c| c == '"' || c == '\'' || c == '`')
}
```

---

## R5: SPEC Corrections

Based on research findings, the following SPEC items need correction:

1. **SPEC says `parser.set_language(&lang)?`** — Confirmed correct. Signature is `set_language(&mut self, &Language) -> Result<(), LanguageError>`.

2. **SPEC says `Parser is NOT Send`** — **Incorrect.** Parser IS Send + Sync (explicit unsafe impls). Thread-local is still the right pattern for performance, not safety.

3. **SPEC says `tree_sitter::Language` is a pointer to C static data** — **Partially correct.** Grammar crates export `LanguageFn` (a function pointer), which is converted to `Language` via `.into()`. `Language` wraps a raw pointer to the grammar tables.

4. **SPEC dependency versions** — Updated: `tree-sitter = "0.24"`, `tree-sitter-typescript = "0.23"`, `tree-sitter-javascript = "0.23"`. All compatible via `tree-sitter-language ^0.1` bridge.

5. **SPEC says class fields are `"public_field_definition"`** — Only for TypeScript grammar. JavaScript grammar uses `"field_definition"` with a different field name (`property` vs `name`). Both must be handled.

---

## Summary of Decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| tree-sitter version | 0.24 | Conservative stability, API unchanged since 0.24 |
| Grammar versions | TS 0.23, JS 0.23 | Latest for TS; compatible baseline for JS |
| Parser thread-local | Yes, single shared Parser | Performance (buffer reuse), language switched per file |
| Store LanguageFn vs Language | LanguageFn in struct | Copy + Send + Sync, convert at parse time |
| Children iteration | TreeCursor required | API constraint — create per extraction function |
| JS class field handling | Support both `field_definition` and `public_field_definition` | Different node kinds in JS vs TS grammars |
| Signature format | `(params): return_type` | Direct text extraction from CST nodes |
| Source specifier | Strip quotes from string node | Consistent with other tree-sitter tools |
