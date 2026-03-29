pub mod callees;
pub mod callers;
pub mod clones;
pub mod communities;
pub mod dead_code;
pub mod diff;
pub mod eval;
pub mod find;
pub mod flows;
pub mod helpers;
pub mod impact;
pub mod index;
pub mod refs;
pub mod risk;
pub mod search;
pub mod setup;
pub mod setup_helpers;
pub mod stats;
pub mod stubs;
pub mod watch;

use clap::{ArgAction, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "tcg",
    version,
    about = "Index codebases into a queryable dependency graph"
)]
pub struct Cli {
    /// Increase verbosity (-v info, -vv debug)
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Enable debug logging
    #[arg(long, global = true)]
    pub debug: bool,

    /// Output as JSON
    #[arg(long, global = true)]
    pub json: bool,

    /// Output as table
    #[arg(long, global = true)]
    pub table: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Index the current project
    Index(IndexArgs),
    /// Find a symbol by name or pattern
    Find(FindArgs),
    /// Show references to a symbol
    Refs(RefsArgs),
    /// Analyze risk scores across the codebase
    Risk(RiskArgs),
    /// Analyze blast radius of changes
    Impact(ImpactArgs),
    /// Detect unused symbols in the codebase
    #[command(name = "dead-code")]
    DeadCode(DeadCodeArgs),
    /// Show symbols affected by git diff
    Diff(DiffArgs),
    /// Show callers of a symbol
    Callers(CallersArgs),
    /// Show callees of a symbol
    Callees(CalleesArgs),
    /// Full-text search across symbols
    Search(SearchArgs),
    /// Analyze execution flows and criticality
    Flows(FlowsArgs),
    /// Detect code clones across the codebase
    Clones(ClonesArgs),
    /// Detect communities of tightly-coupled symbols
    Communities(CommunitiesArgs),
    /// Show graph statistics
    Stats,
    /// Watch for file changes and re-index
    Watch(WatchArgs),
    /// Set up agent integration hooks
    Setup(SetupArgs),
    /// Run evaluation suite
    Eval(EvalArgs),
}

#[derive(clap::Args)]
pub struct IndexArgs {
    /// Path to the project root (defaults to auto-detect)
    #[arg(long)]
    pub path: Option<std::path::PathBuf>,

    /// Incremental update (only re-index changed files)
    #[arg(long)]
    pub incremental: bool,

    /// Specific files to re-index (implies --incremental)
    #[arg(long, value_delimiter = ',')]
    pub files: Option<Vec<std::path::PathBuf>>,

    /// Generate embeddings for all symbols (enables semantic search)
    #[arg(long)]
    pub embed: bool,

    /// ONNX model name for embeddings
    #[arg(long, default_value = "all-MiniLM-L6-v2")]
    pub embed_model: String,
}

#[derive(clap::Args)]
pub struct FindArgs {
    /// Symbol name or pattern to search for
    pub pattern: String,
}

#[derive(clap::Args)]
pub struct RefsArgs {
    /// Qualified name of the symbol
    pub qualified_name: String,
}

#[derive(clap::Args)]
pub struct RiskArgs {
    /// Specific symbol or file to analyze
    pub target: Option<String>,
    /// Show symbol-level risk instead of file-level
    #[arg(long)]
    pub symbols: bool,
    /// Maximum number of results to display
    #[arg(long, default_value = "20")]
    pub limit: usize,
    /// Minimum risk score to display
    #[arg(long, default_value = "0.0")]
    pub min_score: f64,
}

#[derive(clap::Args)]
pub struct ImpactArgs {
    /// Symbol name, qualified name, or file path to analyze
    pub target: String,
    /// Maximum traversal depth
    #[arg(long, default_value = "3")]
    pub depth: usize,
    /// Minimum confidence level (high, medium, low, all)
    #[arg(long, default_value = "all")]
    pub confidence: String,
}

