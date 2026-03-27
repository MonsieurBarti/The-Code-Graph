use std::collections::HashMap;
use std::path::Path;

use domain::model::Language;

use crate::{
    GoParser, JavaScriptParser, LanguageParser, PythonParser, RustParser, TypeScriptParser,
};

/// Registry of language parsers with extension-based dispatch.
pub struct ParserRegistry {
    parsers: Vec<Box<dyn LanguageParser>>,
    extension_map: HashMap<String, usize>,
}

impl ParserRegistry {
    /// Create registry with all supported language parsers.
    pub fn new() -> Self {
        let mut registry = Self {
            parsers: Vec::new(),
            extension_map: HashMap::new(),
        };
        registry.register(Box::new(TypeScriptParser::new()));
        registry.register(Box::new(JavaScriptParser::new()));
        registry.register(Box::new(RustParser::new()));
        registry.register(Box::new(PythonParser::new()));
        registry.register(Box::new(GoParser::new()));
        registry
    }

    fn register(&mut self, parser: Box<dyn LanguageParser>) {
        let idx = self.parsers.len();
        for ext in parser.file_extensions() {
            self.extension_map.insert(ext.to_string(), idx);
        }
        self.parsers.push(parser);
    }

    /// Get the parser for a file based on its extension.
    pub fn parser_for_file(&self, path: &Path) -> Option<&dyn LanguageParser> {
        let ext = path.extension()?.to_str()?;
        let idx = self.extension_map.get(ext)?;
        Some(self.parsers[*idx].as_ref())
    }

    /// Get the parser for a specific Language enum value.
    pub fn parser_for_language(&self, lang: Language) -> Option<&dyn LanguageParser> {
        self.parsers
            .iter()
            .find(|p| p.language() == lang)
            .map(|p| p.as_ref())
    }

    /// List all supported file extensions.
    pub fn supported_extensions(&self) -> Vec<&str> {
        self.extension_map.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Compile-time assertion: ParserRegistry is Send + Sync
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ParserRegistry>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parser_for_ts_file() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_file(Path::new("foo.ts"));
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().language(), Language::TypeScript);
    }

    #[test]
    fn parser_for_tsx_file() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_file(Path::new("foo.tsx"));
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().language(), Language::TypeScript);
    }

    #[test]
    fn parser_for_js_file() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_file(Path::new("foo.js"));
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().language(), Language::JavaScript);
    }

    #[test]
    fn parser_for_jsx_file() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_file(Path::new("foo.jsx"));
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().language(), Language::JavaScript);
    }

    #[test]
    fn parser_for_rs_file() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_file(Path::new("foo.rs"));
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().language(), Language::Rust);
    }

    #[test]
    fn parser_for_py_file() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_file(Path::new("foo.py"));
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().language(), Language::Python);
    }

    #[test]
    fn parser_for_go_file() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_file(Path::new("foo.go"));
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().language(), Language::Go);
    }

    #[test]
    fn parser_for_txt_returns_none() {
        let registry = ParserRegistry::new();
        assert!(registry.parser_for_file(Path::new("foo.txt")).is_none());
    }

    #[test]
    fn parser_for_language_typescript() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_language(Language::TypeScript);
        assert!(parser.is_some());
    }

    #[test]
    fn parser_for_language_rust() {
        let registry = ParserRegistry::new();
        let parser = registry.parser_for_language(Language::Rust);
        assert!(parser.is_some());
    }

    #[test]
    fn supported_extensions_contains_all_five_languages() {
        let registry = ParserRegistry::new();
        let exts = registry.supported_extensions();
        // TypeScript + JavaScript
        assert!(exts.contains(&"ts"));
        assert!(exts.contains(&"tsx"));
        assert!(exts.contains(&"js"));
        assert!(exts.contains(&"jsx"));
        // Rust
        assert!(exts.contains(&"rs"));
        // Python
        assert!(exts.contains(&"py"));
        // Go
        assert!(exts.contains(&"go"));
    }

    #[test]
    fn registry_is_thread_safe() {
        use std::sync::Arc;
        use std::thread;

        let registry = Arc::new(ParserRegistry::new());
        let r1 = Arc::clone(&registry);
        let r2 = Arc::clone(&registry);

        let t1 = thread::spawn(move || {
            let parser = r1.parser_for_file(Path::new("foo.rs"));
            assert!(parser.is_some());
            assert_eq!(parser.unwrap().language(), Language::Rust);
        });

        let t2 = thread::spawn(move || {
            let parser = r2.parser_for_file(Path::new("foo.py"));
            assert!(parser.is_some());
            assert_eq!(parser.unwrap().language(), Language::Python);
        });

        t1.join().expect("thread 1 panicked");
        t2.join().expect("thread 2 panicked");
    }

    #[test]
    fn ac51_parse_from_multiple_threads_all_languages() {
        use std::sync::Arc;
        use std::thread;

        let registry = Arc::new(ParserRegistry::new());

        let sources: Vec<(&str, &str)> = vec![
            ("test.rs", "fn hello() {} struct Foo {}"),
            ("test.py", "def hello(): pass\nclass Foo: pass"),
            ("test.go", "package main\nfunc Hello() {}"),
        ];

        let handles: Vec<_> = sources
            .into_iter()
            .map(|(filename, source)| {
                let reg = Arc::clone(&registry);
                let filename = filename.to_string();
                let source = source.to_string();
                thread::spawn(move || {
                    let parser = reg
                        .parser_for_file(Path::new(&filename))
                        .expect("parser should exist");
                    let result = parser.parse(source.as_bytes(), Path::new(&filename));
                    assert!(result.is_ok(), "parse failed for {filename}");
                    let pr = result.unwrap();
                    assert!(!pr.symbols.is_empty(), "expected symbols from {filename}");
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }
    }
}
