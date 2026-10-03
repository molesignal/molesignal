import type { StreamRuntime, StreamRuntimeStatus } from '@/api/streams';

/**
 * How Home talks about a stream. The backend only reports runtime statuses;
 * `stale` splits `interrupted` into a recent break (someone should look now)
 * and a long silence (nobody is paged by it), so a fleet that has been quiet
 * for days stops reading as an incident wall.
 */
export type StreamCondition =
  | 'healthy'
  | 'delayed'
  | 'interrupted'
  | 'stale'
  | 'inactive'
  | 'unknown';

/** Semantic tone: red = act now, yellow = look soon, green = fine, dim = nothing to say. */
export type ConditionTone = 'green' | 'yellow' | 'red' | 'dim';

export type HealthVerdictKind =
  | 'no_streams'
  | 'healthy'
  | 'delayed'
  | 'interrupted'
  | 'stale'
  | 'no_data'
  | 'unknown';

/** Silence longer than this turns an interrupted stream into a stale one. */
export const STALE_AFTER_MICROS = 24 * 60 * 60 * 1_000_000;

export const CONDITION_TONE: Record<StreamCondition, ConditionTone> = {
  healthy: 'green',
  delayed: 'yellow',
  interrupted: 'red',
  stale: 'yellow',
  inactive: 'dim',
  unknown: 'dim',
};

export const VERDICT_TONE: Record<HealthVerdictKind, ConditionTone> = {
  no_streams: 'dim',
  healthy: 'green',
  delayed: 'yellow',
  interrupted: 'red',
  stale: 'yellow',
  no_data: 'dim',
  unknown: 'dim',
};

/** The per-stream condition a verdict is built from (`no_streams` has none). */
export const VERDICT_CONDITION: Record<HealthVerdictKind, StreamCondition> = {
  no_streams: 'inactive',
  healthy: 'healthy',
  delayed: 'delayed',
  interrupted: 'interrupted',
  stale: 'stale',
  no_data: 'inactive',
  unknown: 'unknown',
};

// API timestamps are microseconds, but older payloads carry epoch seconds.
export function toMicros(value: number): number {
  return value < 1e12 ? value * 1_000_000 : value;
}

export function classifyStreamCondition({
  status,
  lastReceivedAtMicros,
  nowMicros,
}: {
  status: StreamRuntimeStatus;
  lastReceivedAtMicros: number | null | undefined;
  nowMicros: number;
}): StreamCondition {
  switch (status) {
    case 'healthy':
      return 'healthy';
    case 'delayed':
      return 'delayed';
    case 'interrupted':
      return lastReceivedAtMicros != null &&
        nowMicros - toMicros(lastReceivedAtMicros) > STALE_AFTER_MICROS
        ? 'stale'
        : 'interrupted';
    case 'idle':
    case 'unused':
      return 'inactive';
    default:
      return 'unknown';
  }
}

export interface FleetSummary {
  total: number;
  counts: Record<StreamCondition, number>;
  /** The worst condition present; it drives the page verdict. */
  verdict: HealthVerdictKind;
  /** Streams sharing the verdict's condition. */
  affected: number;
  /** Streams that are receiving normally. */
  receiving: number;
  /** Every stream is in the verdict's condition, so the page can say "all N". */
  uniform: boolean;
  lastReceivedAtMicros: number | null;
}

type FleetStream = Pick<StreamRuntime, 'status' | 'last_received_at_micros'>;

function emptyCounts(): Record<StreamCondition, number> {
  return {
    healthy: 0,
    delayed: 0,
    interrupted: 0,
    stale: 0,
    inactive: 0,
    unknown: 0,
  };
}

/** The verdict when runtime status could not be fetched: say so, don't guess. */
export const UNKNOWN_FLEET: FleetSummary = {
  total: 0,
  counts: emptyCounts(),
  verdict: 'unknown',
  affected: 0,
  receiving: 0,
  uniform: false,
  lastReceivedAtMicros: null,
};

// Worst first: an operator reads the page top-down and stops at the first match.
const VERDICT_PRECEDENCE: ReadonlyArray<
  [StreamCondition, Exclude<HealthVerdictKind, 'no_streams' | 'no_data'>]
> = [
  ['interrupted', 'interrupted'],
  ['delayed', 'delayed'],
  ['stale', 'stale'],
  ['unknown', 'unknown'],
  ['healthy', 'healthy'],
];

export function summarizeFleet(
  streams: readonly FleetStream[],
  nowMicros: number,
): FleetSummary {
  const counts = emptyCounts();
  let lastReceived: number | null = null;
  for (const stream of streams) {
    const condition = classifyStreamCondition({
      status: stream.status,
      lastReceivedAtMicros: stream.last_received_at_micros,
      nowMicros,
    });
    counts[condition] += 1;
    if (stream.last_received_at_micros != null) {
      const micros = toMicros(stream.last_received_at_micros);
      lastReceived = lastReceived == null ? micros : Math.max(lastReceived, micros);
    }
  }

  const total = streams.length;
  const match = VERDICT_PRECEDENCE.find(([condition]) => counts[condition] > 0);
  const verdict: HealthVerdictKind =
    total === 0 ? 'no_streams' : (match?.[1] ?? 'no_data');
  const affected = total === 0 ? 0 : counts[VERDICT_CONDITION[verdict]];

  return {
    total,
    counts,
    verdict,
    affected,
    receiving: counts.healthy,
    uniform: total > 0 && affected === total,
    lastReceivedAtMicros: lastReceived,
  };
}
