use domain::error::Result;
use domain::model::CloneConfig;
use domain::use_cases::clones::CloneUseCase;

use crate::adapters::fs::RealFileSystem;
use crate::commands::helpers::open_graph;
use crate::commands::ClonesArgs;
use crate::output::{print, OutputFormat};

pub fn run_clones(args: &ClonesArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let fs = RealFileSystem;
    let uc = CloneUseCase::new(store, fs, root);

    let config = CloneConfig {
        threshold: args.threshold,
        min_lines: args.min_lines,
        ..CloneConfig::default()
    };

    let analysis = uc.analyze(&config)?;

    if let Some(cluster_id) = args.cluster {
        if let Some(cluster) = analysis.clusters.iter().find(|c| c.id == cluster_id) {
            print(&vec![cluster.clone()], output_format);
        } else {
            eprintln!(
                "cluster {cluster_id} not found ({} clusters total)",
                analysis.clusters.len()
            );
        }
    } else {
        print(&analysis, output_format);
    }
    Ok(())
}
