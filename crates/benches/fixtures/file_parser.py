import ast
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Union


@dataclass
class FunctionDef:
    name: str
    line: int
    args: list[str]
    is_async: bool = False
    decorators: list[str] = None

    def __post_init__(self):
        if self.decorators is None:
            self.decorators = []


@dataclass
class ClassDef:
    name: str
    line: int
    bases: list[str]
    methods: list[FunctionDef]


@dataclass
class ImportDef:
    module: str
    names: list[str]
    is_from: bool
    line: int


ParsedItem = Union[FunctionDef, ClassDef, ImportDef]


def parse_python_file(path: Path) -> list[ParsedItem]:
    source = path.read_text(encoding="utf-8")
    tree = ast.parse(source, filename=str(path))
    results: list[ParsedItem] = []

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                results.append(ImportDef(module=alias.name, names=[alias.asname or alias.name], is_from=False, line=node.lineno))
        elif isinstance(node, ast.ImportFrom):
            module = node.module or ""
            names = [a.name for a in node.names]
            results.append(ImportDef(module=module, names=names, is_from=True, line=node.lineno))
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            args = [a.arg for a in node.args.args]
            decs = [ast.unparse(d) for d in node.decorator_list]
            results.append(FunctionDef(name=node.name, line=node.lineno, args=args, is_async=isinstance(node, ast.AsyncFunctionDef), decorators=decs))
        elif isinstance(node, ast.ClassDef):
            bases = [ast.unparse(b) for b in node.bases]
            methods = [FunctionDef(name=m.name, line=m.lineno, args=[a.arg for a in m.args.args]) for m in node.body if isinstance(m, (ast.FunctionDef, ast.AsyncFunctionDef))]
            results.append(ClassDef(name=node.name, line=node.lineno, bases=bases, methods=methods))

    return results


def extract_docstring(node: ast.FunctionDef) -> str:
    if node.body and isinstance(node.body[0], ast.Expr) and isinstance(node.body[0].value, ast.Constant):
        return str(node.body[0].value.value)
    return ""
