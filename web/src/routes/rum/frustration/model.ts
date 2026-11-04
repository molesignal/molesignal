import type { RumAggregate, RumAggregateParams, RumFilter, RumMeasure, RumPlatform, RumRow, RumSearchParams } from '@/api/rum';

import { count, isBrowser, named, numeric, type OverviewRange, type OverviewScope, pageField, scopeFilters } from '../overview/model';

// ---------------------------------------------------------------------------------------------
// What a frustrated action is.

const COUNT_FIELD = 'action.frustration.count';
const TYPE_FIELD = 'action.frustration.type';
const NAME = 'action.target.name';
const SELECTOR = 'action.target.selector';
const SESSION = 'session.id';

/** The three signs an SDK reads: clicks again and again, a click nothing came of, a click that ended in an error. */
export type Kind = 'rage' | 'dead' | 'error';

export const KINDS: readonly Kind[] = ['rage', 'dead', 'error'];

// A browser writes `rage_click`, an app `rage_tap`: the beginning tells the kind of both.
const PREFIX: Record<Kind, string> = { rage: 'rage_', dead: 'dead_', error: 'error_' };

export const parseKind = (value: string | null): Kind | '' => KINDS.find((kind) => kind === value) ?? '';

/** The signs the SDK of an application reads: the browser's three, and for an app the two that taps can show. */
export const kindsFor = (platform: RumPlatform): readonly Kind[] => (isBrowser(platform) ? KINDS : ['rage', 'error']);

const frustrated: RumFilter[] = [{ field: COUNT_FIELD, op: '>', value: '0' }];

const ofKind = (kind: Kind): RumFilter[] => [{ field: TYPE_FIELD, op: 'contains', value: PREFIX[kind] }];

/** People who met a thing are counted by their sessions: one who clicks ten times is not ten. */
const sessionsOf = (name: string, where?: RumFilter[]): RumMeasure => ({
  fn: 'count_distinct',
  field: SESSION,
  name,
  ...(where && { where }),
});

/** The kind of frustration and the words a reader narrowed the page to, which every number of it is narrowed by. */
export interface Narrowing {
  kind: Kind | '';
  search: string;
}

const read = (scope: OverviewScope, range: OverviewRange, narrowing: Narrowing, extra: RumFilter[] = []) => {
  const words = narrowing.search.trim();

  return {
    type: 'actions' as const,
    from: range.from,
    to: range.to,
    filters: scopeFilters(scope, [...frustrated, ...(narrowing.kind ? ofKind(narrowing.kind) : []), ...extra]),
    ...(words && { freeText: [words] }),
  };
};

// ---------------------------------------------------------------------------------------------
// What the page says first.

/** How many frustrated actions there were of each kind, and in how many sessions, over every action of the range. */
export function statementQuery(scope: OverviewScope, range: OverviewRange): RumAggregateParams {
  return {
    ...read(scope, range, { kind: '', search: '' }),
    measures: [sessionsOf('sessions'), ...KINDS.flatMap((kind) => [count(kind, ofKind(kind)), sessionsOf(`${kind}_sessions`, ofKind(kind))])],
  };
}

/** How many sessions there were, and how many of them showed a sign of frustration. */
export function shareQuery(scope: OverviewScope, range: OverviewRange): RumAggregateParams {
  return {
    type: 'sessions',
    from: range.from,
    to: range.to,
    filters: scopeFilters(scope),
    measures: [count('sessions'), count('affected', [{ field: 'session.frustration.count', op: '>', value: '0' }])],
  };
}

export interface KindTotal {
  kind: Kind;
  actions: number;
  sessions: number;
}

export interface Statement {
  /** The sessions with at least one frustrated action. */
  sessions: number;
  /** The kinds that occurred, always in the order rage, dead, error. */
  kinds: KindTotal[];
}

export function readStatement(answer: RumAggregate | undefined): Statement | null {
  const row = answer?.rows[0];

  if (!answer || !row) return null;

  const found = named(answer, row);
  const kinds = KINDS.flatMap((kind): KindTotal[] => {
    const actions = found[kind] ?? 0;

    return actions > 0 ? [{ kind, actions, sessions: found[`${kind}_sessions`] ?? 0 }] : [];
  });

  return { sessions: found.sessions ?? 0, kinds };
}

