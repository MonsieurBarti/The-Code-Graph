import * as path from 'path';
import * as fs from 'fs';
import { createHash } from 'crypto';
import { promisify } from 'util';

const readdir = promisify(fs.readdir);
const stat = promisify(fs.stat);

export interface Dependency {
  name: string;
  version: string;
  resolved: string;
  integrity: string;
}

export interface LockFile {
  version: number;
  dependencies: Record<string, Dependency>;
}

export class DependencyResolver {
  private cache = new Map<string, Dependency>();

  constructor(private readonly rootDir: string) {}

  async resolve(packageName: string): Promise<Dependency | null> {
    if (this.cache.has(packageName)) {
      return this.cache.get(packageName)!;
    }
    const lockPath = path.join(this.rootDir, 'package-lock.json');
    if (!fs.existsSync(lockPath)) return null;
    const lock: LockFile = JSON.parse(fs.readFileSync(lockPath, 'utf-8'));
    const dep = lock.dependencies[packageName] ?? null;
    if (dep) this.cache.set(packageName, dep);
    return dep;
  }

  async listAll(): Promise<string[]> {
    const entries = await readdir(path.join(this.rootDir, 'node_modules'));
    const results: string[] = [];
    for (const entry of entries) {
      const info = await stat(path.join(this.rootDir, 'node_modules', entry));
      if (info.isDirectory()) results.push(entry);
    }
    return results;
  }

  hashContent(content: string): string {
    return createHash('sha256').update(content).digest('hex');
  }

  clearCache(): void {
    this.cache.clear();
  }
}
