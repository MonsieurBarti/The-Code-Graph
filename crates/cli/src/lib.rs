pub mod adapters;
pub mod commands;
pub mod config;
pub mod logging;
pub mod output;
pub mod project;

use domain::error::Result;
use commands::{Cli, Commands};
use output::OutputFormat;

pub fn run(cli: Cli) -> Result<()> {
    let output_format = OutputFormat::from_flags(cli.json, cli.table);

    match &cli.command {
        Commands::Index(args) => commands::index::run_index(args, output_format),
        Commands::Find(args) => commands::find::run_find(args, output_format),
        Commands::Refs(args) => commands::refs::run_refs(args, output_format),
        Commands::Impact(args) => commands::impact::run_impact(args, output_format),
        Commands::Diff(args) => commands::diff::run_diff(args, output_format),
        Commands::Callers(args) => commands::callers::run_callers(args, output_format),
        Commands::Callees(args) => commands::callees::run_callees(args, output_format),
        Commands::Search(args) => commands::search::run_search(args, output_format),
        Commands::Stats => commands::stats::run_stats(output_format),
        Commands::Watch(args) => commands::watch::run_watch(args),
        Commands::Setup(args) => commands::setup::run_setup(args),
        Commands::Eval(args) => commands::eval::run_eval(args, output_format),
    }
}
