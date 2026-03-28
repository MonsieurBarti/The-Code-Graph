use domain::error::Result;
use domain::model::{DeadCodeConfig, SymbolKind};
use domain::use_cases::dead_code::DeadCodeUseCase;

use crate::commands::helpers::open_graph;
use crate::commands::DeadCodeArgs;
use crate::config::load_config;
use crate::output::{print, OutputFormat};

pub fn run_dead_code(args: &DeadCodeArgs, output_format: OutputFormat) -> Result<()> {
    let (store, root) = open_graph()?;
    let config = load_config(&root)?;

    // Build DeadCodeConfig: defaults -> config.toml -> CLI flags
    let mut dead_config = DeadCodeConfig::default();

    // Apply config.toml [dead-code] section
    if let Some(dc) = &config.dead_code {
        if let Some(patterns) = &dc.exclude_patterns {
            dead_config.exclude_patterns.extend(patterns.clone());
        }
        if let Some(patterns) = &dc.entry_point_patterns {
            dead_config.entry_point_patterns.extend(patterns.clone());
        }
        if let Some(patterns) = &dc.migration_patterns {
            dead_config.migration_patterns = patterns.clone();
        }
    }

    // Apply CLI flags (unioned with config, not overriding)
    dead_config
        .exclude_patterns
        .extend(args.exclude_pattern.clone());
    dead_config.include_tests = args.include_tests;

    // Parse --kind flags into SymbolKind filter
    if !args.kind.is_empty() {
        let kinds: Vec<SymbolKind> = args
            .kind
            .iter()
            .filter_map(|k| match k.to_lowercase().as_str() {
                "function" => Some(SymbolKind::Function),
                "class" => Some(SymbolKind::Class),
                "interface" => Some(SymbolKind::Interface),
                "struct" => Some(SymbolKind::Struct),
                "trait" => Some(SymbolKind::Trait),
                "enum" => Some(SymbolKind::Enum),
                "typealias" | "type_alias" => Some(SymbolKind::TypeAlias),
                "method" => Some(SymbolKind::Method),
                "property" => Some(SymbolKind::Property),
                "const" => Some(SymbolKind::Const),
                "macro" => Some(SymbolKind::Macro),
                "variable" => Some(SymbolKind::Variable),
                "component" => Some(SymbolKind::Component),
                "test" => Some(SymbolKind::Test),
                _ => None,
            })
            .collect();
        if !kinds.is_empty() {
            dead_config.kind_filter = Some(kinds);
        }
    }

    let uc = DeadCodeUseCase::new(store);
    let mut analysis = uc.analyze(&dead_config)?;

    // Apply display-layer --limit
    if let Some(limit) = args.limit {
        analysis.dead_symbols.truncate(limit);
    }

    print(&analysis, output_format);
    Ok(())
}
