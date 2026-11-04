/**
 * The answers of the analytics tab, worked out from the rows of the mock organization by the rules
 * the server's statements follow (`infra/rum/analytics`): a funnel counts a session for a step by the
 * first row that meets the step after the session got to the step before; paths take the pages of a
 * session in the order they were opened, a page opened again at once counting once; retention tells
 * a person by their user id or the anonymous id and a period is a day or a Monday week of UTC.
 */
import type { RumRow, RumSchema } from '../../src/api/rum';

export interface Filter {
  field: string;
  op: string;
  value?: string;
}

export class Invalid extends Error {}

type Kind = 'views' | 'actions';

/** Whether a row of a kind meets a filter. */
export type Holds = (kind: Kind | 'sessions', row: RumRow, filter: Filter) => boolean;

const MIN_STEPS = 2;
const MAX_STEPS = 8;
const GROUPS = 10;
const PER_LEVEL = 8;
const MAX_DEPTH = 6;
const MAX_PERIODS = 30;
const OTHER = '__other__';
const DAY = 86_400_000_000;

const PAGE_FIELDS = ['view.url_path_group', 'view.url_path', 'view.name'];

interface Step {
  type: Kind;
  name?: string;
  filters?: Filter[];
}

interface Asked {
  from: number;
  to: number;
  filters: Filter[];
}

const present = (value: unknown) => value !== undefined && value !== null && value !== '';

const checked = ({ from, to }: Asked) => {
  if (!(to > from)) throw new Invalid('range end must be greater than start');
};

/** The rows of a kind in the range that belong to a session and meet the filters. */
function events(rows: readonly RumRow[], kind: Kind, asked: Asked, own: readonly Filter[], holds: Holds): RumRow[] {
  return rows.filter(
    (row) =>
      row.timestamp >= asked.from &&
      row.timestamp < asked.to &&
      present(row['session.id']) &&
      [...asked.filters, ...own].every((filter) => holds(kind, row, filter)),
  );
}

const bySession = (rows: readonly RumRow[]) => {
  const sessions = new Map<string, RumRow[]>();

  for (const row of rows) {
    const id = String(row['session.id']);

    sessions.set(id, [...(sessions.get(id) ?? []), row]);
  }

  return sessions;
};

const middle = (values: number[]): number | null => {
  if (values.length === 0) return null;

  const sorted = [...values].sort((a, b) => a - b);
  const half = Math.floor(sorted.length / 2);

  return sorted.length % 2 === 1 ? (sorted[half] as number) : ((sorted[half - 1] as number) + (sorted[half] as number)) / 2;
};

const share = (part: number, whole: number) => (whole > 0 ? part / whole : null);

const label = (step: Step, index: number) => step.name?.trim() || step.filters?.[0]?.value?.trim() || `#${index + 1}`;

export interface FunnelAsk extends Asked {
  steps: Step[];
  windowSecs?: number | undefined;
  breakdown?: string | undefined;
}

export function funnelAnswer(
  rows: Record<Kind, RumRow[]>,
  ask: FunnelAsk,
  holds: Holds,
  schemaOf: (kind: Kind) => RumSchema,
) {
  const { steps } = ask;

  if (steps.length < MIN_STEPS || steps.length > MAX_STEPS) throw new Invalid(`a funnel has ${MIN_STEPS} to ${MAX_STEPS} steps`);
  checked(ask);
  if (ask.windowSecs !== undefined && !(ask.windowSecs > 0)) throw new Invalid('the window of a funnel must be positive');

  const breakdown = ask.breakdown?.trim() || undefined;

  if (breakdown) {
    const field = schemaOf((steps[0] as Step).type).fields.find((column) => column.name === breakdown);

    if (!field) throw new Invalid(`\`${breakdown}\` is not a field of ${(steps[0] as Step).type}`);
    if (field.dataType !== 'utf8') throw new Invalid(`\`${breakdown}\` has no text to tell sessions apart by`);
  }

  const found = steps.map((step) => bySession(events(rows[step.type], step.type, ask, step.filters ?? [], holds)));
  const first = found[0] as Map<string, RumRow[]>;

  // For each session that did the first step: the moments of the steps it got through, and the value to count it by.
  const reached: Array<{ times: number[]; group: string | null }> = [];

  for (const [session, entries] of first) {
    const earliest = [...entries].sort((a, b) => a.timestamp - b.timestamp)[0] as RumRow;
    const times = [earliest.timestamp];
    const limit = ask.windowSecs === undefined ? Number.POSITIVE_INFINITY : earliest.timestamp + ask.windowSecs * 1_000_000;

    for (const step of found.slice(1)) {
      const later = (step.get(session) ?? [])
        .map((row) => row.timestamp)
        .filter((at) => at > (times[times.length - 1] as number) && at <= limit)
        .sort((a, b) => a - b)[0];

      if (later === undefined) break;

      times.push(later);
    }

    const value = breakdown ? earliest[breakdown] : undefined;

    reached.push({ times, group: breakdown && present(value) ? String(value) : null });
  }

  const countFor = (group: typeof reached) =>
    steps.map((step, index) => {
      const at = group.filter((entry) => entry.times.length > index);
      const counts = at.length;
      const first = group.length;
      const before = index > 0 ? group.filter((entry) => entry.times.length >= index).length : 0;
      const gaps = at.map((entry) => ((entry.times[index] as number) - (entry.times[index - 1] as number)) / 1_000);

      return {
        name: label(step, index),
        sessions: counts,
        conversion: share(counts, first),
        fromPrevious: index > 0 ? share(counts, before) : null,
        medianMs: index > 0 ? middle(gaps) : null,
      };
    });

  if (!breakdown) return { from: ask.from, to: ask.to, groups: [{ value: null, steps: countFor(reached) }] };

  const groups = new Map<string, typeof reached>();

  for (const entry of reached) {
    const key = entry.group ?? '\u0000';

    groups.set(key, [...(groups.get(key) ?? []), entry]);
  }

  return {
    from: ask.from,
    to: ask.to,
    groups: [...groups]
      .sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]))
      .slice(0, GROUPS)
      .map(([key, group]) => ({ value: key === '\u0000' ? null : key, steps: countFor(group) })),
  };
}

