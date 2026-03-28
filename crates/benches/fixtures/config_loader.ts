import * as fs from 'fs';
import * as path from 'path';
import * as yaml from 'js-yaml';
import { z } from 'zod';

const ServerSchema = z.object({
  host: z.string().default('localhost'),
  port: z.number().int().min(1).max(65535).default(8080),
  tls: z.boolean().default(false),
});

const DatabaseSchema = z.object({
  url: z.string().url(),
  maxConnections: z.number().int().positive().default(10),
  idleTimeoutMs: z.number().int().positive().default(30000),
});

const AppConfigSchema = z.object({
  server: ServerSchema,
  database: DatabaseSchema,
  logLevel: z.enum(['debug', 'info', 'warn', 'error']).default('info'),
  features: z.record(z.boolean()).default({}),
});

export type AppConfig = z.infer<typeof AppConfigSchema>;

export function loadConfig(configPath?: string): AppConfig {
  const filePath = configPath ?? path.join(process.cwd(), 'config.yaml');
  if (!fs.existsSync(filePath)) {
    throw new Error(`Config file not found: ${filePath}`);
  }
  const raw = yaml.load(fs.readFileSync(filePath, 'utf-8'));
  return AppConfigSchema.parse(raw);
}

export function mergeConfig(base: AppConfig, overrides: Partial<AppConfig>): AppConfig {
  return AppConfigSchema.parse({ ...base, ...overrides });
}

export function validateConfig(raw: unknown): AppConfig {
  return AppConfigSchema.parse(raw);
}
