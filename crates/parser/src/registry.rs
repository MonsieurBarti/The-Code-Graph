use std::collections::HashMap;
use std::path::Path;

use domain::model::Language;

use crate::{JavaScriptParser, LanguageParser, TypeScriptParser};

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
    fn parser_for_rs_returns_none() {
        let registry = ParserRegistry::new();
        assert!(registry.parser_for_file(Path::new("foo.rs")).is_none());
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
    fn parser_for_language_rust_returns_none() {
        let registry = ParserRegistry::new();
        assert!(registry.parser_for_language(Language::Rust).is_none());
    }

    #[test]
    fn supported_extensions_contains_all_four() {
        let registry = ParserRegistry::new();
        let exts = registry.supported_extensions();
        assert!(exts.contains(&"ts"));
        assert!(exts.contains(&"tsx"));
        assert!(exts.contains(&"js"));
        assert!(exts.contains(&"jsx"));
    }
}