export interface PathsAsk extends Asked {
  anchor?: string | undefined;
  backward?: boolean | undefined;
  depth?: number | undefined;
  by?: string | undefined;
}

export function pathsAnswer(views: RumRow[], ask: PathsAsk, holds: Holds) {
  checked(ask);

  const depth = ask.depth ?? 3;
  const backward = ask.backward === true;
  const by = ask.by ?? PAGE_FIELDS[0];

  if (depth < 1 || depth > MAX_DEPTH) throw new Invalid(`paths go 1 to ${MAX_DEPTH} pages from the page they start at`);
  if (!PAGE_FIELDS.includes(by as string)) throw new Invalid(`pages are told by ${PAGE_FIELDS.join(', ')}`);

  const rows = events(views, 'views', ask, [], holds).filter((row) => present(row[by as string]));

  // The pages of each session in the order they were opened, one for a page opened again at once.
  const nodes = new Map<string, number>();
  const links = new Map<string, number>();
  const column = (at: number) => (backward ? depth - at : at);
  const anchor = ask.anchor?.trim() || undefined;

  for (const entries of bySession(rows).values()) {
    const pages = entries
      .sort((a, b) => a.timestamp - b.timestamp)
      .map((row) => String(row[by as string]))
      .filter((page, index, all) => index === 0 || all[index - 1] !== page);

    const positions = pages.flatMap((page, index) => (anchor === undefined || page === anchor ? [index] : []));

    if (positions.length === 0) continue;

    const origin = backward ? Math.max(...positions) : Math.min(...positions);
    const reached = pages
      .map((page, index) => ({ page, depth: backward ? origin - index : index - origin }))
      .filter((entry) => entry.depth >= 0 && entry.depth <= depth);

    for (const entry of reached) {
      const key = JSON.stringify([entry.depth, entry.page]);

      nodes.set(key, (nodes.get(key) ?? 0) + 1);
    }

    for (const entry of reached) {
      const next = reached.find((other) => other.depth === entry.depth + 1);

      if (!next) continue;

      const key = JSON.stringify([entry.depth, entry.page, next.page]);

      links.set(key, (links.get(key) ?? 0) + 1);
    }
  }

  // The most visited pages of each column keep their names, the rest are one.
  const sessions = new Map<string, number>();

  for (const [key, count] of nodes) {
    const [at, page] = JSON.parse(key) as [number, string];
    const slot = JSON.stringify([column(at), page]);

    sessions.set(slot, (sessions.get(slot) ?? 0) + count);
  }

  const kept = new Map<string, string>();

  for (let level = 0; level <= depth; level += 1) {
    const pages = [...sessions]
      .map(([key, count]) => ({ key, count, at: (JSON.parse(key) as [number, string])[0], page: (JSON.parse(key) as [number, string])[1] }))
      .filter((entry) => entry.at === level)
      .sort((a, b) => b.count - a.count || a.page.localeCompare(b.page));

    pages.forEach((entry, rank) => kept.set(entry.key, rank < PER_LEVEL ? entry.page : OTHER));
  }

  const named = (level: number, page: string) => kept.get(JSON.stringify([level, page])) ?? OTHER;

  const merged = new Map<string, number>();

  for (const [key, count] of sessions) {
    const [level, page] = JSON.parse(key) as [number, string];
    const slot = JSON.stringify([level, named(level, page)]);

    merged.set(slot, (merged.get(slot) ?? 0) + count);
  }

  const joined = new Map<string, number>();

  for (const [key, count] of links) {
    const [at, source, target] = JSON.parse(key) as [number, string, string];

    if (at < 0 || at >= depth) continue;

    // Going backward a link leaves the page that came earlier, further from the anchor.
    const [fromColumn, fromPage, toColumn, toPage] = backward ? [column(at + 1), target, column(at), source] : [column(at), source, column(at + 1), target];
    const slot = JSON.stringify([fromColumn, named(fromColumn, fromPage), named(toColumn, toPage)]);

    joined.set(slot, (joined.get(slot) ?? 0) + count);
  }

  const through = (level: number, page: string) =>
    [...joined]
      .map(([key, count]) => ({ link: JSON.parse(key) as [number, string, string], count }))
      .filter(({ link }) => (backward ? link[0] + 1 === level && link[2] === page : link[0] === level && link[1] === page))
      .reduce((sum, { count }) => sum + count, 0);

  const columns = [...merged]
    .map(([key, count]) => {
      const [level, page] = JSON.parse(key) as [number, string];

      return { column: level, page, sessions: count, ends: Math.max(0, count - through(level, page)) };
    })
    .sort((a, b) => a.column - b.column || Number(a.page === OTHER) - Number(b.page === OTHER) || b.sessions - a.sessions || a.page.localeCompare(b.page));

  const anchored = backward ? depth : 0;

  return {
    from: ask.from,
    to: ask.to,
    columns: depth + 1,
    sessions: columns.filter((node) => node.column === anchored).reduce((sum, node) => sum + node.sessions, 0),
    backward,
    nodes: columns,
    links: [...joined]
      .map(([key, count]) => {
        const [level, source, target] = JSON.parse(key) as [number, string, string];

        return { column: level, source, target, sessions: count };
      })
      .sort((a, b) => a.column - b.column || a.source.localeCompare(b.source) || a.target.localeCompare(b.target)),
  };
}

