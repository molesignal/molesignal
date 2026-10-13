import type { HomeHealthStatus, HomeOverview } from '@/api/home';

import type { Category, Signal, Source } from './sources';

export type PrimaryCategory =
  | 'recommended'
  | 'infrastructure'
  | 'cloud'
  | 'databases'
  | 'queues'
  | 'applications'
  | 'security'
  | 'custom';

export type SignalFilter = 'all' | Signal;

export const SIGNALS: readonly Signal[] = ['logs', 'metrics', 'traces', 'profiles'];

export const PRIMARY_CATEGORIES: readonly PrimaryCategory[] = [
  'recommended',
  'infrastructure',
  'cloud',
  'databases',
  'queues',
  'applications',
  'security',
  'custom',
];

const CLOUD_SOURCE_IDS = new Set(['aws', 'gcp', 'azure', 'cloudflare']);
const INFRASTRUCTURE_SOURCE_IDS = new Set(['kubernetes', 'linux', 'windows']);
const APPLICATION_SOURCE_IDS = new Set([
  'rum',
  'rum-flutter',
  'rum-android',
  'rum-ios',
  'continuous-profiling',
]);

export function primaryCategoryFromRoute(value: string | undefined): PrimaryCategory {
  if (value && PRIMARY_CATEGORIES.includes(value as PrimaryCategory)) {
    return value as PrimaryCategory;
  }
  const legacy: Partial<Record<Category, PrimaryCategory>> = {
    otel: 'applications',
    'otel-collector': 'applications',
    servers: 'infrastructure',
    databases: 'databases',
    security: 'security',
    devops: 'applications',
    networking: 'infrastructure',
    queues: 'queues',
    languages: 'applications',
    ai: 'applications',
    custom: 'custom',
    recommended: 'recommended',
  };
  return legacy[value as Category] ?? 'recommended';
}

export function sourceInPrimaryCategory(source: Source, category: PrimaryCategory): boolean {
  if (category === 'recommended') return source.category === 'recommended';
  if (category === 'cloud') return CLOUD_SOURCE_IDS.has(source.id);
  if (category === 'infrastructure') {
    return (
      INFRASTRUCTURE_SOURCE_IDS.has(source.id) ||
      source.category === 'servers' ||
      (source.category === 'networking' && !CLOUD_SOURCE_IDS.has(source.id))
    );
  }
  if (category === 'databases') return source.category === 'databases';
  if (category === 'queues') return source.category === 'queues';
  if (category === 'security') return source.category === 'security';
  if (category === 'custom') return source.category === 'custom';
  return (
    APPLICATION_SOURCE_IDS.has(source.id) ||
    source.category === 'languages' ||
    source.category === 'devops' ||
    source.category === 'ai' ||
    source.category === 'otel' ||
    source.category === 'otel-collector'
  );
}

/**
 * The category a source is shown under: the one in view when it holds the
 * source, otherwise the first that does. A source in Recommended also has a
 * home of its own, and that one is preferred over Recommended.
 */
export function categoryForSource(source: Source, preferred: PrimaryCategory): PrimaryCategory {
  if (sourceInPrimaryCategory(source, preferred)) return preferred;

  return (
    PRIMARY_CATEGORIES.find(
      (category) => category !== 'recommended' && sourceInPrimaryCategory(source, category),
    ) ?? 'recommended'
  );
}

export function sendsSignal(source: Pick<Source, 'signals'>, signal: SignalFilter): boolean {
  return signal === 'all' || source.signals.includes(signal);
}

/** One category of the catalogue, narrowed to the sources that send `signal`. */
export function sourcesInCategory(
  sources: readonly Source[],
  category: PrimaryCategory,
  signal: SignalFilter,
): Source[] {
  return sources.filter(
    (source) => sourceInPrimaryCategory(source, category) && sendsSignal(source, signal),
  );
}

/**
 * Sources that match what was typed, in every category: a search is a way to
 * reach a source by name, so it does not stop at the category or signal in
 * view. `words` gives the text a source is known by in the reader's language,
 * its name first; sources matched by name come before the rest.
 */
export function searchSources(
  sources: readonly Source[],
  query: string,
  words: (source: Source) => readonly string[],
): Source[] {
  const needle = query.trim().toLocaleLowerCase();
  if (needle === '') return [];

  const byName: Source[] = [];
  const byOtherWords: Source[] = [];

  for (const source of sources) {
    const [name = '', ...rest] = words(source).map((word) => word.toLocaleLowerCase());

    if (name.includes(needle) || source.id.includes(needle)) byName.push(source);
    else if (rest.some((word) => word.includes(needle))) byOtherWords.push(source);
  }

  return [...byName, ...byOtherWords];
}

export function maskToken(token: string): string {
  if (!token) return '';
  if (token.length <= 12) return `${token.slice(0, 4)}••••`;
  return `${token.slice(0, 8)}${'•'.repeat(14)}${token.slice(-6)}`;
}

export interface SourceSignalSummary {
  status: HomeHealthStatus;
  rows: number;
  storedBytes: number;
  lastReceivedAtMicros: number | null;
  activeSignals: number;
  expectedSignals: number;
  streamNames: string[];
}

const STATUS_PRIORITY: Record<HomeHealthStatus, number> = {
  healthy: 5,
  delayed: 4,
  degraded: 3,
  unknown: 2,
  no_data: 1,
};

export function summarizeSourceSignals(
  source: Pick<Source, 'signals'>,
  overview: HomeOverview | undefined,
): SourceSignalSummary {
  if (!overview) {
    return {
      status: 'unknown',
      rows: 0,
      storedBytes: 0,
      lastReceivedAtMicros: null,
      activeSignals: 0,
      expectedSignals: source.signals.length,
      streamNames: [],
    };
  }

  const signals = Array.isArray(overview.signals) ? overview.signals : [];
  const streams = Array.isArray(overview.streams) ? overview.streams : [];
  const matchingSignals = signals.filter((item) =>
    source.signals.includes(item.stream_type as Signal),
  );
  const matchingStreams = streams
    .filter((item) => source.signals.includes(item.stream_type as Signal) && item.rows > 0)
    .sort(
      (a, b) =>
        (b.last_received_at_micros ?? Number.NEGATIVE_INFINITY) -
        (a.last_received_at_micros ?? Number.NEGATIVE_INFINITY),
    );
  const lastReceivedAtMicros = matchingSignals.reduce<number | null>(
    (latest, item) =>
      item.last_received_at_micros != null &&
      (latest == null || item.last_received_at_micros > latest)
        ? item.last_received_at_micros
        : latest,
    null,
  );
  const activeSignals = matchingSignals.filter(
    (item) => item.rows > 0 && item.last_received_at_micros != null,
  );
  const status =
    activeSignals
      .map((item) => item.status)
      .sort((a, b) => STATUS_PRIORITY[b] - STATUS_PRIORITY[a])[0] ??
    (matchingSignals.length > 0 ? 'no_data' : 'unknown');

  return {
    status,
    rows: matchingSignals.reduce((sum, item) => sum + item.rows, 0),
    storedBytes: matchingSignals.reduce((sum, item) => sum + item.stored_bytes, 0),
    lastReceivedAtMicros,
    activeSignals: activeSignals.length,
    expectedSignals: source.signals.length,
    streamNames: [...new Set(matchingStreams.map((item) => item.name))],
  };
}
