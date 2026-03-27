# Discussing M01-S04: Language Parsers & Import Resolution

## 1. Slice Intent

M01-S04 adds three language parsers (Rust, Python, Go) and the **import resolution pipeline** that converts `RawImport`/`Export` (extracted in S03) into resolved edges (`ImportsFrom`, `Calls`, `Extends`, `Implements`, `ReExport`, etc.). This is the slice that transforms raw parse output into a connected graph.

---

## 2. Challenging Assumptions

### A1: All four resolvers belong in one slice

The design spec bundles Rust/Python/Go parsers **and** import resolution for all five languages into one slice. This is the largest and most complex unit so far — S03's TypeScript parser alone was 1,706 lines and it only did extraction (no resolution). Adding:
- 3 new language parsers (Rust, Python, Go)
- 5 language-specific resolvers (TS/JS, Rust, Python, Go)
- `oxc_resolver` integration for TS/JS
- Barrel chain traversal
- Cross-file call resolution (Section 3.9)

**Challenge:** This is likely 3,000-5,000+ lines of new code across ~10 files. Should we split this into two slices: S04a (Rust/Python/Go parsers) and S04b (Import resolution)?

### A2: oxc_resolver is the right tool for TS/JS resolution

The spec calls for `oxc_resolver` to handle tsconfig paths, node_modules resolution, etc. This is a large dependency. Need to verify:
- Does `oxc_resolver` compile cleanly in our tree-sitter ecosystem?
- Is the API stable? (oxc is still evolving rapidly)
- Do we actually need full Node.js resolution, or can we start simpler?

### A3: Cross-file call resolution belongs in S04

Section 3.9's four-step call resolution strategy (scoped -> qualified -> single-candidate -> ambiguous) requires a **fully populated import graph** plus a scan of all call sites. This is a different concern from import resolution — it's more of a post-processing pass. Should it be deferred to S05 (CLI/Index) where the full pipeline orchestration happens?

### A4: Barrel chain traversal is bounded

Real-world TS/JS projects can have deeply nested barrel re-exports (`index.ts` -> `index.ts` -> `index.ts`). The spec says "custom barrel chain traversal" but doesn't specify a depth limit or cycle detection strategy. Unbounded barrel traversal could be expensive.

### A5: Each parser is independent

The Rust, Python, and Go parsers follow the same `LanguageParser` trait, but their extraction patterns differ significantly:
- **Rust:** `mod` tree, `pub use` re-exports, `impl` blocks, trait impls, macros
- **Python:** `__init__.py` packages, `from . import`, `TYPE_CHECKING` blocks, decorators
- **Go:** `go.mod` modules, receiver methods, embedding, dot imports, blank imports

Each needs language-specific tree-sitter knowledge. How much shared infrastructure is there vs. custom per-language code?

---

## 3. Surfacing Unknowns

| Unknown | Risk | Mitigation |
|---------|------|------------|
| `oxc_resolver` API stability and Rust compatibility | Medium | Research phase: verify compilation, test basic resolution |
| tree-sitter grammar versions for Rust/Python/Go ABI compat | Medium | Same bridge pattern as S03 (`tree-sitter-language ^0.1`), but must verify each grammar crate |
| Barrel chain cycle detection | Low-Medium | Cap at 10 hops, detect cycles via visited set |
| Rust `mod` tree resolution correctness | Medium | Cargo workspace awareness, `mod.rs` vs `filename.rs` conventions |
| Python relative imports (`from . import`, `from .. import`) | Medium | Requires path algebra relative to `__init__.py` boundaries |
| Go module path resolution (`go.mod` -> source mapping) | Medium | Parse `go.mod` for module path, map import paths to local files |
| Cross-file call resolution performance | Low | Deferred concern — correctness first, optimize in later slices |
| `rayon` as a parser dep for parallel resolution | Low | Spec lists it but S03 explicitly excluded it — clarify ownership |

---

## 4. Scope Recommendation

Given the size and complexity, I recommend **splitting S04 into two focused slices:**

### Option A: Two-slice split (recommended)

**S04: Language Parsers (Rust, Python, Go)**
- `rust_lang.rs`, `python.rs`, `go.rs` implementing `LanguageParser`
- Thread-local tree-sitter for each language
- Symbol extraction, structural edges, `RawImport`/`Export` extraction
- Update `ParserRegistry` to register all parsers
- No resolution logic

**S04b (new slice): Import Resolution Pipeline**
- `resolver/mod.rs` — `ImportResolver` trait + `resolve_all()` pipeline
- `resolver/typescript.rs` — `oxc_resolver` + barrel traversal
- `resolver/rust_lang.rs` — crate-root module walk
- `resolver/python.rs` — package resolution
- `resolver/go.rs` — go.mod resolution
- Cross-file call resolution (Section 3.9)

### Option B: Single slice (as spec'd)

Keep as one slice but acknowledge it's the largest in the milestone and will need a thorough research phase.

---

## 5. Complexity Classification

| Aspect | Rating | Justification |
|--------|--------|---------------|
| **Algorithmic** | High | Import resolution with barrel chains, cross-file calls, multi-language module systems |
| **Integration** | Medium | Extends existing parser crate, no new crate boundaries |
| **Domain knowledge** | High | Requires understanding of 4 different module systems (TS/JS, Rust, Python, Go) |
| **Dependencies** | Medium | `oxc_resolver`, 3 new tree-sitter grammars |
| **Testing** | High | Need real-world fixtures per language, resolution correctness across edge cases |

**Overall: High complexity** — this is the hardest slice in M01.

---

## 6. Decisions

### Q1: Split or keep together?
**Decision: One large slice.** Keep S04 as a single slice covering all parsers + resolution.

### Q2: Cross-file call resolution timing
**Decision: Defer to S05.** Cross-file call resolution (Section 3.9) moves to S05 where the full index pipeline and orchestration exists.

### Q3: oxc_resolver commitment
**Decision: Research alternatives first.** If no viable simpler alternative exists, commit to `oxc_resolver`. Research phase will evaluate.

### Q4: rayon ownership
**Decision: Research during S04.** Will evaluate where parallel iteration best fits (parser vs. cli) and recommend during research phase.
