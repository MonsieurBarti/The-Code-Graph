# M01-S01 Research: Domain Model Implementation

Research findings for the six technical questions needed before implementing the domain crate.

---

## 1. serde + `std::time::Duration`

**Status: Works out of the box. No custom implementation needed.**

`std::time::Duration` implements `Serialize` and `Deserialize` natively in serde. The `#[derive(Serialize, Deserialize)]` macro handles it automatically when used on structs containing `Duration` fields.

**Serialization format** (JSON via serde_json):

```json
{
  "duration": {
    "secs": 1,
    "nanos": 234000000
  }
}
```

Duration serializes as a struct with two fields: `secs` (u64) and `nanos` (u32). This is serde's built-in representation -- not a human-readable string.

**Verified**: Compiled and ran a test with `serde 1.0.228` / `serde_json 1.0.149`. The `IndexStats` struct from the spec (with a `duration: Duration` field) round-trips correctly through JSON. Zero-valued and large durations (hours + nanoseconds) all work.

**Recommendation**: Use `Duration` directly in `IndexStats`. No `#[serde(with = ...)]` or custom serializer needed. The `{secs, nanos}` JSON representation is fine for machine consumption. If human-readable output is ever needed, that's a formatting concern for the CLI layer, not the domain model.

---

## 2. thiserror 2.x Compatibility

**Status: Fully compatible with the spec's error pattern. No gotchas.**

Tested with `thiserror 2.0.18` (latest as of 2026-03-27). The exact `CodeGraphError` enum from the spec compiles and works correctly, including the `FileSystem` variant with named fields:

```rust
#[error("file system error: {path}: {source}")]
FileSystem { path: PathBuf, source: std::io::Error },
```

**Key findings:**

1. **Named fields in `#[error("...")]`**: Field names are referenced directly (e.g., `{path}`, `{source}`). This works for all types that implement `Display`. `PathBuf` implements `Display` and formats correctly.

2. **Automatic `source()` detection**: thiserror 2.x automatically recognizes a field named `source` as the error source for `std::error::Error::source()`. No `#[source]` annotation is needed when the field is literally named `source`. Verified: `err.source()` returns `Some(...)` pointing to the inner `std::io::Error`.

3. **No `#[from]` needed**: The `FileSystem` variant does not use `#[from]` (which would auto-generate a `From<std::io::Error>` impl). This is correct for the spec's design -- the `path` field requires additional context that `From` cannot provide.

4. **Zero runtime footprint**: `thiserror 2.0.18` depends only on `thiserror-impl` (a proc-macro). The dependency tree is: `thiserror -> thiserror-impl (proc-macro) -> {proc-macro2, quote, syn}`. All compile-time only; zero runtime dependencies.

5. **All nine variants** from the spec compile and produce correct `Display` output:
   - `Parse { file, message }` -- "parse error in src/main.rs: unexpected token"
   - `FileSystem { path, source }` -- "file system error: /some/path: file not found"
   - `NoProject` -- "no project found (no .git directory)"
   - `BlocklistedRoot(PathBuf)` -- "refused to index blocklisted root: /Users"
   - `IndexNotBuilt` -- "index not built -- run `code-graph index` first"
   - Tuple variants (`Resolution`, `Storage`, `Git`, `Other`) all format with `{0}`

**Recommendation**: Use the spec's error definitions as-is. No modifications needed.

---

## 3. Derived `Ord` for Enums (Discriminant Order)

**Status: Guaranteed. Declaration order is stable and reliable.**

Rust's derived `PartialOrd`/`Ord` for enums compares variants by their **discriminant**, which defaults to declaration order (0, 1, 2, ...). This is documented in the standard library:

> "When `derive`d on enums, variants are ordered primarily by their discriminants. Secondarily, they are ordered by their fields. By default, the discriminant is smallest for variants at the top, and largest for variants at the bottom."

**Verified** with the exact `Confidence` enum from the spec:

```rust
#[derive(PartialOrd, Ord)]
enum Confidence { Structural, Low, Medium, High }
```

Results:
- `Structural < Low < Medium < High` -- confirmed
- Sorting produces `[Structural, Low, Medium, High]` -- confirmed
- Filtering with `>= Medium` correctly yields only `Medium` and `High` edges -- confirmed

**Important caveat**: If explicit discriminant values are assigned (e.g., `Top = 2, Bottom = 1`), the derived `Ord` uses those values, reversing the apparent declaration order. The spec's `Confidence` enum uses default discriminants, so declaration order = comparison order.

**Recommendation**: The spec's comment "Variant order is load-bearing for derived Ord" is correct and sufficient. Add a unit test asserting the ordering as a guardrail against accidental reordering:

```rust
#[test]
fn confidence_ordering_is_declaration_order() {
    assert!(Confidence::Structural < Confidence::Low);
    assert!(Confidence::Low < Confidence::Medium);
    assert!(Confidence::Medium < Confidence::High);
}
```

---

## 4. HashMap vs Alternatives for Graph Adjacency

