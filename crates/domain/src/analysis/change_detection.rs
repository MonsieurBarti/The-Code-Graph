use crate::model::{DiffHunk, SymbolNode};

/// Given a set of diff hunks and known symbols, find which symbols are affected.
/// Uses post-diff (new) line numbers for normal hunks.
/// For pure deletions (new_count = 0), uses old line numbers.
pub fn find_affected_symbols(hunks: &[DiffHunk], symbols: &[SymbolNode]) -> Vec<SymbolNode> {
    let mut affected = Vec::new();
    for symbol in symbols {
        let sym_file = symbol.location.file.to_string_lossy();
        let sym_start = symbol.location.line_start;
        let sym_end = symbol.location.line_end;

        for hunk in hunks {
            let hunk_file = hunk.file.to_string_lossy();
            if sym_file != hunk_file {
                continue;
            }

            // Determine the hunk's effective line range
            let (hunk_start, hunk_end) = if hunk.new_count == 0 {
                // Pure deletion — use old line range
                (
                    hunk.old_start,
                    hunk.old_start + hunk.old_count.saturating_sub(1),
                )
            } else {
                // Normal change — use new line range
                (
                    hunk.new_start,
                    hunk.new_start + hunk.new_count.saturating_sub(1),
                )
            };

            // Check line range overlap
            if sym_start <= hunk_end && hunk_start <= sym_end {
                affected.push(symbol.clone());
                break; // Don't add same symbol twice
            }
        }
    }
    affected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn sym(name: &str, file: &str, start: usize, end: usize) -> SymbolNode {
        SymbolNode {
            name: name.into(),
            qualified_name: format!("{file}::{name}"),
            kind: SymbolKind::Function,
            location: Location {
                file: file.into(),
                line_start: start,
                line_end: end,
                col_start: 0,
                col_end: 0,
            },
            visibility: Visibility::Public,
            is_exported: false,
            is_async: false,
            is_test: false,
            decorators: vec![],
            signature: None,
        }
    }

    #[test]
    fn overlapping_hunk_matches_symbol() {
        let symbols = vec![sym("foo", "src/a.rs", 10, 20)];
        let hunks = vec![DiffHunk {
            file: "src/a.rs".into(),
            old_start: 15,
            old_count: 3,
            new_start: 15,
            new_count: 5,
        }];
        let affected = find_affected_symbols(&hunks, &symbols);
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].name, "foo");
    }

    #[test]
    fn non_overlapping_hunk_no_match() {
        let symbols = vec![sym("foo", "src/a.rs", 10, 20)];
        let hunks = vec![DiffHunk {
            file: "src/a.rs".into(),
            old_start: 25,
            old_count: 3,
            new_start: 25,
            new_count: 3,
        }];
        let affected = find_affected_symbols(&hunks, &symbols);
        assert!(affected.is_empty());
    }

    #[test]
    fn different_file_no_match() {
        let symbols = vec![sym("foo", "src/a.rs", 10, 20)];
        let hunks = vec![DiffHunk {
            file: "src/b.rs".into(),
            old_start: 15,
            old_count: 3,
            new_start: 15,
            new_count: 3,
        }];
        let affected = find_affected_symbols(&hunks, &symbols);
        assert!(affected.is_empty());
    }

    #[test]
    fn pure_deletion_hunk_matches_symbol() {
        let symbols = vec![sym("foo", "src/a.rs", 10, 20)];
        let hunks = vec![DiffHunk {
            file: "src/a.rs".into(),
            old_start: 12,
            old_count: 3,
            new_start: 12,
            new_count: 0,
        }];
        let affected = find_affected_symbols(&hunks, &symbols);
        assert_eq!(affected.len(), 1);
    }
}