#[derive(clap::Args)]
pub struct DiffArgs {
    /// Git ref to compare from (default: HEAD)
    #[arg(default_value = "HEAD")]
    pub from: String,
    /// Git ref to compare to (default: working tree)
    pub to: Option<String>,
    /// Maximum traversal depth
    #[arg(long, default_value = "3")]
    pub depth: usize,
    /// Minimum confidence level (high, medium, low, all)
    #[arg(long, default_value = "all")]
    pub confidence: String,
}

#[derive(clap::Args)]
pub struct CallersArgs {
    /// Qualified name of the symbol
    pub qualified_name: String,
}

#[derive(clap::Args)]
pub struct CalleesArgs {
    /// Qualified name of the symbol
    pub qualified_name: String,
}

#[derive(clap::Args)]
pub struct WatchArgs {
    /// Run as background daemon
    #[arg(long)]
    pub daemon: bool,

    /// Show daemon status
    #[arg(long)]
    pub status: bool,

    /// Stop running daemon
    #[arg(long)]
    pub stop: bool,

    /// Internal flag: marks this process as the daemon child
    #[arg(long, hide = true)]
    pub daemon_internal: bool,

    /// Path to the project root (defaults to auto-detect)
    #[arg(long)]
    pub path: Option<std::path::PathBuf>,
}

#[derive(clap::Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Maximum results to return
    #[arg(long, default_value = "20")]
    pub limit: usize,
    /// Use only vector similarity (skip FTS5)
    #[arg(long)]
    pub semantic_only: bool,
    /// Use only FTS5 BM25 (skip vectors)
    #[arg(long)]
    pub fts_only: bool,
}

#[derive(clap::Args)]
pub struct EvalArgs {
    /// Which suite to run
    #[arg(long, default_value = "all")]
    pub suite: String,
    /// Force re-clone of eval repos (ignore cache)
    #[arg(long)]
    pub no_cache: bool,
    /// Compare bench results against a baseline file
    #[arg(long)]
    pub compare: Option<std::path::PathBuf>,
}

#[derive(clap::Args)]
pub struct FlowsArgs {
    /// Filter flows through a specific symbol
    #[arg(long)]
    pub symbol: Option<String>,
    /// Show criticality ranking instead of flows
    #[arg(long)]
    pub rank: bool,
    /// Maximum flow depth
    #[arg(long, default_value = "20")]
    pub depth: usize,
    /// Maximum number of results to display
    #[arg(long, default_value = "20")]
    pub limit: usize,
}

#[derive(clap::Args)]
pub struct ClonesArgs {
    /// Similarity threshold (0.0-1.0)
    #[arg(long, default_value = "0.7")]
    pub threshold: f64,
    /// Minimum symbol body lines
    #[arg(long, default_value = "5")]
    pub min_lines: usize,
    /// Show detailed members of a specific cluster
    #[arg(long)]
    pub cluster: Option<usize>,
}

#[derive(clap::Args)]
pub struct CommunitiesArgs {
    /// Show details for a specific community
    pub community_id: Option<usize>,
    /// Modularity resolution parameter
    #[arg(long)]
    pub resolution: Option<f64>,
    /// Minimum community size to display
    #[arg(long)]
    pub min_size: Option<usize>,
    /// Random seed for reproducibility
    #[arg(long)]
    pub seed: Option<u64>,
    /// Show which community a symbol belongs to
    #[arg(long)]
    pub symbol: Option<String>,
    /// Maximum communities to display
    #[arg(long, default_value = "20")]
    pub limit: usize,
}

#[derive(clap::Args)]
pub struct SetupArgs {
    /// Target platform (currently: "claude")
    pub platform: Option<String>,
    /// Install to ~/.claude/settings.json instead of .claude/settings.json
    #[arg(long)]
    pub global: bool,
    /// Check hook installation status
    #[arg(long)]
    pub check: bool,
    /// Remove all code-graph hooks
    #[arg(long)]
    pub remove: bool,
    /// Also remove .code-graph/ from .gitignore (requires --remove)
    #[arg(long, requires = "remove")]
    pub clean: bool,
    /// Also delete .code-graph/ directory entirely (requires --remove)
    #[arg(long, requires = "remove")]
    pub purge: bool,
}

