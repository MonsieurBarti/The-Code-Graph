# Code Review Graph vs Code Graph AI: Deep Comparison

## Overview

| Dimension | code-review-graph (tirth8205) | code-graph-ai (MonsieurBarti) |
|-----------|-------------------------------|-------------------------------|
| **Language** | Python 3.10+ | Rust (edition 2024) |
| **Stars** | 3,649 | - |
| **Graph Engine** | SQLite (on-disk relational) | petgraph (in-memory, bincode cache) |
| **Languages Parsed** | 18 languages | 5 languages (TS/JS, Rust, Python, Go) |
| **MCP Protocol** | Yes (FastMCP, 22 tools) | No (Claude Code hooks only) |
| **CLI Commands** | ~10 (via MCP tools) | 25 dedicated commands |
| **Frontend** | VSCode extension + D3.js HTML | Svelte 5 + Sigma.js WebGL SPA |
| **AI Integration** | MCP server for any AI agent | Claude Code hooks + RAG agent |
| **Embeddings** | 3 providers (local, Google, MiniMax) | fastembed ONNX (BAAI/bge-small-en) |
| **Token Savings** | 8.2x benchmarked | 60-90% claimed |
| **Test Count** | 22 test files, 65% min coverage | 551 tests + Playwright E2E |

---

## Architecture: Where Each Project Excels

### code-review-graph does better:

1. **Broader language support (18 vs 5)**
   - Covers Python, TS/JS, Go, Rust, Java, Scala, C#, Ruby, Kotlin, Swift, PHP, Solidity, C/C++, Dart, R, Perl
   - code-graph-ai only supports TypeScript, JavaScript, Rust, Python, Go

2. **MCP Protocol (industry standard)**
   - Implements 22 MCP tools via FastMCP
   - Works with ANY MCP-compatible AI agent (Claude, Cursor, Windsurf, Zed, Continue, OpenCode, Antigravity)
   - code-graph-ai uses Claude Code hooks only -- locked to one platform

3. **Richer graph analysis**
   - Execution flow detection with entry point tracing and criticality scoring
   - Community detection via Leiden algorithm (igraph)
   - Risk scoring (0.0-1.0) combining flow participation, cross-community coupling, test coverage, security sensitivity
   - Wiki generation from communities
   - code-graph-ai has blast radius and dead code but no flow tracing, community detection, or risk scoring

4. **Multi-embedding provider support**
   - Local sentence-transformers, Google Gemini, MiniMax
   - code-graph-ai only has fastembed (ONNX local)

5. **Context-aware hints system**
   - Tracks session state, infers user intent (reviewing/debugging/refactoring/exploring)
   - Appends next-step suggestions to tool responses
   - code-graph-ai has no session-level awareness

6. **MCP Prompts**
   - 5 workflow templates: review_changes, architecture_map, debug_issue, onboard_developer, pre_merge_check
   - code-graph-ai has no equivalent

7. **Schema migrations**
   - Versioned v1-v5, idempotent, with rollback support
   - code-graph-ai has no migration path; cache version mismatch = full rebuild

8. **Transparent benchmarking**
   - Eval framework tested across 6 real repos (Express, FastAPI, Flask, Gin, HTTPX, Next.js)
   - Published precision/recall/F1 scores alongside the claimed token reduction

### code-graph-ai does better:

1. **Performance (Rust vs Python)**
   - Thread-local tree-sitter parsers + rayon parallelism = zero lock contention
   - Bincode cache for instant cold starts
   - 75ms debounced file watcher
   - Single static binary (~12 MB), zero runtime dependencies
   - code-review-graph is Python -- inherently slower for parsing-heavy workloads

2. **Deeper import resolution**
   - oxc_resolver for TS/JS (handles tsconfig paths, workspace aliases, barrel re-exports)
   - Multi-step resolution pipeline: workspace detection -> file resolution -> barrel chain -> symbol-level wiring
   - Rust use/pub-use resolution with crate-root module tree walk
   - Python package resolution with `__init__.py` detection
   - Go module resolution with go.mod
   - code-review-graph uses simpler regex-based import parsing

3. **Richer graph model**
   - 5 node types with 15 edge kinds (vs 5 node types, 7 edge kinds)
   - Distinguishes: BarrelReExportAll, ConditionalImport, SideEffectImport, DotImport, Embeds, HasDecorator
   - Non-parsed file awareness (docs, configs, assets, CI files appear as typed nodes)
   - Visibility modifiers (Pub/PubCrate/Private), async markers, decorators on symbols

4. **Web UI (Svelte + Sigma.js)**
   - Full SPA with WebGL graph rendering (Sigma.js + ForceAtlas2)
   - Real-time WebSocket updates from file watcher
   - Code panel with Shiki syntax highlighting
   - File tree navigation
   - RAG chat panel integrated in UI
   - code-review-graph has a basic D3.js HTML export + early VSCode extension (v0.2.0)

5. **RAG conversational agent**
   - Hybrid retrieval: structural (graph queries) + conceptual (vector embeddings)
   - Query classification: Structural / Conceptual / Hybrid
   - Source code citations with actual snippets (up to 40 lines)
   - Session memory with LRU eviction
   - Degrades gracefully without embeddings (structural-only mode)
   - code-review-graph has no built-in RAG

6. **Clone detection**
   - Structural fingerprinting (symbol kind, body size, edge counts, decorator count)
   - code-review-graph has no clone detection

7. **More CLI commands (25 vs ~10)**
   - Dedicated commands for: structure, file-summary, imports, clones, dead-code, diff, diff-impact, decorators, clusters, flow, rename, snapshot, daemon
   - Each with compact/table/json output options

8. **Background daemon**
   - IPC via Unix socket, PID management
   - Watches and re-indexes in the background
   - code-review-graph uses watchdog but no daemon mode

9. **Graph export**
   - DOT and Mermaid format export
   - code-review-graph has no graph export (only HTML visualization)

10. **Security in web server**
    - CSPRNG auth tokens, CSP headers, X-Content-Type-Options, X-Frame-Options
    - Localhost-only binding, strict CORS
    - code-review-graph web visualization has no auth

---

## Shared Weaknesses

| Weakness | code-review-graph | code-graph-ai |
|----------|-------------------|---------------|
| **Cross-file call ambiguity** | Unqualified name targets | Single-candidate-only matches |
| **No incremental resolution** | Flows/communities need full regen | Relationships may go stale |
| **No type inference** | Cannot resolve overloads/dynamic dispatch | Cannot resolve overloads/dynamic dispatch |
| **In-memory scaling** | SQLite (decent) | petgraph (memory-bound) |

---

## The Opportunity: What Neither Does Well

1. **Neither has a proper graph database** -- SQLite is relational, petgraph is in-memory. A true graph DB (Neo4j, or embedded like oxigraph) could enable more powerful traversal queries.

2. **Neither does incremental relationship resolution** -- Both re-parse changed files but don't incrementally update cross-file relationships.

3. **Neither does type-aware analysis** -- No type inference means call resolution for overloaded methods or dynamic dispatch is impossible.

4. **Neither has a truly polished web UI** -- code-review-graph's is a static HTML export; code-graph-ai's is more mature but still v3.

5. **Neither supports collaborative/team use** -- Both are single-developer, local-only tools.

6. **Search quality** -- code-review-graph's MRR is 0.35; code-graph-ai uses keyword prefix classification for RAG. Both have room for improvement.

7. **MCP + performance** -- code-review-graph has MCP but is slow (Python); code-graph-ai is fast (Rust) but has no MCP. The ideal tool would have both.
