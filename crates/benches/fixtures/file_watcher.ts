import * as chokidar from 'chokidar';
import * as path from 'path';
import { EventEmitter } from 'events';
import debounce from 'lodash/debounce';

export type ChangeKind = 'add' | 'change' | 'unlink';

export interface FileEvent {
  kind: ChangeKind;
  filePath: string;
  timestamp: number;
}

export class FileWatcher extends EventEmitter {
  private watcher: chokidar.FSWatcher | null = null;
  private readonly debounceMs: number;

  constructor(debounceMs = 200) {
    super();
    this.debounceMs = debounceMs;
  }

  start(dirs: string[], globs: string[] = ['**/*']): void {
    const patterns = globs.map((g) => dirs.map((d) => path.join(d, g))).flat();
    this.watcher = chokidar.watch(patterns, { ignoreInitial: true, persistent: true });

    const emit = debounce((event: FileEvent) => this.emit('change', event), this.debounceMs);

    this.watcher.on('add', (p) => emit({ kind: 'add', filePath: p, timestamp: Date.now() }));
    this.watcher.on('change', (p) => emit({ kind: 'change', filePath: p, timestamp: Date.now() }));
    this.watcher.on('unlink', (p) => emit({ kind: 'unlink', filePath: p, timestamp: Date.now() }));
  }

  async stop(): Promise<void> {
    await this.watcher?.close();
    this.watcher = null;
  }

  isRunning(): boolean {
    return this.watcher !== null;
  }
}
