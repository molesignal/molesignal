import type { QueryResult } from '@/types/query';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

import { INSTANCE_LABELS, type RuntimeCandidate } from './families';

/** Labels an instance may be known by besides those APM tells instances apart by. */
const INSTANCE_FALLBACKS = ['node.id'];

/** A label value as PromQL writes it between quotes. */
function quoted(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

/** One instance of the service: the label it is told apart by, and its value there. */
export interface RuntimeInstance {
  label: string;
  value: string;
}

/**
 * The candidate's statement for one service, and for one of its instances when
 * one is chosen: each `{{metric}}` becomes the metric with the service's
 * matchers (and its own, after a comma), `BY` the labels instances are told
 * apart by.
 */
export function runtimeStatement(candidate: RuntimeCandidate, service: { name: string; namespace?: string }, instance?: RuntimeInstance | null): string {
  const matchers = [`"service.name"=${quoted(service.name)}`];
  if (service.namespace) matchers.push(`"service.namespace"=${quoted(service.namespace)}`);
  if (instance) matchers.push(`${quoted(instance.label)}=${quoted(instance.value)}`);

  // The labels first: the matchers written after carry the service's own words.
  return candidate.statement
    .replaceAll('BY', `(${INSTANCE_LABELS.map(quoted).join(',')})`)
    .replace(/\{\{([A-Za-z_:][A-Za-z0-9_:]*)(?:,([^}]*))?\}\}/g, (_, metric: string, own: string | undefined) => `${metric}{${[...matchers, ...(own ? [own] : [])].join(',')}}`);
}

/** A PromQL answer as one line per label set, named by the instance where it says one. */
export function seriesOfResult(result: QueryResult, unit: string): TimeSeriesSeries[] {
  const time = result.columns.indexOf('_timestamp');
  const value = result.columns.indexOf('value');
  const labels = result.columns.map((name, index) => ({ name, index })).filter(({ index }) => index !== time && index !== value);
  const groups = new Map<
    string,
    {
      labels: Record<string, string>;
      timestamps: number[];
      data: Array<number | null>;
    }
  >();

  for (const row of result.rows) {
    const set: Record<string, string> = {};
    for (const label of labels) {
      const cell = row[label.index];
      if (cell !== null && cell !== undefined && cell !== '') set[label.name] = String(cell);
    }

    const key = JSON.stringify(set);
    const group = groups.get(key) ?? { labels: set, timestamps: [], data: [] };
    group.timestamps.push(Number(row[time]));
    const number = Number(row[value]);
    group.data.push(Number.isFinite(number) ? number : null);
    groups.set(key, group);
  }

  return [...groups.values()].map((group, index) => ({
    id: `series-${index}`,
    name: instanceOf(group.labels) ?? '—',
    labels: group.labels,
    timestamps: group.timestamps,
    data: group.data,
    unit,
  }));
}

/** Which instance a label set is of, if it says. */
export function instanceOf(labels: Readonly<Record<string, string>>): string | null {
  for (const name of [...INSTANCE_LABELS, ...INSTANCE_FALLBACKS]) {
    if (labels[name]) return labels[name];
  }
  return null;
}
