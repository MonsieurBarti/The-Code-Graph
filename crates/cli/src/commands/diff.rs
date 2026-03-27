use domain::error::Result;
use domain::ports::GitProvider;
use domain::use_cases::impact::ImpactUseCase;

use crate::adapters::git::ShellGitProvider;
use crate::commands::helpers::{open_graph, parse_confidence};
use crate::commands::DiffArgs;
use crate::output::{print, OutputFormat};

pub fn run_diff(args: &DiffArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let git = ShellGitProvider::new(root);
    let hunks = git.diff_hunks(&args.from, args.to.as_deref())?;
    let confidence = parse_confidence(&args.confidence)?;
    let uc = ImpactUseCase::new(store);
    let report = uc.diff_impact(&hunks, args.depth, confidence)?;
    print(&report, output_format);
    Ok(())
}
