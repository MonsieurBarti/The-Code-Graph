use domain::error::Result;
use domain::model::CommunityConfig;
use domain::use_cases::community::CommunityUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::CommunitiesArgs;
use crate::config::load_config;
use crate::output::{print, OutputFormat};

pub fn run_communities(args: &CommunitiesArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let config = load_config(&root)?;

    let mut community_config = CommunityConfig::default();

    // Config file overrides (lowest priority after defaults)
    if let Some(cc) = &config.communities {
        if let Some(r) = cc.resolution {
            community_config.resolution = r;
        }
        if let Some(s) = cc.min_community_size {
            community_config.min_community_size = s;
        }
        if let Some(s) = cc.seed {
            community_config.seed = Some(s);
        }
    }

    // CLI flag overrides (highest priority)
    if let Some(r) = args.resolution {
        community_config.resolution = r;
    }
    if let Some(s) = args.min_size {
        community_config.min_community_size = s;
    }
    if let Some(s) = args.seed {
        community_config.seed = Some(s);
    }

    let uc = CommunityUseCase::new(store);

    if let Some(ref symbol) = args.symbol {
        match uc.community_of(symbol, &community_config)? {
            Some(c) => {
                eprintln!(
                    "{} -> Community #{} ({}, {} members)",
                    symbol,
                    c.id,
                    c.name,
                    c.members.len()
                );
            }
            None => {
                eprintln!("symbol '{}' not found in any community", symbol);
            }
        }
        return Ok(());
    }

    let mut analysis = uc.analyze(&community_config)?;

    if let Some(community_id) = args.community_id {
        if let Some(c) = analysis.communities.iter().find(|c| c.id == community_id) {
            print(&vec![c.clone()], output_format);
        } else {
            eprintln!(
                "community {} not found ({} communities total)",
                community_id,
                analysis.communities.len()
            );
        }
    } else {
        analysis.communities.truncate(args.limit);
        print(&analysis, output_format);
    }
    Ok(())
}
