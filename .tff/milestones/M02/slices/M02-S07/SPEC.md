# Spec — M02-S07: crates.io Release

## Problem

The code-graph binary is only installable from source (git clone + cargo build). To reach a wider audience and enable `cargo install the-code-graph`, we need to publish to crates.io with proper metadata and automate the release cycle so future versions ship with zero manual steps.

## Approach

**release-plz** monitors `main` for conventional commits, auto-creates a Release PR that bumps the version in `Cargo.toml` and generates/updates `CHANGELOG.md`. Merging that PR pushes a `v*` tag, which triggers the existing `release.yml` pipeline (eval gate → multi-platform build → GitHub Release → crates.io publish).

### Publish scope

- **Package name on crates.io**: `the-code-graph`
- **Installed binary name**: `code-graph` (via `[[bin]] name = "code-graph"`, unchanged)
- **All library crates**: `publish = false`
- **License**: MIT

### Changes

| Area | Change |
|---|---|
| `crates/binary/Cargo.toml` | Rename package to `the-code-graph`, add crates.io metadata, `readme = "../../README.md"` |
| All other `Cargo.toml` | Add `publish = false`, `edition.workspace = true` |
| Workspace `Cargo.toml` | Add `[workspace.package]` for shared metadata (repository, license, edition) |
| `LICENSE` | Add MIT license file at repo root |
| `release-plz.toml` | Config: only release `the-code-graph`, conventional commits, CHANGELOG |
| `.github/workflows/release-plz.yml` | New workflow: runs on push to `main`, invokes `release-plz-action` (pinned version). Uses a PAT or GitHub App token (not default `GITHUB_TOKEN`) so the tag push triggers `release.yml` |
| `.github/workflows/release.yml` | Simplify publish step: `cargo publish --dry-run` then `cargo publish -p the-code-graph`, remove `|| true` and multi-crate loop |

### release-plz flow

```
push to main
  → release-plz detects conventional commits (uses PAT for tag push)
  → creates Release PR (version bump + CHANGELOG)
  → merge PR
  → v* tag pushed (by release-plz, using PAT → triggers other workflows)
  → release.yml triggers
  → eval gate → multi-platform build → GitHub Release → crates.io publish
```

### Token requirement

The default `GITHUB_TOKEN` cannot trigger other workflows when pushing tags. The `release-plz.yml` workflow must use a **Personal Access Token (PAT)** or **GitHub App token** stored as a repository secret (e.g., `RELEASE_TOKEN`) so that the `v*` tag push triggers `release.yml`.

## Acceptance Criteria

1. `cargo publish --dry-run -p the-code-graph` succeeds (metadata valid, all required fields present)
2. All non-binary workspace crates have `publish = false`
3. All workspace crates use `edition.workspace = true`
4. `LICENSE` file exists at repo root with MIT license text
5. `release-plz.toml` exists and is configured for single-crate release
6. `.github/workflows/release-plz.yml` exists, runs on push to `main`, uses a non-default token for tag push
7. `.github/workflows/release.yml` publish step: dry-run then publish `the-code-graph` only (no `|| true`)
8. `[workspace.package]` in root `Cargo.toml` shares `repository`, `license`, `edition`
9. Binary crate has: `license`, `description`, `repository`, `readme`, `keywords`, `categories`

## Non-Goals

- Publishing library crates to crates.io
- Windows binary builds
- Homebrew / apt packaging
- Custom changelog format beyond release-plz defaults
