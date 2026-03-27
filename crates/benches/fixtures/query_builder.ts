import { Knex } from 'knex';
import { z } from 'zod';

export interface FilterClause {
  field: string;
  op: 'eq' | 'neq' | 'lt' | 'lte' | 'gt' | 'gte' | 'like' | 'in';
  value: unknown;
}

export interface QueryOptions {
  filters?: FilterClause[];
  orderBy?: { field: string; dir: 'asc' | 'desc' }[];
  limit?: number;
  offset?: number;
}

const FilterSchema = z.object({
  field: z.string(),
  op: z.enum(['eq', 'neq', 'lt', 'lte', 'gt', 'gte', 'like', 'in']),
  value: z.unknown(),
});

export class QueryBuilder {
  constructor(private readonly db: Knex) {}

  build(table: string, opts: QueryOptions): Knex.QueryBuilder {
    let qb = this.db(table);
    for (const f of opts.filters ?? []) {
      const parsed = FilterSchema.parse(f);
      if (parsed.op === 'eq') qb = qb.where(parsed.field, parsed.value);
      else if (parsed.op === 'like') qb = qb.whereLike(parsed.field, parsed.value as string);
      else if (parsed.op === 'in') qb = qb.whereIn(parsed.field, parsed.value as unknown[]);
      else qb = qb.where(parsed.field, parsed.op, parsed.value);
    }
    for (const o of opts.orderBy ?? []) {
      qb = qb.orderBy(o.field, o.dir);
    }
    if (opts.limit != null) qb = qb.limit(opts.limit);
    if (opts.offset != null) qb = qb.offset(opts.offset);
    return qb;
  }

  async count(table: string, filters: FilterClause[]): Promise<number> {
    const result = await this.build(table, { filters }).count('* as cnt').first();
    return Number((result as { cnt: string }).cnt);
  }
}