export interface RetentionAsk extends Asked {
  period?: 'day' | 'week' | undefined;
  periods?: number | undefined;
}

export function retentionAnswer(sessions: RumRow[], ask: RetentionAsk, holds: Holds) {
  checked(ask);

  const period = ask.period ?? 'day';
  const periods = ask.periods ?? 7;

  if (periods < 1 || periods > MAX_PERIODS) throw new Invalid(`retention reads 1 to ${MAX_PERIODS} periods after the first`);

  const index = (micros: number) => (period === 'day' ? Math.floor(micros / DAY) : Math.floor((Math.floor(micros / DAY) + 3) / 7));
  const start = (at: number) => (period === 'day' ? at * DAY : (at * 7 - 3) * DAY);
  const length = period === 'day' ? DAY : 7 * DAY;

  // The periods each person was seen in.
  const seen = new Map<string, Set<number>>();

  for (const row of sessions.filter(
    (candidate) =>
      candidate.timestamp >= ask.from && candidate.timestamp < ask.to && ask.filters.every((filter) => holds('sessions', candidate, filter)),
  )) {
    const person = present(row['usr.id']) ? String(row['usr.id']) : present(row['usr.anonymous_id']) ? String(row['usr.anonymous_id']) : '';

    if (person === '') continue;

    seen.set(person, (seen.get(person) ?? new Set()).add(index(row.timestamp)));
  }

  const cohorts = new Map<number, number[]>();

  for (const active of seen.values()) {
    const first = Math.min(...active);
    const slots = cohorts.get(first) ?? Array.from({ length: periods + 1 }, () => 0);

    for (const at of active) {
      if (at - first <= periods) slots[at - first] = (slots[at - first] as number) + 1;
    }

    cohorts.set(first, slots);
  }

  return {
    from: ask.from,
    to: ask.to,
    period,
    periods,
    cohorts: [...cohorts]
      .sort((a, b) => a[0] - b[0])
      .map(([at, slots]) => ({
        startTime: start(at),
        users: slots[0] as number,
        returned: Array.from({ length: periods }, (_, k) => (start(at) + (k + 2) * length <= ask.to ? (slots[k + 1] as number) : null)),
      })),
  };
}
