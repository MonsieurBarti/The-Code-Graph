pub mod index;
pub mod stubs;
pub mod helpers;

use clap::{Parser, Subcommand, ArgAction};

#[derive(Parser)]
#[command(name = "code-graph", version, about = "Index codebases into a queryable dependency graph")]
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
    /// Show graph statistics
    Stats,
    /// Watch for file changes and re-index
    Watch,
    /// Initialize project configuration
    Setup,
    /// Run evaluation suite
    Eval,
}

#[derive(clap::Args)]
pub struct IndexArgs {
    /// Path to the project root (defaults to auto-detect)
    #[arg(long)]
    pub path: Option<std::path::PathBuf>,
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
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Maximum results to return
    #[arg(long, default_value = "20")]
    pub limit: usize,
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
            vec!["code-graph", "stats"],
            vec!["code-graph", "watch"],
            vec!["code-graph", "setup"],
            vec!["code-graph", "eval"],
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
