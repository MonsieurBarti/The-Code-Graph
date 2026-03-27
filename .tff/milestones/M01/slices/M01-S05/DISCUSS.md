# Discussing M01-S05: CLI Foundation & Index Command

## 1. Slice Intent

M01-S05 creates the two remaining workspace crates (`cli`, `binary`), implements the full indexing pipeline (`IndexUseCase.full_index()`), wires adapters to domain ports, and delivers the first runnable `code-graph index` command. It also absorbs two items deferred from S04: cross-file call resolution (Section 3.9) and rayon-based parallel parsing. After this slice, a user can run `code-graph index` on a real codebase and get a populated SQLite graph.

---

## 2. Challenging Assumptions

### A1: Both `cli` and `binary` crates belong in this slice

The spec defines six workspace crates: domain, parser, storage, watch, **cli**, **binary**. S05 creates both. The `binary` crate is trivial (just `main.rs` calling into `cli`), but the `cli` crate is the orchestration hub — it wires adapters, owns the clap definitions, output formatting, and all command handlers. Creating both is natural since `binary` is useless without `cli`.

**Verdict:** Correct grouping. No split needed.

### A2: FileSystem and GitProvider adapters belong in `cli`, not their own crate

The design spec says `cli` is "the orchestration layer: directly depends on parser, storage, and watch to wire adapters to domain ports." The `FileSystem` and `GitProvider` adapters are adapters (hexagonal sense) — they implement domain port traits with real I/O. The spec places them in `cli` because it's the only crate that sees both domain and external dependencies (like `git2` or `std::fs`).

**Challenge:** Should these adapters live in a dedicated `adapters` crate instead? Counter-argument: the spec is explicit — `cli` owns adapter wiring. Adding a seventh crate contradicts R1 (six-crate workspace). Keep them in `cli`.

### A3: Cross-file call resolution belongs in S05

S04's DISCUSS.md (Q2 decision) explicitly deferred cross-file call resolution to S05: "Cross-file call resolution (Section 3.9) moves to S05 where the full index pipeline and orchestration exists." The rationale is sound — call resolution needs the full graph (all imports resolved) before it can scope-match calls.

**Challenge:** This adds significant algorithmic complexity to what is otherwise a "wiring" slice. The four-step strategy (scoped -> qualified -> single-candidate -> ambiguous) requires:
- A completed import graph
- Scanning all call-site AST nodes (already captured as `Calls` structural edges within files)
- Matching each call against imported scope

This is a post-processing pass after import resolution. Should it be pushed to S06 (Query Commands) instead?

**Counter-argument:** Cross-file call resolution produces *edges*, not query results. It belongs in the indexing pipeline, not the query layer. S05 is the right home.

### A4: Rayon parallelism is straightforward here

S04 deferred rayon to S05 with the note: "Parallelism is the caller's concern (CLI/IndexUseCase in S05)." The parser crate already has thread-local tree-sitter (`thread_local!` + `RefCell<Parser>`), so rayon parallel iteration over files should "just work."

**Challenge:** Resolution is inherently sequential — you need all `ParseResult`s before resolving imports, and all import edges before resolving cross-file calls. So parallelism applies to:
1. File parsing (embarrassingly parallel)
2. Import resolution per file (parallel, reads from shared `ResolveContext`)
3. Cross-file call resolution (needs full import graph, then parallel per file)

This is three sequential phases with parallel work within each. Not a single `par_iter()`.

### A5: Output formatting for `index` is minimal

The `index` command outputs `IndexStats` (files indexed, symbols extracted, edges created, duration). This doesn't need compact/table/JSON formatting — it's a single summary line. The full output format infrastructure (Section 7.2) is more relevant for query commands (S06).

**Challenge:** Should we build the full `OutputFormatter` trait/enum now, or just print the stats directly? Over-engineering the formatter for a single command is waste, but S06 will need it immediately after.

### A6: The `index` command needs project root detection and `.code-graph/` setup

Every command starts with: detect project root (walk up to `.git`), check blocklist, ensure `.code-graph/` exists, open/create `graph.db`. This is shared infrastructure that S06+ will also need.

**Verdict:** This is clearly S05 scope — it's the foundation that every subsequent command relies on.

### A7: `tracing` and logging setup belongs in S05

The spec defines verbosity flags (`--verbose`, `--debug`, `CODE_GRAPH_LOG`). S05 creates the binary entry point, so it owns tracing subscriber initialization.

**Verdict:** Correct. Basic tracing setup with level filtering belongs here.

---

## 3. Surfacing Unknowns

