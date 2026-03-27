use std::cell::RefCell;
use std::path::Path;

use tree_sitter::Parser;
use tree_sitter_language::LanguageFn;

use domain::error::CodeGraphError;
use domain::model::Language;

use crate::{LanguageParser, ParseResult};

thread_local! {
    static GO_PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

/// Parser for Go (.go) files.
pub struct GoParser {
    lang: LanguageFn,
}

impl GoParser {
    pub fn new() -> Self {
        Self {
            lang: tree_sitter_go::LANGUAGE,
        }
    }
}

impl Default for GoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for GoParser {
    fn language(&self) -> Language {
        Language::Go
    }

    fn file_extensions(&self) -> &[&str] {
        &["go"]
    }

    fn parse(&self, source: &[u8], path: &Path) -> domain::error::Result<ParseResult> {
        let lang: tree_sitter::Language = self.lang.into();

        GO_PARSER.with(|parser_cell| {
            let mut parser = parser_cell.borrow_mut();
            parser
                .set_language(&lang)
                .map_err(|e| CodeGraphError::Parse {
                    file: path.to_path_buf(),
                    message: format!("failed to set language: {e}"),
                })?;

            let tree = parser
                .parse(source, None)
                .ok_or_else(|| CodeGraphError::Parse {
                    file: path.to_path_buf(),
                    message: "tree-sitter parse returned None".into(),
                })?;

            extract_all(source, path, &tree)
        })
    }
}

fn extract_all(
    _source: &[u8],
    _path: &Path,
    _tree: &tree_sitter::Tree,
) -> domain::error::Result<ParseResult> {
    // Placeholder — T05 implements full extraction
    Ok(ParseResult::default())
}
