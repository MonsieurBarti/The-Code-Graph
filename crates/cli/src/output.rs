use std::io::Write;

use domain::model::IndexStats;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Compact,
    Table,
    Json,
}

impl OutputFormat {
    pub fn from_flags(json: bool, table: bool) -> Self {
        if json {
            Self::Json
        } else if table {
            Self::Table
        } else {
            Self::Compact
        }
    }
}

pub trait Displayable {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()>;
    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()>;
    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()>;
}

pub fn print<T: Displayable>(value: &T, format: OutputFormat) {
    let stdout = std::io::stdout();
    let mut w = stdout.lock();
    match format {
        OutputFormat::Compact => value.fmt_compact(&mut w),
        OutputFormat::Table => value.fmt_table(&mut w),
        OutputFormat::Json => value.fmt_json(&mut w),
    }
    .expect("failed to write to stdout");
}

impl Displayable for IndexStats {
    fn fmt_compact(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(
            w,
            "Indexed {} files, {} symbols, {} edges in {:.1}s",
            self.files_indexed,
            self.symbols_extracted,
            self.edges_created,
            self.duration.as_secs_f64()
        )
    }

    fn fmt_table(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Metric         | Count")?;
        writeln!(w, "---------------+----------")?;
        writeln!(w, "Files indexed  | {}", self.files_indexed)?;
        writeln!(w, "Symbols        | {}", self.symbols_extracted)?;
        writeln!(w, "Edges          | {}", self.edges_created)?;
        writeln!(w, "Duration       | {:.1}s", self.duration.as_secs_f64())
    }

    fn fmt_json(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self)
            .map_err(std::io::Error::other)?;
        writeln!(w, "{json}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample_stats() -> IndexStats {
        IndexStats {
            files_indexed: 42,
            symbols_extracted: 128,
            edges_created: 256,
            duration: Duration::from_secs_f64(1.5),
        }
    }

    #[test]
    fn output_format_from_flags_json() {
        assert_eq!(OutputFormat::from_flags(true, false), OutputFormat::Json);
    }

    #[test]
    fn output_format_from_flags_table() {
        assert_eq!(OutputFormat::from_flags(false, true), OutputFormat::Table);
    }

    #[test]
    fn output_format_from_flags_compact() {
        assert_eq!(OutputFormat::from_flags(false, false), OutputFormat::Compact);
    }

    #[test]
    fn output_format_json_takes_precedence() {
        assert_eq!(OutputFormat::from_flags(true, true), OutputFormat::Json);
    }

    #[test]
    fn index_stats_compact_format() {
        let stats = sample_stats();
        let mut buf = Vec::new();
        stats.fmt_compact(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("42 files"));
        assert!(s.contains("128 symbols"));
        assert!(s.contains("256 edges"));
        assert!(s.contains("1.5s"));
    }

    #[test]
    fn index_stats_json_format() {
        let stats = sample_stats();
        let mut buf = Vec::new();
        stats.fmt_json(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["files_indexed"], 42);
        assert_eq!(parsed["symbols_extracted"], 128);
        assert_eq!(parsed["edges_created"], 256);
    }

    #[test]
    fn index_stats_table_format() {
        let stats = sample_stats();
        let mut buf = Vec::new();
        stats.fmt_table(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Files indexed"));
        assert!(s.contains("42"));
        assert!(s.contains("Symbols"));
        assert!(s.contains("128"));
    }
}
