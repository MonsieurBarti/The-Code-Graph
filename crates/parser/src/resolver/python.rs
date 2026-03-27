use std::path::{Path, PathBuf};

use domain::model::{Edge, EdgeKind, Language};

use crate::ParseResult;
use super::{ImportResolver, ResolveContext};

// ---------------------------------------------------------------------------
// Stdlib module set
// ---------------------------------------------------------------------------

static STDLIB_MODULES: &[&str] = &[
    "abc", "aifc", "argparse", "array", "ast", "asynchat", "asyncio", "asyncore",
    "atexit", "audioop", "base64", "bdb", "binascii", "binhex", "bisect",
    "builtins", "bz2", "calendar", "cgi", "cgitb", "chunk", "cmath", "cmd",
    "code", "codecs", "codeop", "collections", "colorsys", "compileall",
    "concurrent", "configparser", "contextlib", "contextvars", "copy", "copyreg",
    "cProfile", "crypt", "csv", "ctypes", "curses", "dataclasses", "datetime",
    "dbm", "decimal", "difflib", "dis", "distutils", "doctest", "email",
    "encodings", "enum", "errno", "faulthandler", "fcntl", "filecmp", "fileinput",
    "fnmatch", "formatter", "fractions", "ftplib", "functools", "gc", "getopt",
    "getpass", "gettext", "glob", "grp", "gzip", "hashlib", "heapq", "hmac",
    "html", "http", "idlelib", "imaplib", "imghdr", "imp", "importlib",
    "inspect", "io", "ipaddress", "itertools", "json", "keyword", "lib2to3",
    "linecache", "locale", "logging", "lzma", "mailbox", "mailcap", "marshal",
    "math", "mimetypes", "mmap", "modulefinder", "multiprocessing", "netrc",
    "nis", "nntplib", "numbers", "operator", "optparse", "os", "ossaudiodev",
    "parser", "pathlib", "pdb", "pickle", "pickletools", "pipes", "pkgutil",
    "platform", "plistlib", "poplib", "posix", "posixpath", "pprint",
    "profile", "pstats", "pty", "pwd", "py_compile", "pyclbr", "pydoc",
    "queue", "quopri", "random", "re", "readline", "reprlib", "resource",
    "rlcompleter", "runpy", "sched", "secrets", "select", "selectors",
    "shelve", "shlex", "shutil", "signal", "site", "smtpd", "smtplib",
    "sndhdr", "socket", "socketserver", "spwd", "sqlite3", "sre_compile",
    "sre_constants", "sre_parse", "ssl", "stat", "statistics", "string",
    "stringprep", "struct", "subprocess", "sunau", "symtable", "sys",
    "sysconfig", "syslog", "tabnanny", "tarfile", "telnetlib", "tempfile",
    "termios", "test", "textwrap", "threading", "time", "timeit", "tkinter",
    "token", "tokenize", "tomllib", "trace", "traceback", "tracemalloc",
    "tty", "turtle", "turtledemo", "types", "typing", "unicodedata",
    "unittest", "urllib", "uu", "uuid", "venv", "warnings", "wave",
    "weakref", "webbrowser", "winreg", "winsound", "wsgiref", "xdrlib",
    "xml", "xmlrpc", "zipapp", "zipfile", "zipimport", "zlib",
    // Common underscore-prefixed internals
    "_thread", "__future__", "_abc", "_collections_abc",
];

fn is_stdlib(first_segment: &str) -> bool {
    STDLIB_MODULES.contains(&first_segment)
}

// ---------------------------------------------------------------------------
// Resolution helpers
// ---------------------------------------------------------------------------

/// Try to resolve a candidate path against the file_tree.
/// Checks `{path}.py` first, then `{path}/__init__.py`.
fn try_resolve(candidate: &Path, file_tree: &[PathBuf]) -> Option<PathBuf> {
    let py_path = candidate.with_extension("py");
    if file_tree.contains(&py_path) {
        return Some(py_path);
    }
    let init_path = candidate.join("__init__.py");
    if file_tree.contains(&init_path) {
        return Some(init_path);
    }
    None
}

/// Resolve a Python import specifier to a path in the file_tree.
fn resolve_python_import(
    specifier: &str,
    current_file: &Path,
    project_root: &Path,
    file_tree: &[PathBuf],
) -> Option<PathBuf> {
    // Relative import — starts with one or more dots
    if specifier.starts_with('.') {
        let dot_count = specifier.chars().take_while(|c| *c == '.').count();
        let module_path = &specifier[dot_count..];

        let mut base_dir = current_file.parent().unwrap_or(current_file).to_path_buf();
        for _ in 1..dot_count {
            base_dir = base_dir.parent().unwrap_or(&base_dir).to_path_buf();
        }

        let candidate = if module_path.is_empty() {
            base_dir
        } else {
            let rel: PathBuf = module_path.replace('.', "/").into();
            base_dir.join(rel)
        };

        return try_resolve(&candidate, file_tree);
    }

    // Absolute import — check stdlib first segment
    let first_segment = specifier.split('.').next().unwrap_or(specifier);
    if is_stdlib(first_segment) {
        return None;
    }

    // Local absolute import
    let rel: PathBuf = specifier.replace('.', "/").into();
    let candidate = project_root.join(rel);
    try_resolve(&candidate, file_tree)
}

// ---------------------------------------------------------------------------
// PythonResolver
// ---------------------------------------------------------------------------

/// Python import resolver — filesystem prober + stdlib detection.
pub struct PythonResolver;

impl ImportResolver for PythonResolver {
    fn languages(&self) -> &[Language] {
        &[Language::Python]
    }

