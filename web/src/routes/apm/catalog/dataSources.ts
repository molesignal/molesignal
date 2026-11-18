import type { ServiceDataSources, ServiceLogSource, ServiceMetricSource } from '@/api/apmCatalog';
import { escapeSqlIdentifier } from '@/routes/logs/fieldQueryModel';

/** What the backend keeps of a service's other names and data sources. */
export const MAX_ALIASES = 20;
export const MAX_ALIAS_LENGTH = 128;
export const MAX_SOURCES_PER_KIND = 10;
export const MAX_FILTER_LENGTH = 2_000;
export const MAX_SELECTOR_LENGTH = 1_000;
export const MAX_LINK_NAME_LENGTH = 120;
export const MAX_LINK_URL_LENGTH = 2_048;

/** How many logs a source opens in the explorer, as the explorer's own default. */
const LOG_LIMIT = 200;

/** Other names as people type them: a comma between each, a Chinese comma or 、 too. */
const ALIAS_SEPARATORS = /[,，、]/;

/** Other names as the field shows them. */
export function formatAliases(aliases: readonly string[] | undefined): string {
  return (aliases ?? []).join(', ');
}

export type AliasesProblem = 'length' | 'own_name' | 'twice' | 'too_many';

/**
 * The other names a typed line holds, or what is wrong with it as the
 * backend would say: a name too long, the service's own name, a name given
 * twice (case ignored), too many. Spaces around names are dropped.
 */
export function parseAliases(text: string, ownName: string): { aliases: string[]; problem: AliasesProblem | null; at?: string } {
  const aliases: string[] = [];
  const seen = new Set<string>();
  const own = ownName.trim().toLowerCase();

  for (const part of text.split(ALIAS_SEPARATORS)) {
    const alias = part.trim();
    if (!alias) continue;

    const folded = alias.toLowerCase();
    if ([...alias].length > MAX_ALIAS_LENGTH) return { aliases, problem: 'length', at: alias };
    if (own && folded === own) return { aliases, problem: 'own_name', at: alias };
    if (seen.has(folded)) return { aliases, problem: 'twice', at: alias };

    seen.add(folded);
    aliases.push(alias);
  }

  if (aliases.length > MAX_ALIASES) return { aliases, problem: 'too_many' };
  return { aliases, problem: null };
}

/** The data sources as the form edits them: every entry a row, every value a string. */
export interface SourceRows {
  logs: Array<{ stream: string; filter: string }>;
  metrics: string[];
  dashboards: string[];
  links: Array<{ name: string; url: string }>;
}

export type SourceKind = keyof SourceRows;

export function sourceRows(sources: ServiceDataSources | undefined): SourceRows {
  return {
    logs: (sources?.logs ?? []).map((source) => ({ stream: source.stream, filter: source.filter ?? '' })),
    metrics: (sources?.metrics ?? []).map((source) => source.selector),
    dashboards: [...(sources?.dashboards ?? [])],
    links: (sources?.links ?? []).map((link) => ({ name: link.name, url: link.url })),
  };
}

export type SourceProblemKind = 'too_many' | 'stream' | 'filter' | 'selector' | 'link_name' | 'link_url' | 'dashboard_twice';

export interface SourceProblem {
  kind: SourceKind;
  /** The row that is wrong; none when the whole kind is. */
  index?: number;
  problem: SourceProblemKind;
}

/** A selector as the backend takes one: a metric name, `{…}` of labels, or both, one pair of braces at most, closing it last. */
export function isSelector(text: string): boolean {
  const selector = text.trim();
  const opened = selector.split('{').length - 1;
  const closed = selector.split('}').length - 1;

  return (
    selector.length > 0 &&
    [...selector].length <= MAX_SELECTOR_LENGTH &&
    !/[\n\r]/.test(selector) &&
    opened === closed &&
    opened <= 1 &&
    (opened === 0 || selector.endsWith('}'))
  );
}

/** An http or https address without spaces, as a link is kept. */
export function isLinkAddress(text: string): boolean {
  const url = text.trim();
  return url.length <= MAX_LINK_URL_LENGTH && /^https?:\/\/[^\s/][^\s]*$/i.test(url);
}

/** The first thing the backend would refuse in the rows, or `null`. Rows left empty are dropped, not wrong. */
export function checkSources(rows: SourceRows): SourceProblem | null {
  const kept = sourcesWrite(rows);

  for (const kind of ['logs', 'metrics', 'dashboards', 'links'] as const) {
    if ((kept[kind]?.length ?? 0) > MAX_SOURCES_PER_KIND) return { kind, problem: 'too_many' };
  }

  for (const [index, row] of rows.logs.entries()) {
    const stream = row.stream.trim();
    const filter = row.filter.trim();
    if (!stream && !filter) continue;
    if (!stream) return { kind: 'logs', index, problem: 'stream' };
    if ([...filter].length > MAX_FILTER_LENGTH) return { kind: 'logs', index, problem: 'filter' };
  }

  for (const [index, row] of rows.metrics.entries()) {
    if (row.trim() && !isSelector(row)) return { kind: 'metrics', index, problem: 'selector' };
  }

  const dashboards = new Set<string>();

  for (const [index, row] of rows.dashboards.entries()) {
    const id = row.trim();
    if (!id) continue;
    if (dashboards.has(id)) return { kind: 'dashboards', index, problem: 'dashboard_twice' };
    dashboards.add(id);
  }

  for (const [index, row] of rows.links.entries()) {
    const name = row.name.trim();
    const url = row.url.trim();
    if (!name && !url) continue;
    if (!name || [...name].length > MAX_LINK_NAME_LENGTH) return { kind: 'links', index, problem: 'link_name' };
    if (!isLinkAddress(url)) return { kind: 'links', index, problem: 'link_url' };
  }

  return null;
}

/** What a save writes: every text without the spaces around it, rows left empty dropped, an empty filter none. */
export function sourcesWrite(rows: SourceRows): ServiceDataSources {
  return {
    logs: rows.logs
      .map((row) => ({ stream: row.stream.trim(), filter: row.filter.trim() || null }))
      .filter((row) => row.stream || row.filter),
    metrics: rows.metrics.map((selector) => ({ selector: selector.trim() })).filter((row) => row.selector),
    dashboards: rows.dashboards.map((id) => id.trim()).filter(Boolean),
    links: rows.links.map((link) => ({ name: link.name.trim(), url: link.url.trim() })).filter((link) => link.name || link.url),
  };
}

/** Whether a service lists anything about where its data is. */
export function hasSources(sources: ServiceDataSources | undefined): boolean {
  return Boolean(sources?.logs?.length || sources?.metrics?.length || sources?.dashboards?.length || sources?.links?.length);
}

/** The logs of a source in the explorer: its stream, narrowed by its filter as SQL. */
export function logSourceHref(source: ServiceLogSource): string {
  const params = new URLSearchParams({ stream: source.stream });
  const filter = source.filter?.trim();

  if (filter) {
    params.set('sql', `SELECT * FROM "${escapeSqlIdentifier(source.stream)}"\nWHERE ${filter}\nORDER BY _timestamp DESC\nLIMIT ${LOG_LIMIT}`);
  }

  return `/logs?${params}`;
}

/** The metrics of a selector in the metrics explorer. */
export function metricSourceHref(source: ServiceMetricSource): string {
  return `/metrics?${new URLSearchParams({ promql: source.selector })}`;
}

export function dashboardHref(id: string): string {
  return `/dashboards/${encodeURIComponent(id)}`;
}
