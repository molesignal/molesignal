import type { VersionSummary } from '@/api/apm';
import type { ChangeEntry } from '@/api/changes';

/** Versions seen this close to the last one seen are still running. */
export const LIVE_WINDOW_MICROS = 5 * 60_000_000;

/**
 * What happened to the service at a moment: a pipeline reported a deployment
 * (that went out, failed, or took a version back) or a change of its
 * configuration, of a feature flag, of its scaling or of the infrastructure
 * under it; or spans first carried a version no pipeline reported, or they
 * went back to an older version.
 */
export type TimelineKind = 'deployed' | 'failed' | 'rolled_back' | 'first_seen' | 'returned' | 'configuration' | 'feature_flag' | 'scaling' | 'infrastructure';

/** Kinds that change no version: they sit on the line without moving it. */
const VERSIONLESS: ReadonlySet<TimelineKind> = new Set(['failed', 'configuration', 'feature_flag', 'scaling', 'infrastructure']);

export interface TimelineEntry {
  key: string;
  at: number;
  version: string;
  kind: TimelineKind;
  /** What the pipeline reported. */
  event?: ChangeEntry;
  /** What spans saw of the version, on the newest entry of each version only. */
  observed?: VersionSummary;
  /** The version a rollback went back from. */
  from?: string;
}

/**
 * The service's deployments, the newest first: each one its pipeline
 * reported, each version spans carried that no pipeline reported, and a
 * return to an older version spans show without a report. A deployment of a
 * version that ran before another is a rollback, whatever its status says.
 */
export function deployTimeline(events: readonly ChangeEntry[], versions: readonly VersionSummary[]): TimelineEntry[] {
  const reportedKinds = events.filter((event) => event.kind !== 'annotation' && event.kind !== 'version' && event.kind !== 'rule');
  const deployments = reportedKinds.filter((event) => event.kind === 'deployment');
  const reported = new Set(deployments.map((event) => event.version ?? ''));
  const entries: TimelineEntry[] = [];

  for (const event of reportedKinds) {
    const status = event.status ?? 'succeeded';

    entries.push({
      key: `event:${event.id}`,
      at: event.at,
      version: event.version ?? '',
      kind: event.kind !== 'deployment' ? (event.kind as TimelineKind) : status === 'succeeded' ? 'deployed' : status,
      event,
    });
  }

  for (const version of versions) {
    if (!reported.has(version.version)) {
      entries.push({ key: `seen:${version.version}`, at: version.firstSeenAt, version: version.version, kind: 'first_seen' });
    }
  }

  const returned = returnedVersion(versions);

  // A return a pipeline reported after the newer version came is already on the line.
  if (returned && !deployments.some((event) => event.version === returned.version && event.at > returned.after.firstSeenAt)) {
    entries.push({ key: `returned:${returned.version}`, at: returned.after.lastSeenAt, version: returned.version, kind: 'returned', from: returned.after.version });
  }

  entries.sort((left, right) => left.at - right.at || left.key.localeCompare(right.key));

  // Oldest first, a version that went out again after another is a rollback from the one before it.
  let current: string | null = null;
  const before = new Set<string>();

  for (const entry of entries) {
    // A failure, or a change other than a deployment, leaves the running version as it was.
    if (VERSIONLESS.has(entry.kind)) continue;

    if (current !== null && entry.version !== current) {
      if (before.has(entry.version) && entry.kind === 'deployed') entry.kind = 'rolled_back';
      if (entry.kind === 'rolled_back' || entry.kind === 'returned') entry.from ??= current;
    }

    if (current !== null) before.add(current);
    current = entry.version;
  }

  entries.reverse();

  const byVersion = new Map(versions.map((version) => [version.version, version]));
  const shown = new Set<string>();

  for (const entry of entries) {
    const observed = byVersion.get(entry.version);

    if (observed && !shown.has(entry.version) && !VERSIONLESS.has(entry.kind)) {
      entry.observed = observed;
      shown.add(entry.version);
    }
  }

  return entries;
}

/**
 * The older version spans went back to: the one seen last was first seen
 * before another that stopped well before it.
 */
function returnedVersion(versions: readonly VersionSummary[]): { version: string; after: VersionSummary } | null {
  const latest = [...versions].sort((left, right) => right.lastSeenAt - left.lastSeenAt)[0];

  if (!latest) return null;

  const after = versions
    .filter((version) => version.firstSeenAt > latest.firstSeenAt && latest.lastSeenAt - version.lastSeenAt > LIVE_WINDOW_MICROS)
    .sort((left, right) => right.lastSeenAt - left.lastSeenAt)[0];

  return after ? { version: latest.version, after } : null;
}

export interface CanaryPair {
  baseline: VersionSummary;
  candidate: VersionSummary;
  /** Since when both run: what their side by side is read over. */
  from: number;
}

/** Two versions running at once, as a canary runs beside the version it replaces: the newest two still seen. */
export function canaryPair(versions: readonly VersionSummary[]): CanaryPair | null {
  const lastSeen = Math.max(...versions.map((version) => version.lastSeenAt));
  const live = versions
    .filter((version) => lastSeen - version.lastSeenAt <= LIVE_WINDOW_MICROS)
    .sort((left, right) => right.firstSeenAt - left.firstSeenAt);
  const [candidate, baseline] = live;

  return candidate && baseline ? { baseline, candidate, from: candidate.firstSeenAt } : null;
}

/** Where a commit can be read: the commit's page of a repository on the web. */
export function commitLink(repository: string | undefined, commit: string): string | null {
  if (!repository || !/^https?:\/\//i.test(repository)) return null;

  const base = repository.replace(/\.git$/i, '').replace(/\/+$/, '');

  if (/gitlab/i.test(base)) return `${base}/-/commit/${commit}`;
  if (/bitbucket\.org/i.test(base)) return `${base}/commits/${commit}`;

  return `${base}/commit/${commit}`;
}
