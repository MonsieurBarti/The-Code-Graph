# The Code Graph

Index codebases into a queryable dependency graph.

## Install

```bash
cargo install the-code-graph
```

## Usage

```bash
# Index the current project
tcg index

# Find a symbol
tcg find MyFunction

# Show references
tcg refs MyFunction

# Analyze blast radius
tcg impact MyFunction

# Detect unused symbols
tcg dead-code
```

## License

MIT
