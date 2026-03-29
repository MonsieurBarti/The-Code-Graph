use domain::error::Result;
use domain::model::{FlowConfig, FlowStats};
use domain::use_cases::flow::FlowUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::FlowsArgs;
use crate::config::load_config;
use crate::output::{print, OutputFormat};

pub fn run_flows(args: &FlowsArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let config = load_config(&root)?;

    let mut flow_config = FlowConfig {
        max_depth: args.depth,
        ..FlowConfig::default()
    };
    if let Some(fc) = &config.flows {
        if let Some(extra) = &fc.extra_entry_points {
            flow_config.extra_entry_points = extra.clone();
        }
        if let Some(excluded) = &fc.excluded_entry_points {
            flow_config.excluded_entry_points = excluded.clone();
        }
    }

    let uc = FlowUseCase::new(store);

    if args.rank {
        let mut scores = uc.criticality()?;
        scores.truncate(args.limit);
        print(&scores, output_format);
    } else if let Some(ref symbol) = args.symbol {
        let flows = uc.flows_through(symbol, &flow_config)?;
        let analysis = domain::model::FlowAnalysis {
            entry_points: vec![],
            criticality: vec![],
            stats: FlowStats {
                total_entry_points: 0,
                total_flows: flows.len(),
                max_depth: flows.iter().map(|f| f.depth).max().unwrap_or(0),
                avg_depth: if flows.is_empty() {
                    0.0
                } else {
                    flows.iter().map(|f| f.depth as f64).sum::<f64>() / flows.len() as f64
                },
            },
            flows,
        };
        print(&analysis, output_format);
    } else {
        let mut analysis = uc.analyze(&flow_config)?;
        analysis.flows.truncate(args.limit);
        print(&analysis, output_format);
    }
    Ok(())
}
