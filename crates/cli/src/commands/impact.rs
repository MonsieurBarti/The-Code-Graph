use std::path::PathBuf;

use domain::error::Result;
use domain::model::ImpactTarget;
use domain::use_cases::impact::ImpactUseCase;

use crate::commands::helpers::{open_graph, parse_confidence};
use crate::commands::ImpactArgs;
use crate::output::{print, OutputFormat};

fn has_source_extension(s: &str) -> bool {
    [".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go"]
        .iter()
        .any(|ext| s.ends_with(ext))
}

fn disambiguate_target(target: &str) -> ImpactTarget {
    if target.contains("::") {
        ImpactTarget::Symbol(target.to_string())
    } else if target.contains('/') || has_source_extension(target) {
        ImpactTarget::File(PathBuf::from(target))
    } else {
        ImpactTarget::Symbol(target.to_string())
    }
}

pub fn run_impact(args: &ImpactArgs, output_format: OutputFormat) -> Result<()> {
    let (store, _root) = open_graph()?;
    let target = disambiguate_target(&args.target);
    let confidence = parse_confidence(&args.confidence)?;
    let uc = ImpactUseCase::new(store);
    let report = uc.blast_radius(&[target], args.depth, confidence)?;
    print(&report, output_format);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disambiguate_qualified_name() {
        match disambiguate_target("src/main.rs::foo") {
            ImpactTarget::Symbol(s) => assert_eq!(s, "src/main.rs::foo"),
            _ => panic!("expected Symbol"),
        }
    }

    #[test]
    fn disambiguate_file_path_with_slash() {
        match disambiguate_target("src/main.rs") {
            ImpactTarget::File(p) => assert_eq!(p, PathBuf::from("src/main.rs")),
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn disambiguate_file_with_known_extension() {
        match disambiguate_target("main.ts") {
            ImpactTarget::File(p) => assert_eq!(p, PathBuf::from("main.ts")),
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn disambiguate_bare_name_is_symbol() {
        match disambiguate_target("MyClass") {
            ImpactTarget::Symbol(s) => assert_eq!(s, "MyClass"),
            _ => panic!("expected Symbol"),
        }
    }
}
