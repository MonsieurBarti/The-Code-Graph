import { BinaryHeap } from 'mnemonist';
import levenshtein from 'fast-levenshtein';
import * as path from 'path';

export interface Symbol {
  name: string;
  kind: 'function' | 'class' | 'interface' | 'enum' | 'type' | 'variable';
  filePath: string;
  line: number;
  column: number;
}

export class SymbolIndex {
  private symbols: Symbol[] = [];
  private nameIndex = new Map<string, Symbol[]>();

  add(symbol: Symbol): void {
    this.symbols.push(symbol);
    const list = this.nameIndex.get(symbol.name) ?? [];
    list.push(symbol);
    this.nameIndex.set(symbol.name, list);
  }

  findExact(name: string): Symbol[] {
    return this.nameIndex.get(name) ?? [];
  }

  fuzzySearch(query: string, maxDistance = 2): Symbol[] {
    const results: Array<{ dist: number; sym: Symbol }> = [];
    for (const [name, syms] of this.nameIndex) {
      const dist = levenshtein.get(query, name);
      if (dist <= maxDistance) {
        for (const sym of syms) results.push({ dist, sym });
      }
    }
    results.sort((a, b) => a.dist - b.dist);
    return results.map((r) => r.sym);
  }

  byFile(filePath: string): Symbol[] {
    const abs = path.resolve(filePath);
    return this.symbols.filter((s) => path.resolve(s.filePath) === abs);
  }

  size(): number {
    return this.symbols.length;
  }

  clear(): void {
    this.symbols = [];
    this.nameIndex.clear();
  }
}