/** The share of the sessions that showed a sign of frustration; none where there were no sessions. */
export function readShare(answer: RumAggregate | undefined): number | null {
  const row = answer?.rows[0];

  if (!answer || !row) return null;

  const found = named(answer, row);

  return found.sessions !== null && found.sessions !== undefined && found.sessions > 0 ? Math.min(1, (found.affected ?? 0) / found.sessions) : null;
}

// ---------------------------------------------------------------------------------------------
// What the table is of.

export const GROUP_KEYS = ['element', 'page', 'version', 'browser', 'system'] as const;

export type GroupKey = (typeof GROUP_KEYS)[number];

/** What the table lists: the values of one or two columns, each a group of frustrated actions. */
export interface Group {
  key: GroupKey;
  fields: readonly string[];
}

export const DEFAULT_GROUP: GroupKey = 'element';

/**
 * The groupings an application can be told by. A browser names an element by its text and its selector
 * and is told apart by the browser; an app has no selector and is told apart by its system, and its
 * pages are screens.
 */
export function groupsFor(platform: RumPlatform): Group[] {
  const browser = isBrowser(platform);

  return [
    { key: 'element', fields: browser ? [NAME, SELECTOR] : [NAME] },
    { key: 'page', fields: [pageField(platform)] },
    { key: 'version', fields: ['version'] },
    browser ? { key: 'browser', fields: ['browser.name'] } : { key: 'system', fields: ['os.name'] },
  ];
}

export const parseGroup = (value: string | null): GroupKey => GROUP_KEYS.find((key) => key === value) ?? DEFAULT_GROUP;

/** What a group is called: the pages of an app are its screens. */
export const groupWord = (key: GroupKey, browser: boolean): GroupKey | 'screen' => (key === 'page' && !browser ? 'screen' : key);

/** The grouping a key stands for in an application; one it has none of is the elements. */
export function groupOf(platform: RumPlatform, key: GroupKey): Group {
  const groups = groupsFor(platform);

  return groups.find((group) => group.key === key) ?? (groups[0] as Group);
}

/** The condition that makes the actions of one group: each column its value, or without one where the group has none. */
export const groupFilters = (group: Group, values: readonly (string | null)[]): RumFilter[] =>
  group.fields.map((field, index): RumFilter => {
    const value = values[index];

    return value === null || value === undefined ? { field, op: 'missing', value: '' } : { field, op: '=', value };
  });

/** The most groups the table lists: the ones with the most sessions; the reader orders the ones that are there. */
export const GROUP_ROWS = 100;

/** Each group with how many frustrated actions of each kind there were in it, and in how many sessions. */
export function tableQuery(scope: OverviewScope, range: OverviewRange, group: Group, narrowing: Narrowing): RumAggregateParams {
  return {
    ...read(scope, range, narrowing),
    groupBy: [...group.fields],
    measures: [...KINDS.map((kind) => count(kind, ofKind(kind))), sessionsOf('sessions')],
    orderBy: 'sessions',
    order: 'desc',
    limit: GROUP_ROWS,
  };
}

export interface Row {
  /** What the group is called in an address: its values, as a list. */
  key: string;
  /** The values of the columns the table is of; none where the actions have none. */
  values: Array<string | null>;
  rage: number;
  dead: number;
  error: number;
  sessions: number;
}

export const keyOf = (values: readonly (string | null)[]): string => JSON.stringify(values);

export function readRows(answer: RumAggregate | undefined): Row[] {
  if (!answer) return [];

  return answer.rows.flatMap((row): Row[] => {
    const found = named(answer, row);

    if (found.sessions === null || found.sessions === undefined) return [];

    return [
      {
        key: keyOf(row.group),
        values: row.group.map((value) => value || null),
        rage: found.rage ?? 0,
        dead: found.dead ?? 0,
        error: found.error ?? 0,
        sessions: found.sessions,
      },
    ];
  });
}

