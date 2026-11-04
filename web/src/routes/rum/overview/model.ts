import type { RumAggregate, RumAggregateParams, RumFilter, RumMeasure, RumPlatform, RumType } from '@/api/rum';

import { formatDurationMs } from '../_helpers';
import { quoteRumValue } from '../explorer/urlState';

/** What the overview is of: one application, in one environment or in all of them. */
export interface OverviewScope {
  application: string;
  environment: string;
  /** What a page is narrowed to besides: the conditions of a segment. */
  filters?: readonly RumFilter[];
}

/** The range every read of the page is made over, in microseconds. */
export interface OverviewRange {
  from: number;
  to: number;
}

/**
 * The SDKs of a browser send web vitals and URLs; those of an app send neither, so its overview
 * counts crashes where a page counts slow loads, and tells its screens by name.
 */
export const isBrowser = (platform: RumPlatform) => platform === 'browser';

/** The filters every read of the page carries: the application, and the environment if one is chosen. */
export function scopeFilters(scope: OverviewScope, extra: RumFilter[] = []): RumFilter[] {
  const filters: RumFilter[] = [{ field: 'application.id', op: '=', value: scope.application }];

  if (scope.environment) filters.push({ field: 'env', op: '=', value: scope.environment });

  return [...filters, ...(scope.filters ?? []), ...extra];
}

/** A comparison of a field with a value, as the explorer's field query writes it. */
export const clause = (field: string, value: string) => `${field} = '${quoteRumValue(value)}'`;

/**
 * Where a number of the overview leads: the explorer, on the rows that were counted. The address
 * says what the list is of, so it can be sent to someone as it stands.
 */
export function explorerPath(type: RumType, scope: OverviewScope, clauses: readonly string[]): string {
  const query = [
    clause('application.id', scope.application),
    ...(scope.environment ? [clause('env', scope.environment)] : []),
    ...clauses,
  ].join(' AND ');

  return `/rum/explorer?${new URLSearchParams({ type, q: query })}`;
}

/**
 * Where a vital of the overview leads: the performance page, of the same application and
 * environment, with that vital in front. The time range goes with the reader, not in the address.
 */
export function performancePath(scope: OverviewScope, vital: VitalKey): string {
  const params = new URLSearchParams({ app: scope.application });

  if (scope.environment) params.set('env', scope.environment);
  // The first vital is what the page opens on.
  if (vital !== 'lcp') params.set('vital', vital);

  return `/rum/performance?${params}`;
}

// ---------------------------------------------------------------------------------------------
// The three Core Web Vitals, with the thresholds web.dev gives.

export type VitalKey = 'lcp' | 'inp' | 'cls';

/** What a level of a view is graded by, a vital or a measure read beside them. */
export interface Graded {
  /** The SDK sends it in nanoseconds; the page reads it in milliseconds. A score stays a score. */
  nanoseconds: boolean;
  /** A 75th percentile at or under `good` is good, over `poor` is poor. In milliseconds, or as a score. */
  good: number;
  poor: number;
  /** Where the scale drawn for it ends. */
  scaleMax: number;
}

export interface Vital extends Graded {
  key: VitalKey;
  /** The column of a view that holds it. */
  field: string;
}

const LCP: Vital = { key: 'lcp', field: 'view.performance.lcp.timestamp', nanoseconds: true, good: 2_500, poor: 4_000, scaleMax: 6_000 };
const INP: Vital = { key: 'inp', field: 'view.performance.inp.duration', nanoseconds: true, good: 200, poor: 500, scaleMax: 800 };
const CLS: Vital = { key: 'cls', field: 'view.performance.cls.score', nanoseconds: false, good: 0.1, poor: 0.25, scaleMax: 0.4 };

export const VITALS: readonly Vital[] = [LCP, INP, CLS];

export type Grade = 'good' | 'needs_improvement' | 'poor';

export function gradeOf(vital: Pick<Graded, 'good' | 'poor'>, value: number): Grade {
  if (value <= vital.good) return 'good';

  return value <= vital.poor ? 'needs_improvement' : 'poor';
}

/** A value a view carries, in the unit the page reads the vital in. */
export function inUnit(vital: Pick<Graded, 'nanoseconds'>, raw: number): number {
  return vital.nanoseconds ? raw / 1_000_000 : raw;
}

/** How far along its scale a value lies, from 0 to 1; a value past the end stands at the end. */
export function markerOf(vital: Pick<Graded, 'scaleMax'>, value: number): number {
  return Math.min(1, Math.max(0, value / vital.scaleMax));
}

/** The lengths of the three stretches of a vital's scale, as shares of it. */
export function scaleOf(vital: Graded): [good: number, middle: number, poor: number] {
  return [vital.good / vital.scaleMax, (vital.poor - vital.good) / vital.scaleMax, (vital.scaleMax - vital.poor) / vital.scaleMax];
}

