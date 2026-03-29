# Requirements — v0.3 Ecosystem

## R1: Wiki Generation
Generate human-readable wiki documentation from community detection results (M02-S03). Each detected community produces a wiki page with: member symbols, inter-community edges, purpose summary, and entry points. Output as Markdown files suitable for GitHub wiki or docs site.

## R2: Web UI Visualization
Interactive web-based graph visualization. Render nodes (files, symbols) and edges with zoom, pan, filter by edge type/confidence, and click-to-inspect. Serve locally via `code-graph serve`. Must handle graphs with 10K+ nodes without freezing.

## R3: Multi-Repo Registry
Support indexing and querying across multiple repositories. A registry tracks known repos with their graph DB locations. Cross-repo queries resolve inter-repo dependencies (e.g., shared library used by multiple services). CLI commands: `code-graph registry add/remove/list/query`.

## R4: Refactoring Tools
Rename preview: given a symbol, show all locations that would need updating (callers, importers, re-exporters). Move suggestions: given a symbol being moved to a new file, show the dependency impact and suggest import updates. Output as a structured diff preview, not auto-applied.

## R5: MCP Adapter
Expose code-graph queries as an MCP (Model Context Protocol) server. Tools: `search`, `impact`, `find`, `refs`, `callers`, `callees`, `diff`, `stats`. Allows AI agents to query the graph without shell access. Transport: stdio (default) + SSE (optional).

## R6: Language Extensibility
Prove the architecture supports adding new languages by adding one additional language parser (e.g., Java, C#, or Ruby). Document the process as a contributor guide. Validate that no changes outside `parser/` crate are needed.

## R7: Real-World Validation
End-to-end validation against real open-source repositories (diverse languages, sizes, and patterns). Repos should include at least one per supported language ecosystem. Validate and fix all features across all milestones:
- **v0.1 Core:** `index` (full + incremental), `find`, `refs`, `callers`, `callees`, `search`, `diff`, `impact`, `stats`, `watch`, import resolution, cross-file call resolution, FTS5 search quality
- **v0.2 Analysis:** execution flows, risk scoring, community detection, embeddings + hybrid search, dead code detection, clone detection
- **v0.3 Ecosystem:** wiki generation, web UI, multi-repo registry, refactoring tools, MCP adapter, new language parser
- Fix any bugs, edge cases, or regressions found. This is the gate before release.