/** The kinds a row has actions of, the commonest first. */
export const kindsOfRow = (row: Row): Array<{ kind: Kind; actions: number }> =>
  KINDS.map((kind) => ({ kind, actions: row[kind] }))
    .filter((entry) => entry.actions > 0)
    .sort((a, b) => b.actions - a.actions);

// ---------------------------------------------------------------------------------------------
// How the table is ordered.

export type SortKey = 'sessions' | Kind;

export const SORTS: readonly SortKey[] = ['sessions', ...KINDS];

/** By the sessions a sign touched: a person who clicks ten times is not ahead of ten who clicked once. */
export const DEFAULT_SORT: SortKey = 'sessions';

export const parseSort = (value: string | null): SortKey => SORTS.find((sort) => sort === value) ?? DEFAULT_SORT;

export type Direction = 'asc' | 'desc';

export const parseDirection = (value: string | null): Direction | undefined => (value === 'asc' || value === 'desc' ? value : undefined);

/** The order a column reads in when none is chosen: the most first. */
export const defaultDirection = (): Direction => 'desc';

export function sortRows(rows: readonly Row[], sort: SortKey, direction: Direction): Row[] {
  return [...rows].sort((a, b) => {
    const x = a[sort];
    const y = b[sort];

    if (x === y) return b.sessions - a.sessions;

    return direction === 'asc' ? x - y : y - x;
  });
}

// ---------------------------------------------------------------------------------------------
// What a group is made of.

export const SHARE_ROWS = 4;

/** The sessions the signs of a group were in, bar by bar of the range; the bars begin where the frame does. */
export function trendQuery(
  scope: OverviewScope,
  bars: { binMicros: number; frame: { start: number; end: number } },
  narrowing: Narrowing,
  group: RumFilter[],
): RumAggregateParams {
  return {
    ...read(scope, { from: bars.frame.start, to: bars.frame.end }, narrowing, group),
    measures: [sessionsOf('sessions')],
    bucketTime: bars.binMicros,
    originTime: bars.frame.start,
  };
}

/** The commonest values of a column among the actions of a group, with the sessions each was in. */
export function sharesQuery(scope: OverviewScope, range: OverviewRange, narrowing: Narrowing, group: RumFilter[], field: string): RumAggregateParams {
  return {
    ...read(scope, range, narrowing, group),
    groupBy: [field],
    measures: [sessionsOf('sessions')],
    orderBy: 'sessions',
    order: 'desc',
    limit: SHARE_ROWS,
  };
}

export interface Share {
  /** The value; none where the actions have none. */
  value: string | null;
  sessions: number;
}

export function readShares(answer: RumAggregate | undefined): Share[] {
  if (!answer) return [];

  return answer.rows.flatMap((row): Share[] => {
    const sessions = named(answer, row).sessions;

    return sessions === null || sessions === undefined ? [] : [{ value: row.group[0] || null, sessions }];
  });
}

/** The page most of the sessions of a group were on, when it was most of them. */
export function mainPage(shares: readonly Share[], total: number): string | null {
  const top = shares.find((share) => share.value !== null);

  return top && top.value !== null && total > 0 && top.sessions / total >= 0.5 ? top.value : null;
}

// ---------------------------------------------------------------------------------------------
// The sessions to look at.

/** How many sessions the panel shows, and how many frustrated actions are read to find them: the newest first. */
export const SAMPLES = 3;
export const SAMPLE_ROWS = 60;

/** How long before the first sign of a session its replay begins: long enough to see what the person was doing. */
export const LEAD_IN_MICROS = 5_000_000;

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;

/** The columns of the actions read for the sessions: what they were, who did them, and when the session began. */
export const SAMPLE_COLUMNS = [
  'session.id',
  'session.start',
  'session.has_replay',
  'action.id',
  TYPE_FIELD,
  'action.error.count',
  'usr.id',
  'usr.anonymous_id',
] as const;

