use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Serialize)]
pub struct SuiteResult {
    pub search: Option<SearchSuiteResult>,
    pub impact: Option<ImpactSuiteResult>,
    pub core: Option<CoreSuiteResult>,
    pub flows: Option<FlowsSuiteResult>,
    pub risk: Option<RiskSuiteResult>,
    pub analysis: Option<AnalysisSuiteResult>,
    pub invariants: Option<InvariantsSuiteResult>,
    pub bench: Option<BenchSuiteResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryMrr {
    pub category: String,
    pub queries: usize,
    pub mrr: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchSuiteResult {
    pub repos: usize,
    pub queries: usize,
    pub mrr: f64,
    pub precision_at_5: f64,
    pub precision_at_10: f64,
    pub mrr_target: f64,
    pub mrr_passed: bool,
    pub per_category: Vec<CategoryMrr>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactSuiteResult {
    pub repos: usize,
    pub scenarios: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub precision_target: f64,
    pub precision_passed: bool,
    pub recall_target: f64,
    pub recall_passed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoreSuiteResult {
    pub repos: usize,
    pub idempotent: bool,
    pub incremental_stable: bool,
    pub import_accuracy: f64,
    pub import_target: f64,
    pub import_passed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowsSuiteResult {
    pub repos: usize,
    pub entry_point_precision: f64,
    pub entry_point_target: f64,
    pub entry_point_passed: bool,
    pub invariant_violations: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskSuiteResult {
    pub repos: usize,
    pub top_n_precision: f64,
    pub top_n_target: f64,
    pub top_n_passed: bool,
    pub invariant_violations: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisSuiteResult {
    pub repos: usize,
    pub community_modularity: f64,
    pub dead_code_precision: f64,
    pub dead_code_target: f64,
    pub dead_code_passed: bool,
    pub clone_invariant_violations: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvariantsSuiteResult {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<crate::suites::InvariantResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchSuiteResult {
    pub repos: usize,
    pub baselines: serde_json::Value,
}

impl SuiteResult {
    /// Returns true if all quality targets are met.
    pub fn all_passed(&self) -> bool {
        let search_ok = self.search.as_ref().is_none_or(|s| s.mrr_passed);
        let impact_ok = self
            .impact
            .as_ref()
            .is_none_or(|i| i.precision_passed && i.recall_passed);
        let core_ok = self
            .core
            .as_ref()
            .is_none_or(|c| c.idempotent && c.incremental_stable && c.import_passed);
        let flows_ok = self
            .flows
            .as_ref()
            .is_none_or(|f| f.entry_point_passed && f.invariant_violations == 0);
        let risk_ok = self
            .risk
            .as_ref()
            .is_none_or(|r| r.top_n_passed && r.invariant_violations == 0);
        let analysis_ok = self
            .analysis
            .as_ref()
            .is_none_or(|a| a.dead_code_passed && a.clone_invariant_violations == 0);
        let invariants_ok = self.invariants.as_ref().is_none_or(|i| i.failed == 0);
        let bench_ok = self.bench.is_some() || self.bench.is_none(); // bench always passes
        search_ok
            && impact_ok
            && core_ok
            && flows_ok
            && risk_ok
            && analysis_ok
            && invariants_ok
            && bench_ok
    }

    /// Write compact human-readable output matching the SPEC format.
    pub fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let mut prev = false;
        if let Some(search) = &self.search {
            let status = if search.mrr_passed { "PASS" } else { "FAIL" };
            writeln!(
                w,
                "Search Suite — {} repos, {} queries",
                search.repos, search.queries
            )?;
            writeln!(
                w,
                "  MRR:          {:.2} (target: >={:.2}) {}",
                search.mrr, search.mrr_target, status
            )?;
            writeln!(w, "  Precision@5:  {:.2}", search.precision_at_5)?;
            writeln!(w, "  Precision@10: {:.2}", search.precision_at_10)?;
            if !search.per_category.is_empty() {
                writeln!(w, "  Per-category MRR:")?;
                for cat in &search.per_category {
                    writeln!(
                        w,
                        "    {:12} {} queries  MRR: {:.2}",
                        cat.category, cat.queries, cat.mrr
                    )?;
                }
            }
            prev = true;
        }
        if let Some(impact) = &self.impact {
            let p_status = if impact.precision_passed {
                "PASS"
            } else {
                "FAIL"
            };
            let r_status = if impact.recall_passed { "PASS" } else { "FAIL" };
            if prev {
                writeln!(w)?;
            }
            writeln!(
                w,
                "Impact Suite — {} repos, {} scenarios",
                impact.repos, impact.scenarios
            )?;
            writeln!(
                w,
                "  Precision:    {:.2} (target: >={:.2}) {}",
                impact.precision, impact.precision_target, p_status
            )?;
            writeln!(
                w,
                "  Recall:       {:.2} (target: >={:.2}) {}",
                impact.recall, impact.recall_target, r_status
            )?;
            writeln!(w, "  F1:           {:.2}", impact.f1)?;
            prev = true;
        }
        if let Some(core) = &self.core {
            let status = if core.import_passed { "PASS" } else { "FAIL" };
            if prev {
                writeln!(w)?;
            }
            writeln!(w, "Core Suite — {} repos", core.repos)?;
            writeln!(
                w,
                "  Idempotent:          {}",
                if core.idempotent { "PASS" } else { "FAIL" }
            )?;
            writeln!(
                w,
                "  Incremental Stable:  {}",
                if core.incremental_stable {
                    "PASS"
                } else {
                    "FAIL"
                }
            )?;
            writeln!(
                w,
                "  Import Accuracy:     {:.2} (target: >={:.2}) {}",
                core.import_accuracy, core.import_target, status
            )?;
            prev = true;
        }
        if let Some(flows) = &self.flows {
            let status = if flows.entry_point_passed {
                "PASS"
            } else {
                "FAIL"
            };
            if prev {
                writeln!(w)?;
            }
            writeln!(w, "Flows Suite — {} repos", flows.repos)?;
            writeln!(
                w,
                "  Entry Point Precision: {:.2} (target: >={:.2}) {}",
                flows.entry_point_precision, flows.entry_point_target, status
            )?;
            writeln!(w, "  Invariant Violations:  {}", flows.invariant_violations)?;
            prev = true;
        }
        if let Some(risk) = &self.risk {
            let status = if risk.top_n_passed { "PASS" } else { "FAIL" };
            if prev {
                writeln!(w)?;
            }
            writeln!(w, "Risk Suite — {} repos", risk.repos)?;
            writeln!(
                w,
                "  Top-N Precision:     {:.2} (target: >={:.2}) {}",
                risk.top_n_precision, risk.top_n_target, status
            )?;
            writeln!(w, "  Invariant Violations: {}", risk.invariant_violations)?;
            prev = true;
        }
        if let Some(analysis) = &self.analysis {
            let status = if analysis.dead_code_passed {
                "PASS"
            } else {
                "FAIL"
            };
            if prev {
                writeln!(w)?;
            }
            writeln!(w, "Analysis Suite — {} repos", analysis.repos)?;
            writeln!(
                w,
                "  Community Modularity:    {:.2}",
                analysis.community_modularity
            )?;
            writeln!(
                w,
                "  Dead Code Precision:     {:.2} (target: >={:.2}) {}",
                analysis.dead_code_precision, analysis.dead_code_target, status
            )?;
            writeln!(
                w,
                "  Clone Inv. Violations:   {}",
                analysis.clone_invariant_violations
            )?;
            prev = true;
        }
        if let Some(invariants) = &self.invariants {
            if prev {
                writeln!(w)?;
            }
            writeln!(
                w,
                "Invariants Suite — {}/{} passed",
                invariants.passed, invariants.total
            )?;
            writeln!(w, "  Failed: {}", invariants.failed)?;
            prev = true;
        }
        if let Some(bench) = &self.bench {
            if prev {
                writeln!(w)?;
            }
            writeln!(w, "Bench Suite — {} repos", bench.repos)?;
        }
        Ok(())
    }

    /// Write tabular breakdown of all metrics.
    pub fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Suite   | Metric       | Value | Target | Status")?;
        writeln!(w, "--------+--------------+-------+--------+-------")?;
        if let Some(search) = &self.search {
            let status = if search.mrr_passed { "PASS" } else { "FAIL" };
            writeln!(
                w,
                "Search  | MRR          | {:.2}  | >{:.2}  | {}",
                search.mrr, search.mrr_target, status
            )?;
            writeln!(
                w,
                "Search  | Precision@5  | {:.2}  |        |",
                search.precision_at_5
            )?;
            writeln!(
                w,
                "Search  | Precision@10 | {:.2}  |        |",
                search.precision_at_10
            )?;
            for cat in &search.per_category {
                writeln!(
                    w,
                    "Search  | MRR/{:<8} | {:.2}  |        |",
                    cat.category, cat.mrr
                )?;
            }
        }
        if let Some(impact) = &self.impact {
            let p_status = if impact.precision_passed {
                "PASS"
            } else {
                "FAIL"
            };
            let r_status = if impact.recall_passed { "PASS" } else { "FAIL" };
            writeln!(
                w,
                "Impact  | Precision    | {:.2}  | >{:.2}  | {}",
                impact.precision, impact.precision_target, p_status
            )?;
            writeln!(
                w,
                "Impact  | Recall       | {:.2}  | >{:.2}  | {}",
                impact.recall, impact.recall_target, r_status
            )?;
            writeln!(w, "Impact  | F1           | {:.2}  |        |", impact.f1)?;
        }
        if let Some(core) = &self.core {
            let status = if core.import_passed { "PASS" } else { "FAIL" };
            writeln!(
                w,
                "Core    | Idempotent   | {}  |        | {}",
                core.idempotent,
                if core.idempotent { "PASS" } else { "FAIL" }
            )?;
            writeln!(
                w,
                "Core    | IncrStable   | {}  |        | {}",
                core.incremental_stable,
                if core.incremental_stable {
                    "PASS"
                } else {
                    "FAIL"
                }
            )?;
            writeln!(
                w,
                "Core    | ImportAccu   | {:.2}  | >{:.2}  | {}",
                core.import_accuracy, core.import_target, status
            )?;
        }
        if let Some(flows) = &self.flows {
            let status = if flows.entry_point_passed {
                "PASS"
            } else {
                "FAIL"
            };
            writeln!(
                w,
                "Flows   | EntryPointP  | {:.2}  | >{:.2}  | {}",
                flows.entry_point_precision, flows.entry_point_target, status
            )?;
            writeln!(
                w,
                "Flows   | InvViolation | {}     |        |",
                flows.invariant_violations
            )?;
        }
        if let Some(risk) = &self.risk {
            let status = if risk.top_n_passed { "PASS" } else { "FAIL" };
            writeln!(
                w,
                "Risk    | TopNPrec     | {:.2}  | >{:.2}  | {}",
                risk.top_n_precision, risk.top_n_target, status
            )?;
            writeln!(
                w,
                "Risk    | InvViolation | {}     |        |",
                risk.invariant_violations
            )?;
        }
        if let Some(analysis) = &self.analysis {
            let status = if analysis.dead_code_passed {
                "PASS"
            } else {
                "FAIL"
            };
            writeln!(
                w,
                "Analysis| Modularity   | {:.2}  |        |",
                analysis.community_modularity
            )?;
            writeln!(
                w,
                "Analysis| DeadCodePrec | {:.2}  | >{:.2}  | {}",
                analysis.dead_code_precision, analysis.dead_code_target, status
            )?;
            writeln!(
                w,
                "Analysis| CloneInvViol | {}     |        |",
                analysis.clone_invariant_violations
            )?;
        }
        if let Some(invariants) = &self.invariants {
            writeln!(
                w,
                "Invarian| Total        | {}     |        |",
                invariants.total
            )?;
            writeln!(
                w,
                "Invarian| Passed       | {}     |        |",
                invariants.passed
            )?;
            writeln!(
                w,
                "Invarian| Failed       | {}     |        | {}",
                invariants.failed,
                if invariants.failed == 0 {
                    "PASS"
                } else {
                    "FAIL"
                }
            )?;
        }
        if let Some(bench) = &self.bench {
            writeln!(w, "Bench   | Repos        | {}     |        |", bench.repos)?;
        }
        Ok(())
    }

    /// Write JSON representation of all results.
    pub fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_search() -> SearchSuiteResult {
        SearchSuiteResult {
            repos: 5,
            queries: 52,
            mrr: 0.62,
            precision_at_5: 0.71,
            precision_at_10: 0.58,
            mrr_target: 0.30,
            mrr_passed: true,
            per_category: vec![
                CategoryMrr {
                    category: "exact".into(),
                    queries: 20,
                    mrr: 0.80,
                },
                CategoryMrr {
                    category: "semantic".into(),
                    queries: 20,
                    mrr: 0.50,
                },
                CategoryMrr {
                    category: "partial".into(),
                    queries: 12,
                    mrr: 0.45,
                },
            ],
        }
    }

    fn sample_impact() -> ImpactSuiteResult {
        ImpactSuiteResult {
            repos: 5,
            scenarios: 24,
            precision: 0.61,
            recall: 0.48,
            f1: 0.54,
            precision_target: 0.40,
            precision_passed: true,
            recall_target: 0.30,
            recall_passed: true,
        }
    }

    #[test]
    fn suite_result_compact_search_only() {
        let result = SuiteResult {
            search: Some(sample_search()),
            impact: None,
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        let mut buf = Vec::new();
        result.fmt_compact(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Search Suite — 5 repos, 52 queries"));
        assert!(output.contains("MRR:          0.62 (target: >=0.30) PASS"));
        assert!(output.contains("Precision@5:  0.71"));
        assert!(output.contains("Precision@10: 0.58"));
        assert!(!output.contains("Impact Suite"));
    }

    #[test]
    fn suite_result_compact_impact_only() {
        let result = SuiteResult {
            search: None,
            impact: Some(sample_impact()),
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        let mut buf = Vec::new();
        result.fmt_compact(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Impact Suite — 5 repos, 24 scenarios"));
        assert!(output.contains("Precision:    0.61 (target: >=0.40) PASS"));
        assert!(output.contains("Recall:       0.48"));
        assert!(output.contains("F1:           0.54"));
        assert!(!output.contains("Search Suite"));
    }

    #[test]
    fn suite_result_compact_all() {
        let result = SuiteResult {
            search: Some(sample_search()),
            impact: Some(sample_impact()),
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        let mut buf = Vec::new();
        result.fmt_compact(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Search Suite"));
        assert!(output.contains("Impact Suite"));
        // Verify both sections appear and a blank line separates them
        let search_pos = output.find("Search Suite").unwrap();
        let impact_pos = output.find("Impact Suite").unwrap();
        assert!(
            search_pos < impact_pos,
            "Search Suite should appear before Impact Suite"
        );
        // The blank line separator must exist somewhere between the two sections
        assert!(
            output.contains("\n\nImpact Suite"),
            "expected blank line before Impact Suite"
        );
    }

    #[test]
    fn suite_result_table_format() {
        let result = SuiteResult {
            search: Some(sample_search()),
            impact: Some(sample_impact()),
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        let mut buf = Vec::new();
        result.fmt_table(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Suite   | Metric       | Value | Target | Status"));
        assert!(output.contains("--------+--------------+-------+--------+-------"));
        assert!(output.contains("Search  | MRR"));
        assert!(output.contains("Search  | Precision@5"));
        assert!(output.contains("Search  | Precision@10"));
        assert!(output.contains("Impact  | Precision"));
        assert!(output.contains("Impact  | Recall"));
        assert!(output.contains("Impact  | F1"));
    }

    #[test]
    fn suite_result_json_format() {
        let result = SuiteResult {
            search: Some(sample_search()),
            impact: Some(sample_impact()),
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        let mut buf = Vec::new();
        result.fmt_json(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // Must be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert!(parsed.get("search").is_some());
        assert!(parsed.get("impact").is_some());
        let search = parsed.get("search").unwrap();
        assert_eq!(search.get("mrr").unwrap().as_f64().unwrap(), 0.62);
        assert_eq!(search.get("repos").unwrap().as_u64().unwrap(), 5);
    }

    #[test]
    fn quality_gate_all_pass() {
        let result = SuiteResult {
            search: Some(sample_search()),
            impact: Some(sample_impact()),
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        assert!(result.all_passed());
    }

    #[test]
    fn quality_gate_mrr_fail() {
        let mut search = sample_search();
        search.mrr_passed = false;
        let result = SuiteResult {
            search: Some(search),
            impact: Some(sample_impact()),
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        assert!(!result.all_passed());
    }

    #[test]
    fn quality_gate_precision_fail() {
        let mut impact = sample_impact();
        impact.precision_passed = false;
        let result = SuiteResult {
            search: Some(sample_search()),
            impact: Some(impact),
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        assert!(!result.all_passed());
    }

    #[test]
    fn suite_result_with_all_fields() {
        let result = SuiteResult {
            search: None,
            impact: None,
            core: None,
            flows: None,
            risk: None,
            analysis: None,
            invariants: None,
            bench: None,
        };
        assert!(result.all_passed()); // all None = pass
    }

    #[test]
    fn core_suite_result_serializes() {
        let r = CoreSuiteResult {
            repos: 5,
            idempotent: true,
            incremental_stable: true,
            import_accuracy: 0.75,
            import_target: 0.70,
            import_passed: true,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"idempotent\""));
    }
}