/** A level in the words of its unit: a time as `1.4 s`, a score as `0.12`. */
export function formatVital(vital: Pick<Graded, 'nanoseconds'>, value: number): string {
  return vital.nanoseconds ? formatDurationMs(value) : String(Number(value.toFixed(2)));
}

// ---------------------------------------------------------------------------------------------
// What the page asks.

export const count = (name: string, where?: RumFilter[]): RumMeasure => ({ fn: 'count', name, ...(where && { where }) });

export const percentile = (name: string, field: string): RumMeasure => ({ fn: 'p75', field, name });

/** People are told apart by their id, or where a visitor has none by the anonymous id the SDK keeps. */
const people = (name: string, where?: RumFilter[]): RumMeasure => ({
  fn: 'count_distinct',
  field: 'usr.id',
  fallback: 'usr.anonymous_id',
  name,
  ...(where && { where }),
});

const positive = (field: string): RumFilter[] => [{ field, op: '>', value: '0' }];

const read = (type: RumAggregateParams['type'], scope: OverviewScope, range: OverviewRange, extra?: RumFilter[]) => ({
  type,
  from: range.from,
  to: range.to,
  filters: scopeFilters(scope, extra),
});

/** How many sessions and people there were, and how many met an error or a crash. */
export function pulseQuery(scope: OverviewScope, range: OverviewRange): RumAggregateParams {
  return {
    ...read('sessions', scope, range),
    measures: [
      count('sessions'),
      people('users'),
      count('with_errors', positive('session.error.count')),
      count('with_crashes', positive('session.crash.count')),
      people('users_with_crashes', positive('session.crash.count')),
    ],
  };
}

/** The 75th percentile of each vital over every view, and how many views had a value of it. */
export function vitalsQuery(scope: OverviewScope, range: OverviewRange): RumAggregateParams {
  return {
    ...read('views', scope, range),
    measures: VITALS.flatMap((vital) => [
      percentile(vital.key, vital.field),
      // A count of a column counts the views that have a value in it.
      { fn: 'count' as const, field: vital.field, name: `${vital.key}_samples` },
    ]),
  };
}

/** Where the page of a view is told by: its URL's path with the changing parts grouped, or a screen's name. */
export const pageField = (platform: RumPlatform) => (isBrowser(platform) ? 'view.url_path_group' : 'view.name');

export const PAGE_ROWS = 8;

/** The most viewed pages, each with its vitals and how many of its views met an error. */
export function pagesQuery(scope: OverviewScope, range: OverviewRange, platform: RumPlatform): RumAggregateParams {
  return {
    ...read('views', scope, range),
    groupBy: [pageField(platform)],
    measures: [
      count('views'),
      ...(isBrowser(platform) ? VITALS.map((vital) => percentile(vital.key, vital.field)) : []),
      count('with_errors', positive('view.error.count')),
    ],
    orderBy: 'views',
    limit: PAGE_ROWS,
  };
}

/** Sessions per country: every country there is, for the map. */
export function countriesQuery(scope: OverviewScope, range: OverviewRange): RumAggregateParams {
  return {
    ...read('sessions', scope, range),
    groupBy: ['geo.country_iso_code'],
    measures: [count('sessions')],
    orderBy: 'sessions',
    limit: 250,
  };
}

export const SHARE_ROWS = 5;

/** Sessions per value of a column of them: a browser, an operating system, a kind of device. */
export function sharesQuery(scope: OverviewScope, range: OverviewRange, field: string): RumAggregateParams {
  return {
    ...read('sessions', scope, range),
    groupBy: [field],
    measures: [count('sessions')],
    orderBy: 'sessions',
    limit: SHARE_ROWS + 1,
  };
}

export const FRUSTRATION_ROWS = 5;

const frustration = (word: string): RumFilter[] => [{ field: 'action.frustration.type', op: 'contains', value: word }];

/** The elements users were frustrated by most, on the page they were: how many actions, and of which kind. */
export function frustrationQuery(scope: OverviewScope, range: OverviewRange, platform: RumPlatform): RumAggregateParams {
  return {
    ...read('actions', scope, range, positive('action.frustration.count')),
    groupBy: ['action.target.name', pageField(platform)],
    measures: [
      count('total'),
      count('rage', frustration('rage_')),
      count('dead', frustration('dead_')),
      count('error', frustration('error_')),
    ],
    orderBy: 'total',
    limit: FRUSTRATION_ROWS,
  };
}

// ---------------------------------------------------------------------------------------------
// What the page reads out of the answers.

type Row = RumAggregate['rows'][number];

/** A number the answer has: a value the reader may not see comes back as text, and is none. */
export const numeric = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);

/** The numbers of a row, by the names of the measures. */
export function named(answer: RumAggregate, row: Row): Record<string, number | null> {
  const found: Record<string, number | null> = {};

  answer.measures.forEach((measure, index) => {
    found[measure.name] = numeric(row.values[index]);
  });

  return found;
}

const text = (value: string | null | undefined): string | null => (value ? value : null);

