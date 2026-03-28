pub mod callees;
pub mod callers;
pub mod clones;
pub mod diff;
pub mod eval;
pub mod find;
pub mod flows;
pub mod helpers;
pub mod impact;
pub mod index;
pub mod refs;
pub mod search;
pub mod setup;
pub mod setup_helpers;
pub mod stats;
pub mod stubs;
pub mod watch;

use clap::{ArgAction, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "code-graph",
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
    /// Analyze blast radius of changes
    Impact(ImpactArgs),
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
}

#[derive(clap::Args)]
pub struct EvalArgs {
    /// Which suite to run: search, impact, or all
    #[arg(long, default_value = "all")]
    pub suite: String,
    /// Force re-clone of eval repos (ignore cache)
    #[arg(long)]
    pub no_cache: bool,
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
            vec!["code-graph", "flows"],
            vec!["code-graph", "flows", "--rank"],
            vec!["code-graph", "flows", "--symbol", "foo::bar"],
            vec!["code-graph", "flows", "--depth", "10", "--limit", "50"],
            vec!["code-graph", "clones"],
            vec!["code-graph", "clones", "--threshold", "0.8"],
            vec!["code-graph", "clones", "--min-lines", "10"],
            vec!["code-graph", "clones", "--cluster", "1"],
            vec!["code-graph", "clones", "--threshold", "0.9", "--min-lines", "3", "--cluster", "2"],
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
        ];
        for args in &commands {
            Cli::parse_from(args.iter());
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
