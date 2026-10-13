import type { TFunction } from 'i18next';

import type { LogEntry } from '../viewModel';

/** What an inspection is of: a stream, what was asked of it, and when. */
export interface InspectionTarget {
  stream: string;
  /** The query as the reader wrote it; empty for every log of the stream. */
  statement: string;
  /** Microseconds since the epoch. */
  range: { start: number; end: number };
}

/** What the loaded logs say at a glance, for the model to begin from. */
export interface LogDigest {
  loaded: number;
  levels: Array<[level: string, count: number]>;
  services: Array<[service: string, count: number]>;
  /** The commonest messages of the logs that are not routine, each once. */
  notable: Array<{ level: string; message: string; count: number }>;
}

const SERVICE_FIELDS = ['service.name', 'service', 'service_name'];
const MESSAGE_FIELDS = ['message', 'body', 'msg'];
const ROUTINE_LEVELS = new Set(['INFO', 'DEBUG', 'TRACE']);
/** As many of each as a prompt has room for. */
const TOP = 8;
const MESSAGE_LENGTH = 240;

function counted(values: Iterable<string>): Array<[string, number]> {
  const counts = new Map<string, number>();

  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);

  return [...counts.entries()].sort((left, right) => right[1] - left[1]);
}

const firstText = (record: Record<string, unknown>, fields: readonly string[]): string => {
  const value = fields.map((field) => record[field]).find((candidate) => typeof candidate === 'string' && candidate !== '');

  return typeof value === 'string' ? value : '';
};

/**
 * What the loaded page says, in a few lines: how the logs divide by level and
 * by service, and the commonest messages of those that are not routine. It is
 * a place to begin from, not the evidence: the model reads the stream itself.
 */
export function digestLoadedLogs(rows: readonly LogEntry[]): LogDigest {
  const notable = new Map<string, { level: string; message: string; count: number }>();

  for (const row of rows) {
    if (ROUTINE_LEVELS.has(row.level)) continue;

    const message = firstText(row.raw, MESSAGE_FIELDS).slice(0, MESSAGE_LENGTH);
    if (!message) continue;

    const key = `${row.level}\u0000${message}`;
    const held = notable.get(key);

    if (held) held.count += 1;
    else notable.set(key, { level: row.level, message, count: 1 });
  }

  return {
    loaded: rows.length,
    levels: counted(rows.map((row) => row.level)).slice(0, TOP),
    services: counted(rows.map((row) => firstText(row.raw, SERVICE_FIELDS)).filter(Boolean)).slice(0, TOP),
    notable: [...notable.values()].sort((left, right) => right.count - left.count).slice(0, TOP),
  };
}

const listed = (pairs: ReadonlyArray<readonly [string, number]>) => pairs.map(([name, count]) => `${name} ${count}`).join(', ');

interface InspectionPromptInput {
  target: InspectionTarget;
  /** The range as the reader sees it: "Last 1 hour", or two moments. */
  rangeLabel: string;
  /** What the reader asked the inspection to attend to; may be empty. */
  requirements: string;
  digest: LogDigest;
}

/**
 * What the model is asked: to inspect the logs the query matches, reading them
 * with its own tools, and to answer in the reader's language. The words are
 * the product's (`logs:explore.inspection.prompt`), so a report reads in the
 * language the page is in.
 */
export function buildInspectionPrompt(
  t: TFunction<'logs'>,
  { target, rangeLabel, requirements, digest }: InspectionPromptInput,
): string {
  const lines = [
    t('explore.inspection.prompt.task', { stream: target.stream }),
    t('explore.inspection.prompt.range', { label: rangeLabel, start: target.range.start, end: target.range.end }),
    target.statement.trim()
      ? t('explore.inspection.prompt.query', { statement: target.statement.trim() })
      : t('explore.inspection.prompt.query_all'),
    '',
    t('explore.inspection.prompt.steps'),
    '',
    t('explore.inspection.prompt.rules'),
  ];

  if (digest.loaded > 0) {
    lines.push(
      '',
      t('explore.inspection.prompt.digest', { count: digest.loaded }),
      ...(digest.levels.length > 0 ? [`- ${t('explore.inspection.prompt.digest_levels')}: ${listed(digest.levels)}`] : []),
      ...(digest.services.length > 0 ? [`- ${t('explore.inspection.prompt.digest_services')}: ${listed(digest.services)}`] : []),
      ...digest.notable.map((item) => `- [${item.level} ×${item.count}] ${item.message}`),
    );
  }
  if (requirements.trim()) {
    lines.push('', t('explore.inspection.prompt.requirements'), requirements.trim());
  }

  return lines.join('\n');
}