| Unknown | Risk | Mitigation |
|---------|------|------------|
| `git2` vs shell-out for GitProvider | Low | Research: `git2` (libgit2) is a heavy dep but avoids shelling out. For v0.1 we only need `git ls-files`, `git status --porcelain`, and `git diff`. Shell-out may be simpler. |
| Rayon dep ownership (parser vs cli) | Low | Spec says `rayon` is a parser dep (Section 2.2), but S04 excluded it. Needs a decision: add to parser or cli. |
| Cross-file call resolution complexity | Medium | Four-step strategy is well-defined in spec but untested. Edge cases: overloaded methods, re-exported aliases, namespace imports. Start with scoped resolution only, extend in later pass. |
| `IndexUseCase` signature mismatch | Low | Current scaffold takes `(store, fs, git)` but also needs `ParserRegistry` and `ResolverRegistry`. May need to add parser/resolver as constructor params or introduce a pipeline struct. |
| Exit code mapping | Low | Spec defines 4 codes (0/1/2/3). Map `CodeGraphError` variants to codes in `binary/main.rs`. |
| `.code-graphignore` file parsing | Low | Spec mentions gitignore-syntax ignore file. `ignore` crate handles this. But is it S05 scope or later? |
| How `full_index` calls the parser+resolver pipeline | Medium | `IndexUseCase` is in the domain crate, which can't depend on parser. Either: (a) inject a trait that wraps parse+resolve, (b) move orchestration to cli, or (c) use a closure/callback. This is an architectural question. |

---

## 4. Scope Recommendation

### Option A: Full slice as implied (recommended)

**S05: CLI Foundation & Index Command**
- `cli` crate: project root detection, `.code-graph/` setup, adapters (`RealFileSystem`, `ShellGitProvider`), clap CLI definition, index command handler, output formatting (minimal — just stats), tracing setup
- `binary` crate: `main.rs`, error-to-exit-code mapping
- Index pipeline: implement `full_index()` — file walk, parallel parse (rayon), import resolution, cross-file call resolution, store to SQLite
- Cross-file call resolution (Section 3.9): the four-step scoped/qualified/single-candidate/ambiguous strategy as a post-processing pass

### Option B: Split call resolution out

Same as Option A but defer cross-file call resolution to S06. S05 produces a graph with import edges but no cross-file `Calls` edges. S06 adds call resolution as a pre-query step.

**Trade-off:** Option B is simpler for S05 but means the graph is incomplete after indexing — `callers`/`callees` queries would return nothing until S06 also runs. This defeats the purpose of having a working `index` command.

---

## 5. Complexity Classification

| Aspect | Rating | Justification |
|--------|--------|---------------|
| **Algorithmic** | Medium-High | Cross-file call resolution (4-step), rayon parallelism with sequential phases |
| **Integration** | High | Creates 2 new crates, wires 4 existing crates together, implements 2 port adapters |
| **Domain knowledge** | Medium | CLI patterns (clap), project detection, adapter wiring are well-understood |
| **Dependencies** | Medium | `clap`, `tracing`, `tracing-subscriber`, `rayon`, possibly `git2` or `ignore` |
| **Testing** | Medium-High | Need integration tests that index real code fixtures and verify graph correctness end-to-end |

**Overall: High complexity** — primarily from integration breadth (wiring 4 crates) and the deferred cross-file call resolution.

---

## 6. Decisions

### Q1: Where does the parse+resolve orchestration live?
**Decision: (a) `ParseProvider` outbound port trait in domain.** Hexagonal architecture — the domain defines what it needs ("give me parse results for these files"), the `cli` crate implements it as an adapter using `ParserRegistry` + `ResolverRegistry` + `rayon`. Keeps `IndexUseCase` testable with a mock `ParseProvider` and domain dependency-free.

### Q2: Rayon — parser dep or cli dep?
**Decision: cli crate.** Parallelism is an orchestration concern at the composition root. Parser crate stays single-threaded and composable. The `ParseProvider` adapter in cli owns the `par_iter()` over files. Parser's `thread_local!` design is rayon-compatible but doesn't depend on rayon itself.

### Q3: git2 vs shell-out for GitProvider?
**Decision: Shell-out (`Command::new("git")`).** Lighter binary, simpler implementation. Git is already required on any machine that has a codebase to index. v0.1 only needs `git ls-files`, `git status --porcelain`, and `git diff`.

### Q4: `.code-graphignore` parsing?
**Decision: Include in S05.** Use the `ignore` crate (gitignore-syntax). Layer on top of `git ls-files` output to apply additional exclusion patterns from `.code-graph/config.toml` `[index].exclude` and `.code-graphignore`.

### Q5: Output formatting infrastructure?
**Decision: Build the full `OutputFormat` enum + `Formatter` trait.** Three modes (compact, table, JSON) as specified in Section 7.2. S06 reuses immediately. For the `index` command, compact prints a summary line, JSON prints `IndexStats` as JSON.
