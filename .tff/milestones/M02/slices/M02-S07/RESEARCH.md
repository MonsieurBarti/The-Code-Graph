# Research — M02-S07: crates.io Release

## Findings

### Workspace Structure

**Root `Cargo.toml`** (`/Cargo.toml`):
- `[workspace]` with `members` and `resolver = "2"` only
- No `[workspace.package]` section exists
- No `[workspace.dependencies]` section exists
- No `[patch]` or `[replace]` sections
- 9 workspace members: `domain`, `storage`, `parser`, `watch`, `cli`, `binary`, `eval`, `benches`, `embeddings`

**Missing repo-root files**:
- No `README.md` at repo root (only `.beads/README.md`)
- No `LICENSE` file at repo root
- No `CHANGELOG.md`
- `Cargo.lock` exists (86.8K)

### Binary Crate Metadata

**`crates/binary/Cargo.toml`** current state:

| Field | Value |
|---|---|
| `name` | `binary` (needs rename to `the-code-graph`) |
| `version` | `0.1.0` |
| `edition` | `2021` (set locally) |
| `[[bin]] name` | `code-graph` |
| `[[bin]] path` | `src/main.rs` |
| `description` | **missing** |
| `license` | **missing** |
| `repository` | **missing** |
| `readme` | **missing** |
| `keywords` | **missing** |
| `categories` | **missing** |
| `publish` | not set (defaults to `true`) |

Dependencies: `cli` (path), `domain` (path), `clap` (v4). No features block.

The binary crate is a thin wrapper -- `main.rs` is 27 lines, parsing CLI via `clap`, initializing logging, and delegating to `cli::run`.

### Library Crates

| Crate | Package Name | `publish = false`? | Edition | Path Dependencies |
|---|---|---|---|---|
| `domain` | `domain` | No | `2021` (local) | none |
| `storage` | `storage` | No | `2021` (local) | `domain` |
| `parser` | `parser` | No | `2021` (local) | `domain` |
| `watch` | `watch` | No | `2021` (local) | `domain` |
| `cli` | `cli` | No | `2021` (local) | `domain`, `parser`, `storage`, `watch`, `eval`, `embeddings` (optional) |
| `eval` | `eval` | No | `2021` (local) | `domain`, `parser`, `storage` |
| `embeddings` | `embeddings` | No | **`2024`** (local) | `domain` |
| `benches` | `code-graph-benches` | **Yes** | `2021` (local) | `parser`, `storage`, `domain` |

Key observations:
- Only `benches` has `publish = false` today
- 7 library crates need `publish = false` added: `domain`, `storage`, `parser`, `watch`, `cli`, `eval`, `embeddings`
- `embeddings` uses edition `2024`, all others use `2021`
- No crate has `license`, `description`, `repository`, or any other crates.io metadata

### CI Workflows

**`.github/workflows/release.yml`** (current):
- Triggered on `push tags: ["v*"]`
- Pipeline: `eval-gate` -> `build` (4 targets) -> `publish`
- Publish step (lines 87-113):
  1. Creates GitHub Release via `softprops/action-gh-release@v2` with `generate_release_notes: true`
  2. Publishes to crates.io with a **loop over all crates** in order: `domain`, `parser`, `storage`, `watch`, `eval`, `cli`, `binary`
  3. Uses `|| true` to swallow publish failures
  4. Sleeps 30 seconds between each crate publish
  5. Uses `CARGO_REGISTRY_TOKEN` secret

This publish step is problematic because:
- It tries to publish library crates that should be `publish = false`
- The `|| true` masks real failures
- The 30-second sleep is a fragile workaround for crates.io dependency propagation
- Package name `binary` would not match intended `the-code-graph`

**`.github/workflows/ci.yml`**:
- Triggers on PRs to `main` and `milestone/*`
- Jobs: fmt, clippy, test, test-embeddings, coverage, audit, bench
- Not directly relevant to publishing, but confirms CI runs on milestone branches

**No existing release-plz config**:
- No `release-plz.toml` or `.release-plz.toml`
- No `.github/workflows/release-plz.yml`

### License

- **No `LICENSE` file** exists at the repo root
- No `Cargo.toml` in any crate references a `license` field
- crates.io requires either `license` or `license-file` field

### Dependencies

**Path dependency graph** (relevant to publishing):
```
binary -> cli, domain
cli -> domain, parser, storage, watch, eval, embeddings (optional)
eval -> domain, parser, storage
parser -> domain
storage -> domain
watch -> domain
embeddings -> domain
benches -> parser, storage, domain
```

