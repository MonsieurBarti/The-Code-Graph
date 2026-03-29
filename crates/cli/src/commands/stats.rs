use domain::error::Result;
use domain::model::{CloneConfig, FlowConfig};
use domain::use_cases::clones::CloneUseCase;
use domain::use_cases::flow::FlowUseCase;
use domain::use_cases::query::QueryUseCase;

use crate::adapters::fs::RealFileSystem;
use crate::commands::helpers::open_graph;
use crate::output::{print, OutputFormat};

pub fn run_stats(output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let uc = QueryUseCase::new(store.clone(), store.clone());
    let mut stats = uc.stats()?;

    // On-demand flow analysis integration
    let clone_store = store.clone();
    let flow_uc = FlowUseCase::new(store);
    let flow_config = FlowConfig::default();
    let analysis = flow_uc.analyze(&flow_config)?;
    stats.entry_point_count = Some(analysis.stats.total_entry_points);

    // Only compute avg_criticality if <= 5000 symbols (Brandes' is O(V*E))
    if stats.symbols <= 5000 {
        let avg = if analysis.criticality.is_empty() {
            0.0
        } else {
            analysis
                .criticality
                .iter()
                .map(|c| c.betweenness)
                .sum::<f64>()
                / analysis.criticality.len() as f64
        };
        stats.avg_criticality = Some(avg);
    }

    // On-demand clone analysis integration
    if stats.symbols <= 10_000 {
        let clone_fs = RealFileSystem;
        let clone_uc = CloneUseCase::new(clone_store, clone_fs, root);
        let clone_config = CloneConfig::default();
        if let Ok(clone_analysis) = clone_uc.analyze(&clone_config) {
            stats.clone_clusters = Some(clone_analysis.clusters.len());
            stats.duplication_pct = Some(clone_analysis.duplication_pct);
            stats.most_duplicated = clone_analysis.most_duplicated;
        }
    }

    print(&stats, output_format);
    Ok(())
}
