use domain::error::Result;
use domain::model::RiskConfig;
use domain::use_cases::risk::RiskUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::RiskArgs;
use crate::config::load_config;
use crate::output::{print, OutputFormat};

pub fn run_risk(args: &RiskArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let config = load_config(&root)?;

    // Build RiskConfig from defaults + config.toml overrides
    let mut risk_config = RiskConfig::default();

    if let Some(rc) = &config.risk {
        // Override weights if specified
        if let Some(w) = rc.weight_criticality {
            risk_config.weights.criticality = w;
        }
        if let Some(w) = rc.weight_coupling {
            risk_config.weights.coupling = w;
        }
        if let Some(w) = rc.weight_test_gap {
            risk_config.weights.test_gap = w;
        }
        if let Some(w) = rc.weight_sensitivity {
            risk_config.weights.sensitivity = w;
        }

        // Merge security patterns: built-in + extra - excluded
        if let Some(extra) = &rc.extra_security_patterns {
            risk_config.security_patterns.extend(extra.clone());
        }
        if let Some(excluded) = &rc.excluded_security_patterns {
            risk_config
                .security_patterns
                .retain(|p| !excluded.contains(p));
        }
    }

    // Normalize weights
    risk_config.weights = risk_config.weights.normalized();

    let uc = RiskUseCase::new(store);

    if let Some(ref target) = args.target {
        // Single target mode
        let score = uc.score_symbol(target, &risk_config)?;
        print(&score, output_format);
    } else if args.symbols {
        // Symbol list mode
        let analysis = uc.analyze(&risk_config)?;
        let filtered: Vec<_> = analysis
            .symbol_scores
            .into_iter()
            .filter(|s| s.composite >= args.min_score)
            .take(args.limit)
            .collect();
        print(&filtered, output_format);
    } else {
        // Default: file list mode
        let analysis = uc.analyze(&risk_config)?;
        let mut filtered_analysis = analysis;
        filtered_analysis
            .file_scores
            .retain(|f| f.composite >= args.min_score);
        filtered_analysis.file_scores.truncate(args.limit);
        print(&filtered_analysis, output_format);
    }
    Ok(())
}
