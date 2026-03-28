# M02-S02: Risk Scoring — Design Spec

## Problem

Developers reviewing or refactoring code need to prioritize effort. Which symbols and files carry the most risk? Without a composite signal, they must mentally combine flow criticality, coupling, test coverage, and security sensitivity — or ignore some.

Risk Scoring computes a single normalized score per symbol (and aggregated per file) so reviewers can sort by risk and focus where it matters.

## Approach

**Weighted linear combination of four normalized factors.**

1. **Criticality** — reuse betweenness centrality from FlowAnalysis (M02-S01). Normalized [0.0, 1.0].
2. **Coupling** — degree centrality: `(in_degree + out_degree) / max_degree` across all symbols. Excludes structural edges (`Contains`, `ChildOf`, `HasDecorator`) which inflate counts without measuring real coupling. Only edges where both source and target are in the symbol set contribute. When `max_degree = 0`, all coupling scores are `0.0`.
3. **Test gap** — `1.0` if symbol has zero incoming `TestedBy` edges, `0.0` if ≥1. Binary: "is this tested at all?" Higher = riskier.
4. **Security sensitivity** — `1.0` if symbol name or decorators match a security pattern, `0.0` otherwise. Case-insensitive word-boundary match (pattern must appear as a whole word or word prefix, not as a substring of an unrelated word). Built-in patterns: `auth, password, secret, token, crypto, credential, sql, exec, eval, unsafe`. Extensible via config.

**Composite**: `risk = Σ(w_i × f_i)`, clamped to [0.0, 1.0].
**File-level**: `max(symbol_scores)` within that file.
**Weights**: configurable in `.code-graph/config.toml`, defaults: `criticality=0.30, coupling=0.25, test_gap=0.25, sensitivity=0.20`.

### Why not alternatives?
- **Multiplicative model**: Harder to explain individual factor contributions. Diminishing returns make scores less intuitive.
- **Percentile ranking**: Self-calibrating but relative — can't compare across repos or set absolute thresholds.
- **ML-based**: Requires training data we don't have. Static heuristics are transparent and auditable.

## Domain Model Additions

### New types in `crates/domain/src/model.rs`

```rust
struct RiskScore {
    qualified_name: String,
    composite: f64,           // weighted sum, [0.0, 1.0]
    factors: RiskFactors,
}

struct RiskFactors {
    criticality: f64,         // betweenness centrality [0.0, 1.0]
    coupling: f64,            // degree centrality [0.0, 1.0]
    test_gap: f64,            // 1.0 = untested, 0.0 = tested
    sensitivity: f64,         // 1.0 = matches security pattern, 0.0 = no match
}

struct FileRiskScore {
    path: PathBuf,
    composite: f64,           // max of contained symbol scores
    symbol_count: usize,      // symbols scored in this file
    highest_symbol: String,   // which symbol drives the file score
}

struct RiskAnalysis {
    symbol_scores: Vec<RiskScore>,
    file_scores: Vec<FileRiskScore>,
    config: RiskConfig,
    stats: RiskStats,
}

struct RiskStats {
    symbols_scored: usize,
    files_scored: usize,
    avg_risk: f64,
    median_risk: f64,
    p90_risk: f64,            // 90th percentile
}

struct RiskConfig {
    weights: RiskWeights,
    security_patterns: Vec<String>,      // built-in + user-supplied
    min_score: f64,                       // filter threshold (default 0.0)
}

struct RiskWeights {
    criticality: f64,   // default 0.30
    coupling: f64,       // default 0.25
    test_gap: f64,       // default 0.25
    sensitivity: f64,    // default 0.20
}
```

## Analysis Algorithm

### New module: `crates/domain/src/analysis/risk.rs`

All pure functions — no side effects.

**`compute_criticality_scores(symbols, edges) → HashMap<String, f64>`**
- Call existing `brandes_betweenness()` from `analysis/flow.rs`
- Symbols not in the graph get 0.0