#[derive(clap::Args)]
pub struct DeadCodeArgs {
    /// Additional exclusion patterns (repeatable)
    #[arg(long = "exclude-pattern")]
    pub exclude_pattern: Vec<String>,
    /// Include test functions as dead code candidates
    #[arg(long)]
    pub include_tests: bool,
    /// Filter to specific symbol kinds (repeatable)
    #[arg(long)]
    pub kind: Vec<String>,
    /// Maximum results to display
    #[arg(long)]
    pub limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_index_command() {
        let cli = Cli::parse_from(["code-graph", "index"]);
        assert!(matches!(cli.command, Commands::Index(_)));
    }

    #[test]
    fn parse_find_command() {
        let cli = Cli::parse_from(["code-graph", "find", "Foo"]);
        if let Commands::Find(args) = cli.command {
            assert_eq!(args.pattern, "Foo");
        } else {
            panic!("expected Find command");
        }
    }

    #[test]
    fn parse_json_global_flag() {
        let cli = Cli::parse_from(["code-graph", "--json", "stats"]);
        assert!(cli.json);
    }

    #[test]
    fn parse_verbose_flag() {
        let cli = Cli::parse_from(["code-graph", "-vv", "stats"]);
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn parse_clones_command() {
        let cli = Cli::parse_from(["code-graph", "clones"]);
        if let Commands::Clones(args) = cli.command {
            assert!((args.threshold - 0.7).abs() < f64::EPSILON);
            assert_eq!(args.min_lines, 5);
            assert!(args.cluster.is_none());
        } else {
            panic!("expected Clones command");
        }
    }

    #[test]
    fn all_subcommands_parse() {
        let commands = [
            vec!["code-graph", "index"],
            vec!["code-graph", "find", "X"],
            vec!["code-graph", "refs", "a::b"],
            vec!["code-graph", "impact", "a::b"],
            vec!["code-graph", "diff"],
            vec!["code-graph", "callers", "a::b"],
            vec!["code-graph", "callees", "a::b"],
            vec!["code-graph", "search", "foo"],
            vec!["code-graph", "search", "foo", "--semantic-only"],
            vec!["code-graph", "search", "foo", "--fts-only"],
            vec!["code-graph", "index", "--embed"],
            vec!["code-graph", "index", "--embed", "--embed-model", "custom"],
            vec!["code-graph", "flows"],
            vec!["code-graph", "flows", "--rank"],
            vec!["code-graph", "flows", "--symbol", "foo::bar"],
            vec!["code-graph", "flows", "--depth", "10", "--limit", "50"],
            vec!["code-graph", "clones"],
            vec!["code-graph", "clones", "--threshold", "0.8"],
            vec!["code-graph", "clones", "--min-lines", "10"],
            vec!["code-graph", "clones", "--cluster", "1"],
            vec![
                "code-graph",
                "clones",
                "--threshold",
                "0.9",
                "--min-lines",
                "3",
                "--cluster",
                "2",
            ],
            vec!["code-graph", "risk"],
            vec!["code-graph", "risk", "--symbols"],
            vec!["code-graph", "risk", "--symbols", "--limit", "50"],
            vec!["code-graph", "risk", "AuthService"],
            vec!["code-graph", "risk", "--min-score", "0.5"],
            vec!["code-graph", "stats"],
            vec!["code-graph", "watch"],
            vec!["code-graph", "watch", "--daemon"],
            vec!["code-graph", "watch", "--status"],
            vec!["code-graph", "watch", "--stop"],
            vec!["code-graph", "setup", "claude"],
            vec!["code-graph", "setup", "--check"],
            vec!["code-graph", "setup", "--remove"],
            vec!["code-graph", "setup", "--remove", "--clean"],
            vec!["code-graph", "setup", "--remove", "--purge"],
            vec!["code-graph", "eval"],
            vec!["code-graph", "eval", "--suite", "search"],
            vec!["code-graph", "eval", "--no-cache"],
            vec!["code-graph", "communities"],
            vec!["code-graph", "communities", "--resolution", "1.5"],
            vec![
                "code-graph",
                "communities",
                "--seed",
                "42",
                "--min-size",
                "3",
            ],
            vec!["code-graph", "communities", "1"],
            vec!["code-graph", "communities", "--symbol", "src/main.rs::main"],
            vec!["code-graph", "dead-code"],
            vec!["code-graph", "dead-code", "--include-tests"],
            vec!["code-graph", "dead-code", "--exclude-pattern", "**/gen/**"],
            vec![
                "code-graph",
                "dead-code",
                "--kind",
                "Function",
                "--limit",
                "10",
            ],
        ];
        for args in &commands {
            Cli::parse_from(args.iter());
        }
    }

    #[test]
    fn parse_dead_code_command() {
        let cli = Cli::parse_from(["code-graph", "dead-code"]);
        assert!(matches!(cli.command, Commands::DeadCode(_)));
    }

    #[test]
    fn parse_dead_code_with_flags() {
        let cli = Cli::parse_from([
            "code-graph",
            "dead-code",
            "--include-tests",
            "--exclude-pattern",
            "**/generated/**",
            "--kind",
            "Function",
            "--limit",
            "50",
        ]);
        if let Commands::DeadCode(args) = cli.command {
            assert!(args.include_tests);
            assert_eq!(args.exclude_pattern, vec!["**/generated/**"]);
            assert_eq!(args.kind, vec!["Function"]);
            assert_eq!(args.limit, Some(50));
        } else {
            panic!("expected DeadCode command");
        }
    }

    #[test]
    fn parse_risk_command() {
        let cli = Cli::parse_from(["code-graph", "risk"]);
        assert!(matches!(cli.command, Commands::Risk(_)));
    }

    #[test]
    fn parse_risk_symbols() {
        let cli = Cli::parse_from(["code-graph", "risk", "--symbols", "--limit", "50"]);
        if let Commands::Risk(args) = cli.command {
            assert!(args.symbols);
            assert_eq!(args.limit, 50);
        } else {
            panic!("expected Risk command");
        }
    }

    #[test]
    fn parse_risk_target() {
        let cli = Cli::parse_from(["code-graph", "risk", "AuthService"]);
        if let Commands::Risk(args) = cli.command {
            assert_eq!(args.target.unwrap(), "AuthService");
        } else {
            panic!("expected Risk command");
        }
    }

    #[test]
    fn parse_risk_min_score() {
        let cli = Cli::parse_from(["code-graph", "risk", "--min-score", "0.5"]);
        if let Commands::Risk(args) = cli.command {
            assert!((args.min_score - 0.5).abs() < f64::EPSILON);
        } else {
            panic!("expected Risk command");
        }
    }

    #[test]
    fn parse_search_with_semantic_only() {
        let cli = Cli::parse_from(["code-graph", "search", "foo", "--semantic-only"]);
        if let Commands::Search(args) = cli.command {
            assert!(args.semantic_only);
            assert!(!args.fts_only);
        } else {
            panic!("expected Search");
        }
    }

    #[test]
    fn parse_search_with_fts_only() {
        let cli = Cli::parse_from(["code-graph", "search", "foo", "--fts-only"]);
        if let Commands::Search(args) = cli.command {
            assert!(args.fts_only);
            assert!(!args.semantic_only);
        } else {
            panic!("expected Search");
        }
    }

    #[test]
    fn parse_index_with_embed() {
        let cli = Cli::parse_from(["code-graph", "index", "--embed"]);
        if let Commands::Index(args) = cli.command {
            assert!(args.embed);
            assert_eq!(args.embed_model, "all-MiniLM-L6-v2");
        } else {
            panic!("expected Index");
        }
    }

    #[test]
    fn parse_index_with_embed_model() {
        let cli = Cli::parse_from([
            "code-graph",
            "index",
            "--embed",
            "--embed-model",
            "custom-model",
        ]);
        if let Commands::Index(args) = cli.command {
            assert!(args.embed);
            assert_eq!(args.embed_model, "custom-model");
        } else {
            panic!("expected Index");
        }
    }

    #[test]
    fn stub_returns_not_implemented() {
        let result = stubs::not_implemented("find");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("not yet implemented"));
    }
}