/** The newest frustrated actions of a group, to tell the sessions they were in. */
export function samplesQuery(scope: OverviewScope, range: OverviewRange, narrowing: Narrowing, group: RumFilter[]): RumSearchParams {
  return { ...read(scope, range, narrowing, group), columns: [...SAMPLE_COLUMNS], limit: SAMPLE_ROWS };
}

export interface Sample {
  session: string;
  /** When the first of its signs that was read happened, in microseconds. */
  at: number;
  /** Where its replay begins, in microseconds: a little before the sign, and not before the session did. */
  replayAt: number;
  /** The same, in milliseconds from the beginning of the session; none where the row does not say when it began. */
  offset: number | null;
  /** The person, by the id the application knows them by; an anonymous visitor by the id the SDK keeps. */
  user: string | null;
  anonymous: boolean;
  /** The signs it showed in this group, the commonest first. */
  kinds: Array<{ kind: Kind; count: number }>;
  /** Some of them ended in an error. */
  withError: boolean;
  replay: boolean;
}

const text = (row: RumRow, field: string): string | null => {
  const value = row[field];

  return typeof value === 'string' && value.trim() !== '' ? value : null;
};

/** The kinds a column of types names: a list of words, or the list written out. */
export function kindsOf(value: unknown): Kind[] {
  const words = ((): unknown[] => {
    if (Array.isArray(value)) return value;
    if (typeof value !== 'string') return [];

    try {
      const parsed: unknown = JSON.parse(value);

      return Array.isArray(parsed) ? parsed : [value];
    } catch {
      return [value];
    }
  })();

  return words.flatMap((word) => {
    const kind = typeof word === 'string' ? KINDS.find((candidate) => word.startsWith(PREFIX[candidate])) : undefined;

    return kind ? [kind] : [];
  });
}

/**
 * The sessions the newest signs were in, the newest first, each told by its first sign among the ones
 * read. A session that has none of its own start is replayed from five seconds before the sign.
 */
export function readSamples(rows: readonly RumRow[], limit = SAMPLES): Sample[] {
  const sessions = new Map<string, RumRow[]>();

  for (const row of rows) {
    const id = text(row, SESSION);

    if (id !== null) sessions.set(id, [...(sessions.get(id) ?? []), row]);
  }

  return [...sessions]
    .slice(0, limit)
    .map(([session, own]): Sample => {
      const first = own.reduce((a, b) => (b.timestamp < a.timestamp ? b : a));
      const at = first.timestamp;
      const began = numeric(first['session.start']);
      const beganMicros = began === null ? null : began * 1_000;
      const replayAt = beganMicros === null ? at - LEAD_IN_MICROS : Math.max(beganMicros, at - LEAD_IN_MICROS);
      const tally = new Map<Kind, number>();

      for (const row of own) for (const kind of kindsOf(row[TYPE_FIELD])) tally.set(kind, (tally.get(kind) ?? 0) + 1);

      const person = text(first, 'usr.id');
      const visitor = text(first, 'usr.anonymous_id');

      return {
        session,
        at,
        replayAt,
        offset: beganMicros === null ? null : Math.round((replayAt - beganMicros) / 1_000),
        user: person ?? visitor,
        anonymous: person === null && visitor !== null,
        kinds: [...tally].map(([kind, total]) => ({ kind, count: total })).sort((a, b) => b.count - a.count),
        withError: own.some((row) => (numeric(row['action.error.count']) ?? 0) > 0),
        replay: own.some((row) => row['session.has_replay'] === true),
      };
    });
}

/**
 * Where a session is opened: the page looks for it a day either side of the sign, and a replay opens
 * at the moment the sample says.
 */
export function sessionPath(sample: Pick<Sample, 'session' | 'at' | 'replayAt' | 'replay'>): string {
  const params = new URLSearchParams({
    from: new Date(Math.floor((sample.at - DAY_MICROS) / 1_000)).toISOString(),
    to: new Date(Math.floor((sample.at + DAY_MICROS) / 1_000)).toISOString(),
  });

  if (sample.replay) params.set('at', String(Math.round(sample.replayAt)));

  return `/rum/sessions/view/${encodeURIComponent(sample.session)}?${params}`;
}
