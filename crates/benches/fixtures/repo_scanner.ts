import * as path from 'path';
import * as fs from 'fs/promises';
import { glob } from 'glob';
import ignore from 'ignore';

export interface ScanResult {
  filePath: string;
  sizeBytes: number;
  ext: string;
  lines: number;
}

export interface ScanOptions {
  extensions: string[];
  maxFileSizeBytes?: number;
  respectGitignore?: boolean;
}

export class RepoScanner {
  private ig = ignore();

  constructor(private readonly root: string) {}

  async loadGitignore(): Promise<void> {
    const gitignorePath = path.join(this.root, '.gitignore');
    try {
      const content = await fs.readFile(gitignorePath, 'utf-8');
      this.ig.add(content);
    } catch { /* no .gitignore */ }
  }

  async scan(opts: ScanOptions): Promise<ScanResult[]> {
    const patterns = opts.extensions.map((ext) => `**/*.${ext}`);
    const files = await glob(patterns, { cwd: this.root, absolute: true, dot: false });
    const results: ScanResult[] = [];
    for (const file of files) {
      const rel = path.relative(this.root, file);
      if (opts.respectGitignore && this.ig.ignores(rel)) continue;
      const stat = await fs.stat(file);
      if (opts.maxFileSizeBytes && stat.size > opts.maxFileSizeBytes) continue;
      const content = await fs.readFile(file, 'utf-8');
      results.push({ filePath: file, sizeBytes: stat.size, ext: path.extname(file).slice(1), lines: content.split('\n').length });
    }
    return results;
  }

  async countByExt(): Promise<Record<string, number>> {
    const all = await this.scan({ extensions: ['ts', 'js', 'py', 'rs', 'go'] });
    const counts: Record<string, number> = {};
    for (const f of all) counts[f.ext] = (counts[f.ext] ?? 0) + 1;
    return counts;
  }
}
