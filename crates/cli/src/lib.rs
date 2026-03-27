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
        Commands::Find(_) => commands::stubs::not_implemented("find"),
        Commands::Refs(_) => commands::stubs::not_implemented("refs"),
        Commands::Impact(_) => commands::stubs::not_implemented("impact"),
        Commands::Diff(_) => commands::stubs::not_implemented("diff"),
        Commands::Callers(_) => commands::stubs::not_implemented("callers"),
        Commands::Callees(_) => commands::stubs::not_implemented("callees"),
        Commands::Search(_) => commands::stubs::not_implemented("search"),
        Commands::Stats => commands::stubs::not_implemented("stats"),
        Commands::Watch => commands::stubs::not_implemented("watch"),
        Commands::Setup => commands::stubs::not_implemented("setup"),
        Commands::Eval => commands::stubs::not_implemented("eval"),
    }
}
