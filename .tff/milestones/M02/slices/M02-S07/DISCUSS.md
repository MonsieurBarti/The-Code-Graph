# Discuss — M02-S07: crates.io Release

## Decisions

- **Publish scope**: Single binary crate `the-code-graph` only. All library crates `publish = false`.
- **Binary name**: `code-graph` (unchanged `[[bin]]` target). Package name on crates.io is `the-code-graph`.
- **License**: MIT
- **Automation**: release-plz (fully automated Release PRs from conventional commits)
- **Token**: PAT or GitHub App token required for cross-workflow tag triggering

## Complexity

- **Tier**: F-lite
- **Files affected**: ~12
- **New files**: 3 (release-plz.toml, release-plz.yml, LICENSE)
- **Architecture impact**: None (config/metadata only)
- **External integrations**: crates.io, release-plz GitHub Action