**`compute_coupling_scores(symbols, edges) → HashMap<String, f64>`**
- Filter edges: exclude structural kinds (`Contains`, `ChildOf`, `HasDecorator`); only count edges where both source and target are in the symbol set
- For each symbol: count in-degree + out-degree from filtered edges
- Find `max_degree` across all symbols
- If `max_degree = 0`: all coupling scores are `0.0` (avoid division by zero)
- Otherwise normalize: `coupling = degree / max_degree`
- Symbols with 0 edges get 0.0

**Note:** Criticality uses only high-confidence edges (per `brandes_betweenness`); coupling intentionally uses a broader set (all non-structural edges) to capture dependency surface beyond call paths.

**`compute_test_gaps(symbols, edges) → HashMap<String, f64>`**
- For each symbol: check if any incoming edge has `kind == TestedBy`
- `1.0` if no TestedBy edge (untested), `0.0` if tested

**`compute_sensitivity(symbols, patterns) → HashMap<String, f64>`**
- For each symbol: check `qualified_name` and `decorators` against pattern list
- Case-insensitive word-boundary match: pattern must appear at a word boundary (e.g., `auth` matches `auth_service`, `AuthToken`, but not `authenticate_author` at the `auth` in `author`)
- Implementation: split qualified_name on `_`, `.`, `::`, camelCase boundaries; match pattern against segments
- `1.0` if any pattern matches, `0.0` otherwise

**`score_symbols(criticality, coupling, test_gaps, sensitivity, weights) → Vec<RiskScore>`**
- For each symbol: `composite = Σ(w_i × f_i).clamp(0.0, 1.0)`
- Sorted descending by composite

**`aggregate_file_scores(symbol_scores, symbols) → Vec<FileRiskScore>`**
- Group symbols by containing file
- File score = max(symbol composites in file)
- Track which symbol drives the file score
- Sorted descending

## Use Case

### New: `crates/domain/src/use_cases/risk.rs`

```rust
struct RiskUseCase<S: GraphStore> {
    store: S,
}

impl<S: GraphStore> RiskUseCase<S> {
    fn analyze(&self, config: &RiskConfig) -> Result<RiskAnalysis>;
    fn score_symbol(&self, qualified_name: &str, config: &RiskConfig) -> Result<RiskScore>;
}
```

`analyze()` loads all symbols + edges once, computes all four factors, scores, aggregates.
`score_symbol()` is a convenience for single-target queries (still loads graph, filters result).

**No new port methods needed.** GraphStore already provides `all_symbols()`, `all_edges()`, `get_edges_to()`.

**Stats integration:** GraphStats gains optional fields:
- `avg_risk: Option<f64>`
- `p90_risk: Option<f64>`

## CLI

### New command: `code-graph risk`

```
code-graph risk                           # top files by risk (default limit 20)
code-graph risk --symbols                 # top symbols by risk
code-graph risk <target>                  # risk for a specific symbol or file
code-graph risk --symbols --limit 50      # top 50 symbols
code-graph risk --min-score 0.5           # only show risk >= 0.5
code-graph risk --json / --table          # output format
```

### Compact output (default — files)

```
Files by risk (top 20 of 234):
# File                            Risk   Driver
1 src/auth/service.rs             0.78   AuthService.validate
2 src/db/connection.rs            0.71   Database.connect
3 src/crypto/hash.rs              0.65   hash_password
```

### --symbols output

```
Symbols by risk (top 20 of 1,892):
# Symbol                          Risk  Crit  Coup  Test  Sec
1 AuthService.validate             0.82  0.72  0.81  1.00  1.00
2 Database.query                   0.71  0.85  0.90  0.00  0.00
```

### Single target

```
AuthService.validate  risk = 0.82
  criticality:  0.72  (betweenness centrality)
  coupling:     0.81  (in: 23, out: 8, max: 45)
  test_gap:     1.00  (no TestedBy edges)
  sensitivity:  1.00  (matches: auth)
  weights: crit=0.30 coup=0.25 test=0.25 sec=0.20
```

### Stats integration

```
Files: 234 | Symbols: 1,892 | Edges: 5,431
Entry points: 12 | Avg criticality: 0.034
Avg risk: 0.23 | P90 risk: 0.61
```

## Config

`.code-graph/config.toml` gains a `[risk]` section:

```toml
[risk]
weight_criticality = 0.30
weight_coupling = 0.25
weight_test_gap = 0.25
weight_sensitivity = 0.20

# Extra security-sensitive patterns (added to built-in list)
extra_security_patterns = ["unsafe", "inject"]

# Patterns to exclude from security sensitivity
excluded_security_patterns = []
```

Built-in patterns (hardcoded): `auth, password, secret, token, crypto, credential, sql, exec, eval, unsafe`

**Weight normalization**: weights are normalized to sum to 1.0 at load time. If a user provides `[0.5, 0.5, 0.5, 0.5]`, they become `[0.25, 0.25, 0.25, 0.25]`. This preserves relative proportions while ensuring the composite stays in [0.0, 1.0] without relying on the clamp.

**Exclusion semantics**: `excluded_security_patterns` removes patterns from the combined list (built-in + extra) before matching. If a symbol matches both an excluded and a non-excluded pattern, the non-excluded pattern still triggers sensitivity = 1.0. Exclusion operates on patterns, not on symbols.

## File Changes

| File | Change |
|------|--------|
| `crates/domain/src/model.rs` | Add RiskScore, RiskFactors, FileRiskScore, RiskAnalysis, RiskStats, RiskConfig, RiskWeights |
| `crates/domain/src/model.rs` | Add avg_risk + p90_risk to GraphStats |
| `crates/domain/src/analysis/mod.rs` | Add `pub mod risk;` |
| `crates/domain/src/analysis/risk.rs` | **NEW** — factor computation, scoring, aggregation |
| `crates/domain/src/use_cases/mod.rs` | Add `pub mod risk;` |
| `crates/domain/src/use_cases/risk.rs` | **NEW** — RiskUseCase |
| `crates/domain/src/lib.rs` | Re-export new types |
| `crates/cli/src/commands/mod.rs` | Add Risk command + RiskArgs |
| `crates/cli/src/commands/risk.rs` | **NEW** — run_risk() CLI handler |
| `crates/cli/src/config.rs` | Add RiskCliConfig with Deserialize, add `risk: Option<RiskCliConfig>` to CodeGraphConfig |
| `crates/cli/src/lib.rs` | Wire Risk command |

## Acceptance Criteria

1. `code-graph risk` lists files ranked by composite risk score (descending)
2. `code-graph risk --symbols` lists symbols ranked by risk (descending)
3. `code-graph risk <target>` shows all four factor values, composite score, matched patterns, and active weights for the target
4. Composite score = weighted linear sum of criticality, coupling, test_gap, sensitivity, clamped to [0.0, 1.0]
5. Criticality values equal betweenness centrality scores from `brandes_betweenness()` on the same graph (verified via unit test)
6. Coupling factor = `(in_degree + out_degree) / max_degree` using non-structural edges only (excludes `Contains`, `ChildOf`, `HasDecorator`); only edges where both endpoints are symbols; when `max_degree = 0`, all coupling scores are `0.0`
7. Test gap = 1.0 if no incoming TestedBy edges, 0.0 otherwise
8. Security sensitivity matches `SymbolNode.qualified_name` and `SymbolNode.decorators` against the active pattern list (case-insensitive word-boundary match, not substring)
9. Weights configurable via `.code-graph/config.toml` `[risk]` section
10. `extra_security_patterns` adds to the built-in list; `excluded_security_patterns` removes patterns from the combined list before matching (pattern-level, not symbol-level exclusion)
11. File-level score = max of contained symbol composite scores; files with zero scored symbols are excluded from output
12. `--min-score` flag filters output to entries with composite >= threshold (inclusive)
13. All three output formats: compact (default), `--table`, `--json` (JSON outputs `Vec<RiskScore>` / `Vec<FileRiskScore>` serialized with serde)
14. `code-graph stats` shows avg_risk and p90_risk
15. `code-graph risk` exits 0 and produces ≥1 file result on the dogfood codebase; `code-graph risk --symbols` produces ≥1 symbol result on test fixtures

## Non-Goals

- Historical risk tracking / trends (no persistence)
- Risk diffing between commits
- ML-based risk prediction
- Continuous (non-binary) test coverage weighting
- Cross-repository risk comparison