Since only the binary crate will be published and all library crates will have `publish = false`, the path dependencies are fine. `cargo publish` for a binary crate that depends on path-only (non-published) crates works because:
- The binary is compiled from source when installed via `cargo install`
- Wait -- this is wrong. **`cargo install` from crates.io cannot resolve path dependencies.** If the binary crate is published and its dependencies reference `path = "../cli"` without also being on crates.io, `cargo install the-code-graph` will fail.

**This is the critical issue**: for `cargo install the-code-graph` to work, either:
1. All dependency crates must also be published to crates.io (not desired per spec), OR
2. The binary crate must vendor/inline all code (not practical), OR
3. Dependencies must use `{ path = "../cli", version = "0.1.0" }` dual-source syntax AND be published

Since the spec says "all library crates get `publish = false`", this creates a contradiction -- the binary crate cannot be published to crates.io if its dependencies are not also published.

**Resolution options**:
- A) Publish all library crates too (contradicts spec)
- B) Only publish GitHub Releases (no crates.io)
- C) Restructure into a single crate (major refactor, out of scope)
- D) Publish library crates with `publish = false` removed, using version-pinned path+version deps

## Risks & Considerations

### Critical: Path dependency publishing conflict
The binary crate (`the-code-graph`) depends on `cli` and `domain` via path references. `cli` in turn depends on 5 other workspace crates. **crates.io requires all dependencies to be available on the registry.** Publishing only the binary crate while keeping all libraries at `publish = false` will fail `cargo publish --dry-run`.

This must be resolved before implementation. Options:
1. **Publish all crates** (simplest): Remove the `publish = false` constraint from the spec. All 7 library crates get published with `the-code-graph-` prefix (e.g., `the-code-graph-domain`). release-plz handles the ordering.
2. **Consolidate into single crate**: Merge all library code into the binary crate. Major refactor, not appropriate for this slice.
3. **GitHub-only releases**: Drop crates.io publishing, keep only GitHub Release binaries. Users install via `cargo install --git` or download binaries.

### Crate name conflicts
- Package names like `domain`, `storage`, `parser`, `cli`, `eval`, `watch` are extremely generic and likely taken on crates.io. If publishing libraries, they need prefixed names (e.g., `the-code-graph-domain`).

### Edition mismatch
- `embeddings` uses edition `2024` while all other crates use `2021`. If `[workspace.package] edition = "2021"` is set, `embeddings` must override it locally. This is fine with `edition = "2024"` in its own `Cargo.toml` (overrides workspace default).

### README requirement
- No `README.md` exists at the repo root. crates.io strongly encourages one, and the spec calls for `readme = "../../README.md"` in the binary crate. A README must be created (or the field omitted).

### Token for release-plz
- The spec correctly identifies that `GITHUB_TOKEN` cannot trigger other workflows. A PAT or GitHub App token (`RELEASE_TOKEN`) must be configured as a repository secret.

## Recommendations

1. **Resolve the publishing conflict first** -- this is a blocker. Discuss with the project owner whether to (a) publish all library crates with prefixed names, (b) drop crates.io and keep GitHub-only releases, or (c) restructure. The spec as written is not achievable without one of these changes.

2. **If publishing all crates**: rename library package names to `the-code-graph-{name}` (e.g., `the-code-graph-domain`), update all internal `path` deps to include `version` field, remove `publish = false` from spec requirement, configure release-plz for workspace-level releases.

3. **If keeping single-crate publish**: the only viable path is restructuring the binary crate to not depend on unpublished path crates (major refactor).

4. **Remaining implementation** (once blocker resolved):
   - Add `[workspace.package]` with `edition = "2021"`, `license = "MIT"`, `repository = "https://github.com/MonsieurBarti/The-Code-Graph"`
   - Migrate all crates to `edition.workspace = true` (with `embeddings` overriding to `2024`)
   - Add metadata to binary crate: `description`, `license.workspace = true`, `repository.workspace = true`, `readme`, `keywords`, `categories`
   - Create `LICENSE` (MIT) at repo root
   - Create `release-plz.toml` scoped to the publish-target crate(s)
   - Create `.github/workflows/release-plz.yml`
   - Simplify `release.yml` publish step (remove loop, remove `|| true`)
   - Create `README.md` at repo root (or at minimum a stub)
