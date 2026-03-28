import * as ts from 'typescript';
import * as path from 'path';
import * as fs from 'fs';

export interface ImportDeclaration {
  moduleSpecifier: string;
  names: string[];
  isTypeOnly: boolean;
  line: number;
}

export function parseImports(filePath: string): ImportDeclaration[] {
  const source = fs.readFileSync(filePath, 'utf-8');
  const sourceFile = ts.createSourceFile(filePath, source, ts.ScriptTarget.Latest, true);
  const imports: ImportDeclaration[] = [];

  function visit(node: ts.Node): void {
    if (ts.isImportDeclaration(node)) {
      const moduleSpecifier = (node.moduleSpecifier as ts.StringLiteral).text;
      const names: string[] = [];
      const namedBindings = node.importClause?.namedBindings;
      if (namedBindings && ts.isNamedImports(namedBindings)) {
        namedBindings.elements.forEach((el) => names.push(el.name.text));
      } else if (node.importClause?.name) {
        names.push(node.importClause.name.text);
      }
      const line = sourceFile.getLineAndCharacterOfPosition(node.getStart()).line + 1;
      imports.push({ moduleSpecifier, names, isTypeOnly: node.importClause?.isTypeOnly ?? false, line });
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return imports;
}

export function resolveModulePath(specifier: string, fromFile: string): string | null {
  if (specifier.startsWith('.')) {
    return path.resolve(path.dirname(fromFile), specifier);
  }
  return null;
}
