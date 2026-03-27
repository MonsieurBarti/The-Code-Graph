import { Transform, pipeline } from 'stream';
import { promisify } from 'util';
import * as zlib from 'zlib';
import * as fs from 'fs';

const pipelineAsync = promisify(pipeline);

export interface PipelineStage<I, O> {
  name: string;
  process(input: I): Promise<O>;
}

export class PipelineRunner<T> {
  private stages: PipelineStage<unknown, unknown>[] = [];

  addStage<I, O>(stage: PipelineStage<I, O>): this {
    this.stages.push(stage as PipelineStage<unknown, unknown>);
    return this;
  }

  async run(input: T): Promise<unknown> {
    let current: unknown = input;
    for (const stage of this.stages) {
      current = await stage.process(current);
    }
    return current;
  }

  async compressFile(src: string, dest: string): Promise<void> {
    await pipelineAsync(
      fs.createReadStream(src),
      zlib.createGzip(),
      fs.createWriteStream(dest),
    );
  }

  createBatchTransform(batchSize: number): Transform {
    let buffer: unknown[] = [];
    return new Transform({
      objectMode: true,
      transform(chunk, _enc, cb) {
        buffer.push(chunk);
        if (buffer.length >= batchSize) {
          this.push(buffer);
          buffer = [];
        }
        cb();
      },
      flush(cb) {
        if (buffer.length) this.push(buffer);
        cb();
      },
    });
  }
}
