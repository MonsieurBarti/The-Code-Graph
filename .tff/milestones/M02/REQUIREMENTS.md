# Requirements — v0.2 Analysis

## Goal

Enable deeper codebase intelligence beyond structural graph queries, and ship a distributable package to crates.io.

## Requirements

### R1: Execution Flow Detection + Criticality Scoring
- Detect execution flows through the graph (entry points → terminal nodes)
- Score nodes by criticality: how many flows pass through them
- Expose via CLI: `code-graph flows` and criticality in `code-graph stats`

### R2: Risk Scoring
- Composite risk score per symbol/file based on:
  - Flow participation (criticality from R1)
  - Coupling (in-degree + out-degree from graph edges)
  - Test coverage (presence/absence of `TestedBy` edges)
  - Security sensitivity (decorators, naming heuristics, e.g. `auth`, `crypto`, `password`)
- Expose via CLI: `code-graph risk <target>` with compact/table/json output

### R3: Community Detection (Leiden Algorithm)
- Partition the graph into communities (clusters of tightly-coupled symbols)
- Leiden algorithm for quality + performance
- Expose via CLI: `code-graph communities` listing communities with member counts
- Per-community detail: `code-graph communities <id>`

### R4: Embeddings + Hybrid Search
- Generate embeddings for symbols (name, signature, context)
- Hybrid search: FTS5 BM25 + vector similarity, fused via Reciprocal Rank Fusion (RRF)
- Improve search MRR beyond v0.1 baseline
- Expose via existing `code-graph search` command (transparent upgrade)

### R5: Dead Code Detection
- Identify symbols with zero incoming edges (excluding entry points and exports)
- Configurable entry-point patterns (e.g., `main`, test functions, exported symbols)
- Expose via CLI: `code-graph dead-code` with compact/table/json output

### R6: Clone Detection
- Detect structurally similar code blocks across the codebase
- Threshold-based similarity (configurable)
- Expose via CLI: `code-graph clones` with compact/table/json output

### R7: crates.io Release
- Publish `the-code-graph` binary crate to crates.io (`cargo install the-code-graph`)
- All workspace crates published with synchronized versions
- Release automation: version bump, CHANGELOG entry, git tag, GitHub release
- CI pipeline for release on tag push (`v*`)
