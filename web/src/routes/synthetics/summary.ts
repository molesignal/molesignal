import type {
  MonitorKind,
  MonitorState,
  ProbeOutcome,
  SyntheticResult,
} from '@/api/synthetics';

import type { CheckRow } from './data';
import { operationalResults } from './model';

/** Worst first: what a reader has to look at before anything else. */
export const STATES_WORST_FIRST: readonly MonitorState[] = [
  'failing',
  'flaky',
  'degraded',
  'unknown',
  'healthy',
];

/** One colour per state, for marks, bars and run strips. Text takes the same token. */
export const STATE_COLOR: Record<MonitorState | ProbeOutcome, string> = {
  failing: 'var(--red)',
  flaky: 'var(--orange)',
  degraded: 'var(--yellow)',
  healthy: 'var(--green)',
  unknown: 'var(--tx-3)',
  skipped: 'var(--tx-3)',
};

export const STATE_TEXT: Record<MonitorState | ProbeOutcome, string> = {
  failing: 'text-red',
  flaky: 'text-orange',
  degraded: 'text-yellow',
  healthy: 'text-green',
  unknown: 'text-tx-2',
  skipped: 'text-tx-2',
};

/** The kinds of check a reader thinks in: what is called, not which protocol. */
export type CheckFamily = 'api' | 'browser' | 'network' | 'heartbeat';

export const CHECK_FAMILIES: readonly CheckFamily[] = ['api', 'browser', 'network', 'heartbeat'];

export function checkFamily(kind: MonitorKind): CheckFamily {
  if (kind === 'http') return 'api';
  if (kind === 'browser' || kind === 'heartbeat') return kind;
  return 'network';
}

/** Checks that are on schedule. A paused or draft check has no state worth reading. */
export function runningChecks(rows: readonly CheckRow[]): CheckRow[] {
  return rows.filter((row) => row.monitor.lifecycle === 'active');
}

export function stateCounts(rows: readonly CheckRow[]): Record<MonitorState, number> {
  const counts: Record<MonitorState, number> = {
    failing: 0,
    flaky: 0,
    degraded: 0,
    unknown: 0,
    healthy: 0,
  };
  for (const row of rows) counts[row.monitor.state] += 1;

  return counts;
}

/**
 * What the checks say, as one word.
 *
 * A check that has not reported yet does not make the module unwell: it is
 * `waiting` only when nothing else can be said.
 */
export type ChecksVerdict = 'failing' | 'attention' | 'passing' | 'waiting' | 'idle';

export function checksVerdict(counts: Record<MonitorState, number>): ChecksVerdict {
  if (counts.failing > 0) return 'failing';
  if (counts.flaky + counts.degraded > 0) return 'attention';
  if (counts.healthy > 0) return 'passing';
  if (counts.unknown > 0) return 'waiting';

  return 'idle';
}

function isDecided(result: SyntheticResult): boolean {
  return result.outcome !== 'unknown' && result.outcome !== 'skipped';
}

/** Scheduled runs that came to a conclusion, the latest first. */
export function decidedRuns(results: readonly SyntheticResult[]): SyntheticResult[] {
  return operationalResults([...results])
    .filter(isDecided)
    .sort((left, right) => right.started_at - left.started_at);
}

/**
 * When the present run of trouble began: the first of the latest runs in a row
 * that did not pass. Nothing when the latest run passed, or there is none.
 */
export function unhealthySince(results: readonly SyntheticResult[]): number | undefined {
  let since: number | undefined;
  for (const result of decidedRuns(results)) {
    if (result.outcome === 'healthy') break;
    since = result.started_at;
  }

  return since;
}

/** When a run last did not pass, among the runs given. */
export function lastFailureAt(results: readonly SyntheticResult[]): number | undefined {
  return decidedRuns(results).find((result) => result.outcome !== 'healthy')?.started_at;
}