export interface Pulse {
  sessions: number;
  users: number;
  withErrors: number;
  withCrashes: number;
  usersWithCrashes: number;
}

/** The numbers of the sessions: none for a read that has not answered or that is masked, zeros for no sessions. */
export function readPulse(answer: RumAggregate | undefined): Pulse | null {
  if (!answer) return null;

  const row = answer.rows[0];

  if (!row) return { sessions: 0, users: 0, withErrors: 0, withCrashes: 0, usersWithCrashes: 0 };

  const found = named(answer, row);

  if (found.sessions === null || found.sessions === undefined) return null;

  return {
    sessions: found.sessions,
    users: found.users ?? 0,
    withErrors: found.with_errors ?? 0,
    withCrashes: found.with_crashes ?? 0,
    usersWithCrashes: found.users_with_crashes ?? 0,
  };
}

/** What is left of a total when some are taken away, as a share of it; none without a total. */
const remaining = (total: number, taken: number): number | null => (total > 0 ? Math.min(1, Math.max(0, 1 - taken / total)) : null);

export const errorFreeSessions = (pulse: Pulse) => remaining(pulse.sessions, pulse.withErrors);
export const crashFreeSessions = (pulse: Pulse) => remaining(pulse.sessions, pulse.withCrashes);
export const crashFreeUsers = (pulse: Pulse) => remaining(pulse.users, pulse.usersWithCrashes);

export interface VitalReading {
  vital: Vital;
  /** In the unit the page reads it in; none when no view had it. */
  value: number | null;
  samples: number;
  grade: Grade | null;
}

/** The three vitals in their order, each with what views had of it. */
export function readVitals(answer: RumAggregate | undefined): VitalReading[] {
  const row = answer?.rows[0];
  const found = answer && row ? named(answer, row) : {};

  return VITALS.map((vital) => {
    const raw = found[vital.key] ?? null;
    const samples = found[`${vital.key}_samples`] ?? 0;
    const value = raw !== null && samples > 0 ? inUnit(vital, raw) : null;

    return { vital, value, samples, grade: value === null ? null : gradeOf(vital, value) };
  });
}

export interface PageRow {
  page: string;
  views: number;
  /** In the unit the page reads each in; none where the page has no value or the application sends no vitals. */
  lcp: number | null;
  inp: number | null;
  cls: number | null;
  /** The share of its views that met an error. */
  errorRate: number | null;
}

export function readPages(answer: RumAggregate | undefined): PageRow[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const page = text(row.group[0]);
    const found = named(answer, row);

    if (page === null || found.views === null || found.views === undefined) return [];

    const value = (vital: Vital): number | null => {
      const raw = found[vital.key];

      return raw === null || raw === undefined ? null : inUnit(vital, raw);
    };

    return [
      {
        page,
        views: found.views,
        lcp: value(LCP),
        inp: value(INP),
        cls: value(CLS),
        errorRate: found.views > 0 ? (found.with_errors ?? 0) / found.views : null,
      },
    ];
  });
}

export interface Share {
  value: string;
  sessions: number;
}

/** The values of a column that sessions have, the commonest first. */
export function readShares(answer: RumAggregate | undefined): Share[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const value = text(row.group[0]);
    const sessions = named(answer, row).sessions;

    return value !== null && sessions !== null && sessions !== undefined && sessions > 0 ? [{ value, sessions }] : [];
  });
}

/** The first `limit` values and what is left of the total beyond them: the others, and the sessions with no value. */
export function withOthers(shares: readonly Share[], total: number, limit: number): { shown: Share[]; others: number } {
  const shown = shares.slice(0, limit);

  return { shown, others: Math.max(0, total - shown.reduce((sum, share) => sum + share.sessions, 0)) };
}

export interface Country {
  code: string;
  sessions: number;
}

export function readCountries(answer: RumAggregate | undefined): Country[] {
  return readShares(answer).map((share) => ({ code: share.value, sessions: share.sessions }));
}

export type FrustrationKind = 'rage' | 'dead' | 'error';

export interface FrustratedElement {
  /** The name of the element; none where the SDK could not name it. */
  element: string | null;
  page: string | null;
  total: number;
  /** What most of the actions on it were. */
  kind: FrustrationKind;
}

/** Most of the three kinds wins; a tie goes to the one that says most about the page: rage, then dead, then error. */
function dominant(found: Record<string, number | null>): FrustrationKind {
  const kinds: FrustrationKind[] = ['rage', 'dead', 'error'];

  return kinds.reduce((best, kind) => ((found[kind] ?? 0) > (found[best] ?? 0) ? kind : best), 'rage');
}

export function readFrustration(answer: RumAggregate | undefined): FrustratedElement[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const found = named(answer, row);

    if (found.total === null || found.total === undefined || found.total <= 0) return [];

    return [{ element: text(row.group[0]), page: text(row.group[1]), total: found.total, kind: dominant(found) }];
  });
}
