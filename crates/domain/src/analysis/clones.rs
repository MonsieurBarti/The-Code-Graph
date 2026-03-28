// Clone detection analysis — implemented in T03-T06

use std::collections::{HashMap, HashSet};

/// Split source code into tokens.
/// - Strips line comments (`//` and `#`)
/// - Splits on whitespace
/// - Splits tokens further on punctuation boundaries (keeps punctuation as separate tokens)
/// - Skips empty tokens
pub fn tokenize(source: &str) -> Vec<String> {
    let mut tokens = Vec::new();

    for line in source.lines() {
        // Strip line comments: // and #
        let stripped = if let Some(pos) = line.find("//") {
            &line[..pos]
        } else if let Some(pos) = line.find('#') {
            &line[..pos]
        } else {
            line
        };

        for word in stripped.split_whitespace() {
            // Split word on punctuation boundaries, keeping punctuation as separate tokens
            split_on_punctuation(word, &mut tokens);
        }
    }

    tokens
}

/// Split a word on punctuation boundaries, keeping punctuation as separate tokens.
fn split_on_punctuation(word: &str, out: &mut Vec<String>) {
    let mut current = String::new();

    for ch in word.chars() {
        if ch.is_ascii_punctuation() {
            // Flush current alphanumeric token
            if !current.is_empty() {
                out.push(current.clone());
                current.clear();
            }
            // Push punctuation as its own token
            out.push(ch.to_string());
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
}

/// Replace identifiers with positional placeholders.
/// - Keywords, operators/punctuation, and numeric literals are kept as-is.
/// - Identifiers are replaced with `_1`, `_2`, etc. (same identifier → same placeholder everywhere).
pub fn normalize_identifiers(tokens: &[String]) -> Vec<String> {
    let keywords: HashSet<&str> = [
        // Rust
        "fn", "let", "mut", "const", "struct", "enum", "impl", "trait", "pub", "use", "mod",
        "if", "else", "match", "for", "while", "loop", "return", "break", "continue", "where",
        "async", "await", "move", "ref", "type", "self", "super", "crate",
        // TypeScript/JS
        "function", "var", "class", "interface", "export", "import", "from", "default", "extends",
        "implements", "new", "this", "typeof", "instanceof", "void", "null", "undefined", "true",
        "false", "try", "catch", "throw", "finally",
        // Python
        "def", "class", "import", "from", "return", "if", "elif", "else", "for", "while", "with",
        "as", "try", "except", "raise", "pass", "None", "True", "False", "lambda",
        // Go
        "func", "package", "import", "type", "struct", "interface", "map", "chan", "go", "defer",
        "select", "case", "range", "nil",
    ]
    .iter()
    .copied()
    .collect();

    let mut identifier_map: HashMap<String, usize> = HashMap::new();
    let mut counter = 0usize;
    let mut result = Vec::with_capacity(tokens.len());

    for token in tokens {
        // Keep keywords as-is
        if keywords.contains(token.as_str()) {
            result.push(token.clone());
            continue;
        }

        // Keep purely punctuation tokens as-is (single or multi-char operator/punct)
        if token.chars().all(|c| c.is_ascii_punctuation()) {
            result.push(token.clone());
            continue;
        }

        // Keep numeric literals as-is
        if token.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            result.push(token.clone());
            continue;
        }

        // It's an identifier — replace with positional placeholder
        let next_id = identifier_map.len() + 1;
        let idx = identifier_map.entry(token.clone()).or_insert(next_id);
        if *idx == next_id {
            counter += 1;
            // idx was just inserted as next_id; keep it (counter tracks inserts)
            let _ = counter; // suppress unused warning
        }
        result.push(format!("_{}", idx));
    }

    result
}

/// Jaccard similarity on token multisets.
/// - intersection = sum of min(count_a, count_b) for each token
/// - union = sum of max(count_a, count_b) for each token
/// - Returns intersection / union (both empty → 1.0, one empty → 0.0)
pub fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let mut counts_a: HashMap<&str, usize> = HashMap::new();
    let mut counts_b: HashMap<&str, usize> = HashMap::new();

    for token in a {
        *counts_a.entry(token.as_str()).or_insert(0) += 1;
    }
    for token in b {
        *counts_b.entry(token.as_str()).or_insert(0) += 1;
    }

    // Collect all unique tokens
    let all_tokens: HashSet<&str> = counts_a.keys().chain(counts_b.keys()).copied().collect();

    let mut intersection = 0usize;
    let mut union = 0usize;

    for token in &all_tokens {
        let ca = counts_a.get(token).copied().unwrap_or(0);
        let cb = counts_b.get(token).copied().unwrap_or(0);
        intersection += ca.min(cb);
        union += ca.max(cb);
    }

    if union == 0 {
        return 1.0;
    }

    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_identical_tokens() {
        let a = vec!["fn".to_string(), "foo".into(), "(".into(), ")".into()];
        let b = a.clone();
        assert!((jaccard_similarity(&a, &b) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_disjoint_tokens() {
        let a = vec!["fn".to_string(), "foo".into()];
        let b = vec!["class".to_string(), "Bar".into()];
        assert!((jaccard_similarity(&a, &b) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_partial_overlap() {
        let a = vec!["fn".to_string(), "foo".into(), "(".into()];
        let b = vec!["fn".to_string(), "bar".into(), "(".into()];
        // intersection: fn, ( = 2; union: fn, foo, bar, ( = 4; jaccard = 0.5
        assert!((jaccard_similarity(&a, &b) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn normalize_replaces_identifiers_with_positional_placeholders() {
        let tokens = vec![
            "fn".to_string(),
            "add".into(),
            "(".into(),
            "a".into(),
            ",".into(),
            "b".into(),
            ")".into(),
            "{".into(),
            "a".into(),
            "+".into(),
            "b".into(),
            "}".into(),
        ];
        let normalized = normalize_identifiers(&tokens);
        assert_eq!(normalized[0], "fn"); // keyword kept
        assert_eq!(normalized[1], "_1"); // add -> _1
        assert_eq!(normalized[3], "_2"); // a -> _2
        assert_eq!(normalized[5], "_3"); // b -> _3
        assert_eq!(normalized[8], "_2"); // a again -> _2
        assert_eq!(normalized[10], "_3"); // b again -> _3
    }

    #[test]
    fn type2_clones_detected_after_normalization() {
        let tokens_a = vec![
            "fn".to_string(),
            "add".into(),
            "(".into(),
            "x".into(),
            ",".into(),
            "y".into(),
            ")".into(),
            "{".into(),
            "x".into(),
            "+".into(),
            "y".into(),
            "}".into(),
        ];
        let tokens_b = vec![
            "fn".to_string(),
            "sum".into(),
            "(".into(),
            "a".into(),
            ",".into(),
            "b".into(),
            ")".into(),
            "{".into(),
            "a".into(),
            "+".into(),
            "b".into(),
            "}".into(),
        ];
        let raw_score = jaccard_similarity(&tokens_a, &tokens_b);
        assert!(raw_score < 0.95);
        let norm_a = normalize_identifiers(&tokens_a);
        let norm_b = normalize_identifiers(&tokens_b);
        let norm_score = jaccard_similarity(&norm_a, &norm_b);
        assert!((norm_score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tokenize_strips_comments_and_splits() {
        let source = "fn foo() { // comment\n  let x = 1;\n}";
        let tokens = tokenize(source);
        assert!(!tokens.contains(&"comment".to_string()));
        assert!(tokens.contains(&"fn".to_string()));
        assert!(tokens.contains(&"foo".to_string()));
    }

    #[test]
    fn tokenize_empty_source() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn jaccard_both_empty() {
        assert!((jaccard_similarity(&[], &[]) - 1.0).abs() < f64::EPSILON);
    }
}
