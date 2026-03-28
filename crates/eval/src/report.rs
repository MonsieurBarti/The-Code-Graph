use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Serialize)]
pub struct SuiteResult {
    pub search: Option<SearchSuiteResult>,
    pub impact: Option<ImpactSuiteResult>,
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
}

impl SuiteResult {
    /// Returns true if all quality targets are met.
    pub fn all_passed(&self) -> bool {
        let search_ok = self.search.as_ref().is_none_or(|s| s.mrr_passed);
        let impact_ok = self.impact.as_ref().is_none_or(|i| i.precision_passed);
        search_ok && impact_ok
    }

    /// Write compact human-readable output matching the SPEC format.
    pub fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
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
        }
        if let Some(impact) = &self.impact {
            let status = if impact.precision_passed {
                "PASS"
            } else {
                "FAIL"
            };
            if self.search.is_some() {
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
                impact.precision, impact.precision_target, status
            )?;
            writeln!(w, "  Recall:       {:.2}", impact.recall)?;
            writeln!(w, "  F1:           {:.2}", impact.f1)?;
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
            let status = if impact.precision_passed {
                "PASS"
            } else {
                "FAIL"
            };
            writeln!(
                w,
                "Impact  | Precision    | {:.2}  | >{:.2}  | {}",
                impact.precision, impact.precision_target, status
            )?;
            writeln!(
                w,
                "Impact  | Recall       | {:.2}  |        |",
                impact.recall
            )?;
            writeln!(w, "Impact  | F1           | {:.2}  |        |", impact.f1)?;
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
        }
    }

    #[test]
    fn suite_result_compact_search_only() {
        let result = SuiteResult {
            search: Some(sample_search()),
            impact: None,
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
        };
        assert!(!result.all_passed());
    }
}