    fn resolve(
        &self,
        file_path: &Path,
        parse_result: &ParseResult,
        context: &ResolveContext,
    ) -> domain::error::Result<Vec<Edge>> {
        let source = file_path.to_string_lossy().into_owned();
        let mut edges = Vec::new();

        for import in &parse_result.imports {
            let resolved = resolve_python_import(
                &import.specifier,
                file_path,
                &context.project_root,
                &context.file_tree,
            );

            if let Some(target_path) = resolved {
                let target = target_path.to_string_lossy().into_owned();
                let kind = if import.is_type_only {
                    EdgeKind::ConditionalImport
                } else {
                    EdgeKind::ImportsFrom
                };
                edges.push(Edge {
                    kind,
                    source: source.clone(),
                    target,
                    metadata: None,
                });
            }
        }

        Ok(edges)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use domain::model::EdgeKind;

    use super::PythonResolver;
    use crate::resolver::{ImportResolver, ResolveContext};
    use crate::{ImportName, ParseResult, RawImport};

    fn make_context(project_root: &str, file_tree: Vec<&str>) -> ResolveContext {
        ResolveContext {
            project_root: PathBuf::from(project_root),
            parsed_files: HashMap::new(),
            file_tree: file_tree.into_iter().map(PathBuf::from).collect(),
        }
    }

    // AC40: Resolves `from .models import User` to sibling models.py
    #[test]
    fn resolves_relative_import_single_dot() {
        let context = make_context(
            "/project",
            vec![
                "/project/app/models.py",
                "/project/app/views.py",
            ],
        );
        let parse_result = ParseResult {
            imports: vec![RawImport {
                specifier: ".models".into(),
                names: vec![ImportName { name: "User".into(), alias: None, is_type: false }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/app/views.py"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::ImportsFrom);
        assert_eq!(edges[0].source, "/project/app/views.py");
        assert_eq!(edges[0].target, "/project/app/models.py");
    }

    // AC41: Resolves `from ..utils import helper` by walking up directories
    #[test]
    fn resolves_relative_import_double_dot() {
        let context = make_context(
            "/project",
            vec![
                "/project/utils.py",
                "/project/app/views.py",
            ],
        );
        let parse_result = ParseResult {
            imports: vec![RawImport {
                specifier: "..utils".into(),
                names: vec![ImportName { name: "helper".into(), alias: None, is_type: false }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/app/views.py"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::ImportsFrom);
        assert_eq!(edges[0].source, "/project/app/views.py");
        assert_eq!(edges[0].target, "/project/utils.py");
    }

    // AC42: Skips stdlib imports (import os) — no edge
    #[test]
    fn skips_stdlib_import() {
        let context = make_context("/project", vec![]);
        let parse_result = ParseResult {
            imports: vec![RawImport {
                specifier: "os".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/main.py"), &parse_result, &context)
            .unwrap();
        assert!(edges.is_empty(), "stdlib import should produce no edge");
    }

    // AC42: Skips stdlib submodule imports (e.g. os.path)
    #[test]
    fn skips_stdlib_submodule_import() {
        let context = make_context("/project", vec![]);
        let parse_result = ParseResult {
            imports: vec![RawImport {
                specifier: "os.path".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/main.py"), &parse_result, &context)
            .unwrap();
        assert!(edges.is_empty(), "stdlib submodule import should produce no edge");
    }

    // AC43: Creates ConditionalImport edge for TYPE_CHECKING imports
    #[test]
    fn creates_conditional_import_for_type_checking() {
        let context = make_context(
            "/project",
            vec!["/project/app/models.py", "/project/app/views.py"],
        );
        let parse_result = ParseResult {
            imports: vec![RawImport {
                specifier: ".models".into(),
                names: vec![ImportName { name: "User".into(), alias: None, is_type: false }],
                is_type_only: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/app/views.py"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::ConditionalImport);
    }

    // Absolute local import resolution
    #[test]
    fn resolves_absolute_local_import() {
        let context = make_context(
            "/project",
            vec!["/project/utils/helpers.py"],
        );
        let parse_result = ParseResult {
            imports: vec![RawImport {
                specifier: "utils.helpers".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/main.py"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, EdgeKind::ImportsFrom);
        assert_eq!(edges[0].target, "/project/utils/helpers.py");
    }

    // Package import resolves to __init__.py
    #[test]
    fn resolves_package_import_to_init() {
        let context = make_context(
            "/project",
            vec!["/project/mypackage/__init__.py"],
        );
        let parse_result = ParseResult {
            imports: vec![RawImport {
                specifier: "mypackage".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/main.py"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target, "/project/mypackage/__init__.py");
    }

    // Unresolvable import produces no edge
    #[test]
    fn unresolvable_import_produces_no_edge() {
        let context = make_context("/project", vec![]);
        let parse_result = ParseResult {
            imports: vec![RawImport {
                specifier: "third_party_lib".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/main.py"), &parse_result, &context)
            .unwrap();
        assert!(edges.is_empty());
    }

    // Multiple imports in one parse result
    #[test]
    fn resolves_multiple_imports() {
        let context = make_context(
            "/project",
            vec![
                "/project/models.py",
                "/project/utils.py",
            ],
        );
        let parse_result = ParseResult {
            imports: vec![
                RawImport {
                    specifier: "models".into(),
                    ..Default::default()
                },
                RawImport {
                    specifier: "utils".into(),
                    ..Default::default()
                },
                RawImport {
                    specifier: "sys".into(), // stdlib — skipped
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let resolver = PythonResolver;
        let edges = resolver
            .resolve(Path::new("/project/main.py"), &parse_result, &context)
            .unwrap();
        assert_eq!(edges.len(), 2);
    }
}