/** What the latest run that did not pass said went wrong. */
export function latestError(results: readonly SyntheticResult[]): string | undefined {
  const failed = decidedRuns(results).find((result) => result.outcome !== 'healthy');
  if (!failed) return undefined;

  const attempt = [...failed.attempts]
    .reverse()
    .find((candidate) => candidate.error_message || candidate.error_category);
  const assertion = failed.assertions.find((observation) => !observation.passed);

  return (
    attempt?.error_message ||
    assertion?.message ||
    attempt?.error_category ||
    undefined
  );
}

/** The latest decided run from each location, the latest first. */
export function latestByLocation(
  results: readonly SyntheticResult[],
): Map<string, SyntheticResult> {
  const latest = new Map<string, SyntheticResult>();
  for (const result of decidedRuns(results)) {
    if (!latest.has(result.location_id)) latest.set(result.location_id, result);
  }

  return latest;
}

/** The locations whose latest run did not pass. */
export function troubledLocationIds(results: readonly SyntheticResult[]): string[] {
  return [...latestByLocation(results)]
    .filter(([, result]) => result.outcome !== 'healthy')
    .map(([locationId]) => locationId);
}

export interface CheckAttention {
  row: CheckRow;
  /** Where its latest runs did not pass. */
  locationIds: string[];
  error: string | undefined;
  since: number | undefined;
}

/**
 * The checks someone has to look at, the worst first and, among equals, the
 * one that has been unwell the longest.
 */
export function checksNeedingAttention(rows: readonly CheckRow[]): CheckAttention[] {
  return runningChecks(rows)
    .filter((row) => ['failing', 'flaky', 'degraded'].includes(row.monitor.state))
    .map((row) => ({
      row,
      locationIds: troubledLocationIds(row.results),
      error: latestError(row.results),
      since: unhealthySince(row.results),
    }))
    .sort(
      (left, right) =>
        STATES_WORST_FIRST.indexOf(left.row.monitor.state) -
          STATES_WORST_FIRST.indexOf(right.row.monitor.state) ||
        (left.since ?? Number.POSITIVE_INFINITY) - (right.since ?? Number.POSITIVE_INFINITY),
    );
}

/**
 * How an availability figure reads: unremarkable above the first line, worth a
 * second look under it, and a problem under the second.
 */
export function availabilityTone(value: number | undefined): 'warn' | 'danger' | undefined {
  if (value === undefined || value >= 0.99) return undefined;

  return value < 0.95 ? 'danger' : 'warn';
}

/** The share of decided runs that passed. Nothing when there are none. */
export function availability(results: readonly SyntheticResult[]): number | undefined {
  const decided = decidedRuns(results);
  if (decided.length === 0) return undefined;

  return decided.filter((result) => result.outcome === 'healthy').length / decided.length;
}

/**
 * Availability over the time the runs span, in equal steps from the oldest run
 * to the latest. A step nothing ran in is `null`, so a trend drawn from it
 * breaks there instead of claiming a value.
 */
export function availabilityTrend(
  results: readonly SyntheticResult[],
  steps = 24,
): Array<number | null> {
  const decided = decidedRuns(results);
  const latest = decided[0];
  const oldest = decided.at(-1);
  if (!latest || !oldest || steps < 1) return [];

  const span = Math.max(1, latest.started_at - oldest.started_at);
  const passed = new Array<number>(steps).fill(0);
  const total = new Array<number>(steps).fill(0);
  for (const result of decided) {
    const step = Math.min(
      steps - 1,
      Math.floor(((result.started_at - oldest.started_at) / span) * steps),
    );
    total[step] = (total[step] ?? 0) + 1;
    if (result.outcome === 'healthy') passed[step] = (passed[step] ?? 0) + 1;
  }

  return total.map((count, step) => (count > 0 ? (passed[step] ?? 0) / count : null));
}

/** The latest scheduled runs, oldest first, for a strip that reads left to right. */
export function runStrip(results: readonly SyntheticResult[], length = 20): SyntheticResult[] {
  return operationalResults([...results])
    .sort((left, right) => right.started_at - left.started_at)
    .slice(0, length)
    .reverse();
}
