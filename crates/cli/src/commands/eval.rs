use domain::error::{CodeGraphError, Result};
use eval::report::SuiteResult;
use eval::{Suite, SuiteConfig};
use std::io::Write;

use super::EvalArgs;
use crate::output::{Displayable, OutputFormat};

fn suite_from_str(s: &str) -> Result<Suite> {
    match s {
        "search" => Ok(Suite::Search),
        "impact" => Ok(Suite::Impact),
        "core" => Ok(Suite::Core),
        "flows" => Ok(Suite::Flows),
        "risk" => Ok(Suite::Risk),
        "analysis" => Ok(Suite::Analysis),
        "invariants" => Ok(Suite::Invariants),
        "bench" => Ok(Suite::Bench),
        "all" => Ok(Suite::All),
        _ => Err(CodeGraphError::Other(format!(
            "Unknown suite '{}'. Valid: search, impact, core, flows, risk, analysis, invariants, bench, all",
            s
        ))),
    }
}

impl Displayable for SuiteResult {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        self.fmt_compact(w)
    }
    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        self.fmt_table(w)
    }
    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        self.fmt_json(w)
    }
}

pub fn run_eval(args: &EvalArgs, output_format: OutputFormat) -> Result<()> {
    let suite = suite_from_str(&args.suite)?;
    let suites_dir = find_suites_dir()?;

    let config = SuiteConfig {
        suite,
        no_cache: args.no_cache,
        suites_dir,
        search_limit: 20,
        compare_baseline: args.compare.clone(),
    };

    let result = eval::run_suite(&config)?;
    crate::output::print(&result, output_format);

    if !result.all_passed() {
        return Err(CodeGraphError::Other(
            "Quality targets not met — see results above".into(),
        ));
    }
    Ok(())
}

fn find_suites_dir() -> Result<std::path::PathBuf> {
    let cwd_suites = std::path::PathBuf::from("eval/suites");
    if cwd_suites.is_dir() {
        return Ok(cwd_suites);
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe_suites = exe.parent().unwrap_or(exe.as_ref()).join("eval/suites");
        if exe_suites.is_dir() {
            return Ok(exe_suites);
        }
    }
    Err(CodeGraphError::Other(
        "eval/suites/ directory not found — run from project root".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eval::report::{ImpactSuiteResult, SearchSuiteResult, SuiteResult};

    #[test]
    fn suite_from_string_search() {
        let suite = suite_from_str("search");
        assert!(suite.is_ok());
        assert!(matches!(suite.unwrap(), Suite::Search));
    }

    #[test]
    fn suite_from_string_impact() {
        let suite = suite_from_str("impact");
        assert!(suite.is_ok());
        assert!(matches!(suite.unwrap(), Suite::Impact));
    }

    #[test]
    fn suite_from_string_all() {
        let suite = suite_from_str("all");
        assert!(suite.is_ok());
        assert!(matches!(suite.unwrap(), Suite::All));
    }

    #[test]
    fn suite_from_string_invalid() {
        let suite = suite_from_str("unknown");
        assert!(suite.is_err());
        let msg = format!("{}", suite.unwrap_err());
        assert!(msg.contains("Unknown suite 'unknown'"));
    }

    #[test]
    fn suite_from_string_core() {
        assert!(matches!(suite_from_str("core").unwrap(), Suite::Core));
    }

    #[test]
    fn suite_from_string_flows() {
        assert!(matches!(suite_from_str("flows").unwrap(), Suite::Flows));
    }

    #[test]
    fn suite_from_string_risk() {
        assert!(matches!(suite_from_str("risk").unwrap(), Suite::Risk));
    }

    #[test]
    fn suite_from_string_analysis() {
        assert!(matches!(
            suite_from_str("analysis").unwrap(),
            Suite::Analysis
        ));
    }

    #[test]
    fn suite_from_string_invariants() {
        assert!(matches!(
            suite_from_str("invariants").unwrap(),
            Suite::Invariants
        ));
    }

    #[test]
    fn suite_from_string_bench() {
        assert!(matches!(suite_from_str("bench").unwrap(), Suite::Bench));
    }

    fn sample_suite_result() -> SuiteResult {
        SuiteResult {
            search: Some(SearchSuiteResult {
                repos: 3,
                queries: 30,
                mrr: 0.65,
                precision_at_5: 0.72,
                precision_at_10: 0.60,
                mrr_target: 0.30,
                mrr_passed: true,
                per_category: vec![],
            }),
            impact: Some(ImpactSuiteResult {
                repos: 3,
                scenarios: 15,
                precision: 0.55,
                recall: 0.45,
                f1: 0.50,
                precision_target: 0.40,
                precision_passed: true,
                recall_target: 0.30,
                recall_passed: true,
            }),
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        }
    }

    #[test]
    fn displayable_compact_output() {
        let result = sample_suite_result();
        let mut buf = Vec::new();
        Displayable::fmt_compact(&result, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Search Suite"));
        assert!(output.contains("MRR"));
        assert!(output.contains("Impact Suite"));
        assert!(output.contains("Precision"));
    }

    #[test]
    fn displayable_json_output() {
        let result = sample_suite_result();
        let mut buf = Vec::new();
        Displayable::fmt_json(&result, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert!(parsed.get("search").is_some());
        assert!(parsed.get("impact").is_some());
    }
}