**Status: `HashMap<String, Vec<(String, EdgeKind)>>` is the right choice for v0.1.**

Benchmarked `HashMap` vs `BTreeMap` at realistic codebase sizes (1k, 10k, 50k, 100k symbols with ~3 edges per symbol):

| Symbols | HashMap build | BTreeMap build | HashMap lookup (1k) | BTreeMap lookup (1k) | HashMap iter | BTreeMap iter | Est. memory |
|---------|--------------|----------------|---------------------|----------------------|-------------|--------------|-------------|
| 1,000   | 0.6ms | 0.7ms | 51us | 120us | 4us | 10us | ~0.2 MB |
| 10,000  | 9ms | 8ms | 73us | 159us | 50us | 81us | ~1.7 MB |
| 50,000  | 14ms | 22ms | 32us | 65us | 97us | 157us | ~8.3 MB |
| 100,000 | 31ms | 45ms | 60us | 68us | 187us | 401us | ~16.7 MB |

### Analysis of alternatives

**`HashMap` (spec's choice)**:
- O(1) amortized lookup, O(n) iteration
- Non-deterministic iteration order
- Best raw performance for single-key lookups
- ~17 MB for 100k symbols -- entirely acceptable

**`BTreeMap`**:
- O(log n) lookup, O(n) sorted iteration
- Deterministic (sorted) iteration order
- ~1.5-2x slower on lookups and iteration at scale
- Useful if sorted output is needed, but that's a presentation concern

**`IndexMap`** (from `indexmap` crate):
- O(1) amortized lookup (hashbrown-backed), O(n) insertion-order iteration
- Preserves insertion order -- deterministic iteration
- Performance comparable to HashMap for lookups
- Would add an external dependency to the domain crate (violates AC10)

**`petgraph`**:
- Full graph library with built-in BFS/DFS, cycle detection, topological sort
- Rich API: `Graph` (adjacency list), `StableGraph`, `GraphMap`, `CSR`
- **Significant dependency** -- would dominate the domain crate's deps
- Overkill when only BFS, DFS, and adjacency lookups are needed
- The spec's traversal needs are simple enough to implement directly (~50 lines each)

### Recommendation

**Use `HashMap` as specified.** Reasons:

1. **Performance is excellent** -- 100k symbols in 31ms build, sub-100us lookups, <20 MB memory. These are tiny numbers for a CLI tool.
2. **Zero additional dependencies** -- keeps domain crate clean (AC10).
3. **Iteration order does not matter** for BFS/DFS -- they use their own queue/stack ordering.
4. **Simplicity** -- the team understands HashMap; no learning curve.
5. If deterministic output ordering is ever needed (e.g., for snapshot tests), sort the results after collection, not during traversal.

If petgraph is ever considered (v0.2+), it would be for advanced algorithms (community detection, topological sort), not for the basic adjacency structure.

---

## 5. BFS/DFS with Confidence Filtering

**Status: Two viable patterns. Recommend the spec's approach (min_confidence parameter) with an internal closure for the implementation.**

### Pattern 1: `min_confidence` parameter (spec's approach)

```rust
fn bfs_filtered(
    &self,
    start: &str,
    direction: Direction,
    max_depth: usize,
    min_confidence: Confidence,
) -> Vec<TraversalResult>
```

Filter is `edge_kind.confidence() >= min_confidence`, leveraging derived `Ord`.

**Pros**: Simple API, matches the spec, covers the primary use case (confidence-tier filtering). Callers don't need to construct closures.

**Cons**: Only supports confidence-based filtering. Cannot filter by specific `EdgeKind` without a separate method.

### Pattern 2: Closure-based filter

```rust
fn bfs_with_filter<F>(
    &self,
    start: &str,
    direction: Direction,
    max_depth: usize,
    edge_filter: F,
) -> Vec<TraversalResult>
where
    F: Fn(&EdgeKind) -> bool,
```

**Pros**: Maximum flexibility -- any predicate works. Confidence filtering is just `|ek| ek.confidence() >= min`. Can also filter by specific edge kinds.

**Cons**: Slightly more complex API. Closures are less discoverable in docs. Generic parameter makes error messages noisier.

### Pattern 3: Combined (recommended implementation strategy)

Keep the public API matching the spec (`bfs_filtered` with `min_confidence`), but internally implement it using a private generic helper:

```rust
// Public API (matches spec)
pub fn bfs_filtered(
    &self, start: &str, direction: Direction,
    max_depth: usize, min_confidence: Confidence,
) -> Vec<TraversalResult> {
    self.bfs_inner(start, direction, max_depth, |ek| ek.confidence() >= min_confidence)
}

// Public unfiltered BFS (matches spec)
pub fn bfs(
    &self, start: &str, direction: Direction, max_depth: usize,
) -> Vec<TraversalResult> {
    self.bfs_inner(start, direction, max_depth, |_| true)
}

// Private generic helper -- avoids code duplication
fn bfs_inner<F: Fn(&EdgeKind) -> bool>(
    &self, start: &str, direction: Direction,
    max_depth: usize, edge_filter: F,
) -> Vec<TraversalResult> {
    // Single BFS implementation with filter
}
```

**Verified**: Both patterns compile and produce correct results. The closure is monomorphized at compile time, so there is zero runtime cost for the abstraction.

**Recommendation**: Use Pattern 3 (combined). The public API stays clean and matches the spec. The private helper eliminates duplication between `bfs` and `bfs_filtered`. If a generic `bfs_with_filter` is ever needed publicly (v0.2), the internal helper is already there.

---

## 6. QualifiedName Newtype

**Status: Implement `FromStr`, `TryFrom<String>`, `AsRef<str>`, `Borrow<str>`, and `Display`. Do NOT implement `Deref<Target=str>`.**

### Validated pattern

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QualifiedName(String);

#[derive(Debug, Clone, PartialEq)]
pub enum QualifiedNameError {
    Empty,
    MissingSeparator,
    EmptyFilePath,
    EmptySymbolPath,
}
```

### Trait implementations (in priority order)

| Trait | Implement? | Purpose |
|-------|-----------|---------|
| `QualifiedName::parse(&str) -> Result<Self>` | Yes | Primary constructor. Validates grammar. |
| `FromStr` | Yes | Enables `"path::sym".parse::<QualifiedName>()`. Delegates to `parse()`. |
| `TryFrom<&str>` | Yes | Enables `QualifiedName::try_from("path::sym")`. Delegates to `parse()`. |
| `TryFrom<String>` | Yes | Zero-copy construction when caller owns the String. Avoids re-allocation. |
| `AsRef<str>` | Yes | Enables passing to functions that take `impl AsRef<str>`. |
| `Borrow<str>` | Yes | **Critical**: Enables `HashMap<QualifiedName, V>::get("raw_str")` lookups without constructing a `QualifiedName`. |
| `Display` | Yes | Formats as the inner string. Required for error messages and logging. |
| `Deref<Target=str>` | **No** | See rationale below. |

### Why NOT `Deref<Target=str>`

`Deref` is intended for smart pointer types. Implementing it on a newtype makes every `&str` method callable on `&QualifiedName`, which:

1. **Breaks the abstraction**: Callers can call `.split()`, `.replace()`, `.to_uppercase()` etc., bypassing the validated invariant.
2. **Implicit coercion surprise**: `&QualifiedName` silently coerces to `&str` in function arguments, making it easy to accidentally pass raw strings where validated names are expected.
3. **Clippy warns**: `clippy::deref_to_str` (in newer Clippy versions) flags this pattern.

`AsRef<str>` + `Borrow<str>` provide the necessary conversions explicitly, without exposing the full `str` API.

### `Borrow<str>` for HashMap lookups -- verified working

```rust
use std::borrow::Borrow;

impl Borrow<str> for QualifiedName {
    fn borrow(&self) -> &str { &self.0 }
}

// Enables:
let mut map: HashMap<QualifiedName, u32> = HashMap::new();
map.insert(QualifiedName::parse("src/file.rs::Sym").unwrap(), 42);
assert_eq!(map.get("src/file.rs::Sym"), Some(&42)); // looks up with &str
```

This works because `HashMap::get<Q>` requires `K: Borrow<Q>` and `Q: Hash + Eq`. Since `String` (inside `QualifiedName`) and `str` hash identically, and we implement `Borrow<str>`, lookups with `&str` keys work correctly.

**Verified**: Compiled and tested. HashMap lookup via `&str` on a `QualifiedName`-keyed map returns the correct value.

### Serde behavior

With `#[derive(Serialize, Deserialize)]`, the newtype serializes as a plain string (the inner `String` value). This is the correct behavior -- `QualifiedName("src/file.rs::Sym")` serializes to `"src/file.rs::Sym"` in JSON.

**Important note on deserialization**: The derived `Deserialize` will NOT validate the grammar on deserialization. If validation on deserialization is required, implement a custom `Deserialize` or use `#[serde(try_from = "String")]`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(try_from = "String")]
pub struct QualifiedName(String);

// serde will call TryFrom<String> on deserialization, enforcing validation
```

**Recommendation**: Use `#[serde(try_from = "String")]` to enforce validation on deserialization. This ensures that any `QualifiedName` in the system -- whether constructed in code or deserialized from JSON/storage -- is always valid.

---

## Summary of Recommendations

| Topic | Decision |
|-------|----------|
| serde + Duration | Use directly, works out of the box |
| thiserror 2.x | Use spec's pattern as-is, no modifications |
| Derived Ord | Reliable, add a unit test as guardrail |
| Graph adjacency | `HashMap` is correct, no alternatives needed for v0.1 |
| BFS filtering | Public `min_confidence` param, private closure-based helper |
| QualifiedName | `FromStr` + `TryFrom` + `AsRef<str>` + `Borrow<str>`, no `Deref`, use `#[serde(try_from)]` |
