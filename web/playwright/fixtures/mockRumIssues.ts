/**
 * The issues of the mock organization: the errors its rows hold, grouped by the id of their issue,
 * and what people have done about them. The numbers of a range are counted from the rows, the state
 * is kept apart, and a list joins the two, as the server's does.
 *
 * The state lives in the mock server of one test, so what a test changes is gone with it.
 */
import type { RumFilter, RumFrame, RumIssue, RumIssueDetail, RumIssueRow, RumIssueSample, RumIssueStack, RumRow } from '../../src/api/rum';
import { issueIdOf, MOCK_RUM } from './mockRumData';

const DAY = 24 * 60 * 60 * 1_000_000;
const TREND_BARS = 12;
const CANDIDATES = 250;

export class Invalid extends Error {}

type Holds = (row: RumRow, filter: RumFilter) => boolean;

const text = (value: unknown): string => (typeof value === 'string' ? value : '');

/** The first frame of the application's own code, as `file:line · function`. */
function placeOf(row: RumRow): string {
  const frames = (row['error.frames'] as Array<Record<string, unknown>> | undefined) ?? [];
  const own = frames.find((frame) => !text(frame.original_file ?? frame.file).includes('node_modules'));

  if (own) {
    const file = text(own.original_file ?? own.file);
    const line = own.original_line ?? own.line;

    return `${file}${line ? `:${String(line)}` : ''} · ${text(own.original_function ?? own.function)}`;
  }

  return text(row['error.resource.url']) ? `${text(row['error.resource.method'])} ${new URL(text(row['error.resource.url'])).pathname}`.trim() : '';
}

/** The issues the rows hold, all open, first and last seen as the rows say; then what people did about three of them. */
export function seedIssues(errors: readonly RumRow[], now: number): Map<string, RumIssue> {
  const issues = new Map<string, RumIssue>();
  const oldest = [...errors].sort((a, b) => a.timestamp - b.timestamp);

  for (const row of oldest) {
    const id = text(row['error.issue_id']);
    const version = text(row.version) || null;
    const known = issues.get(id);

    if (known) {
      known.lastSeenAt = row.timestamp;
      known.lastVersion = version ?? known.lastVersion;
      continue;
    }

    issues.set(id, {
      id,
      applicationId: text(row['application.id']),
      status: 'open',
      assigneeId: null,
      teamId: null,
      overflow: false,
      source: text(row['error.source']),
      errorType: text(row['error.type']),
      message: text(row['error.message']),
      location: placeOf(row),
      firstSeenAt: row.timestamp,
      lastSeenAt: row.timestamp,
      firstVersion: version,
      lastVersion: version,
      resolvedAt: null,
      resolvedVersion: null,
      resolvedBy: null,
      regressedAt: null,
      regressedVersion: null,
      regressedEnvironment: null,
      updatedBy: null,
      updatedAt: row.timestamp,
    });
  }

  const web = MOCK_RUM.applications.web;
  const change = (type: string, message: string, changes: Partial<RumIssue>) => {
    const found = issues.get(issueIdOf(web, type, message));

    if (found) Object.assign(found, changes);
  };

  change('GatewayTimeout', 'Payment provider timeout', { status: 'investigating', assigneeId: 'u3', teamId: 'team-payments', updatedBy: 'u1', updatedAt: now - 2 * 3_600_000_000 });

  // Resolved in 2.3.9 three days ago, and back in 2.4.0.
  const failed = issues.get(issueIdOf(web, 'ServerError', 'Failed to fetch /api/orders: 500 Internal Server Error'));

  if (failed) {
    Object.assign(failed, {
      status: 'regressed',
      assigneeId: 'u2',
      teamId: 'team-platform',
      firstSeenAt: now - 6 * DAY,
      firstVersion: '2.3.9',
      resolvedAt: now - 3 * DAY,
      resolvedVersion: '2.3.9',
      resolvedBy: 'u1',
      regressedAt: failed.lastSeenAt,
      regressedVersion: '2.4.0',
      regressedEnvironment: 'production',
      updatedBy: null,
      updatedAt: failed.lastSeenAt,
    } satisfies Partial<RumIssue>);
  }

  return issues;
}

const asNumber = (value: unknown, fallback: number) => (value === undefined || value === null || value === '' ? fallback : Number(value));

function list<T>(value: unknown): T[] {
  if (Array.isArray(value)) return value as T[];
  if (typeof value === 'string' && value !== '') return JSON.parse(value) as T[];

  return [];
}

/** The rows of the errors a read counts: the application, what else it names, and the range. */
function counted(rows: readonly RumRow[], q: Record<string, unknown>, holds: Holds, from: number, to: number): RumRow[] {
  const filters: RumFilter[] = [
    { field: 'application.id', op: '=', value: String(q.application) },
    { field: 'error.issue_id', op: '!=', value: '' },
    ...(q.env ? [{ field: 'env', op: '=', value: String(q.env) } as RumFilter] : []),
    ...(q.source ? [{ field: 'source', op: '=', value: String(q.source) } as RumFilter] : []),
    ...list<RumFilter>(q.filters),
  ];

  return rows.filter((row) => row.timestamp >= from && row.timestamp < to).filter((row) => filters.every((filter) => holds(row, filter)));
}

const person = (row: RumRow) => text(row['usr.id']) || text(row['usr.anonymous_id']);

const UNRESOLVED = new Set(['open', 'investigating', 'regressed']);

/** `GET /rum/issues`. */
export function listAnswer(rows: readonly RumRow[], issues: ReadonlyMap<string, RumIssue>, q: Record<string, unknown>, holds: Holds, now: number) {
  if (!q.application) throw new Invalid('application is required');

  const to = asNumber(q.to, now);
  const from = asNumber(q.from, to - DAY);
  const found = counted(rows, q, holds, from, to);

  const byIssue = new Map<string, RumRow[]>();
  for (const row of found) byIssue.set(text(row['error.issue_id']), [...(byIssue.get(text(row['error.issue_id'])) ?? []), row]);

  const numbers = (list_: RumRow[]) => ({
    sessions: new Set(list_.map((row) => text(row['session.id']))).size,
    users: new Set(list_.map(person)).size,
    occurrences: list_.length,
  });

  const status = String(q.status ?? 'unresolved');
  const sort = String(q.sort ?? 'sessions');
  const search = String(q.search ?? '').trim().toLowerCase();
  const assignee = String(q.assignee ?? '');
  const team = String(q.team ?? '');
  const bucket = Math.max(1_000, Math.ceil((to - from) / TREND_BARS));
  const parts = Math.max(1, Math.ceil((to - from) / bucket));

  const candidates = [...byIssue]
    .map(([id, own]) => ({ id, own, ...numbers(own) }))
    .sort((a, b) => b.sessions - a.sessions || a.id.localeCompare(b.id))
    .slice(0, CANDIDATES);

  const matches = (issue: RumIssue) => {
    if (status === 'unresolved' ? !UNRESOLVED.has(issue.status) : status !== 'all' && issue.status !== status) return false;
    if (assignee === 'none' ? issue.assigneeId !== null : assignee !== '' && issue.assigneeId !== assignee) return false;
    if (team === 'none' ? issue.teamId !== null : team !== '' && issue.teamId !== team) return false;
    if (search && ![issue.errorType, issue.message, issue.location].some((part) => part.toLowerCase().includes(search))) return false;
    if (String(q.fresh) === 'true' && !(issue.firstSeenAt >= from || (issue.status === 'regressed' && (issue.regressedAt ?? 0) >= from))) return false;

    return true;
  };

  const rank: Record<string, number> = { regressed: 0, open: 1, investigating: 2, resolved: 3, ignored: 4 };
  const descending = q.order === undefined || q.order === '' ? sort !== 'status' : q.order === 'desc';

  const joined = candidates
    .flatMap((candidate) => {
      const issue = issues.get(candidate.id);

      return issue && matches(issue) ? [{ issue, candidate }] : [];
    })
    .sort((a, b) => {
      const key = (entry: { issue: RumIssue; candidate: { sessions: number; users: number; occurrences: number } }) =>
        ({
          sessions: entry.candidate.sessions,
          users: entry.candidate.users,
          occurrences: entry.candidate.occurrences,
          last_seen: entry.issue.lastSeenAt,
          first_seen: entry.issue.firstSeenAt,
          status: rank[entry.issue.status] ?? 9,
        })[sort] ?? 0;
      const order = descending ? key(b) - key(a) : key(a) - key(b);

      return order || b.candidate.sessions - a.candidate.sessions || a.issue.id.localeCompare(b.issue.id);
    });

  const limit = Math.min(100, Math.max(1, asNumber(q.limit, 50)));

  const shown: RumIssueRow[] = joined.slice(0, limit).map(({ issue, candidate }) => {
    const trend = new Array<number>(parts).fill(0);

    for (const row of candidate.own) {
      const at = Math.floor((row.timestamp - from) / bucket);
      if (at >= 0 && at < parts) trend[at] = (trend[at] ?? 0) + 1;
    }

    return { ...issue, sessions: candidate.sessions, users: candidate.users, occurrences: candidate.occurrences, trend };
  });

  return { issues: shown, total: joined.length };
}

/** The frames of an error row as the drawer shows them. */
function framesOf(row: RumRow): RumFrame[] {
  const frames = (row['error.frames'] as Array<Record<string, unknown>> | undefined) ?? [];

  return frames.map((frame) => {
    const restored = frame.original_file !== undefined;
    const place = (file: unknown, fn: unknown, line: unknown, column: unknown) => ({
      file: text(file),
      function: text(fn),
      line: typeof line === 'number' ? line : null,
      column: typeof column === 'number' ? column : null,
    });

    return {
      raw: place(frame.file, frame.function, frame.line, frame.column),
      restored: restored ? place(frame.original_file, frame.original_function, frame.original_line, frame.original_column) : null,
      inApp: !text(restored ? frame.original_file : frame.file).includes('node_modules'),
    };
  });
}

/** `GET /rum/issues/{id}`. */
export function detailAnswer(rows: readonly RumRow[], issue: RumIssue, q: Record<string, unknown>, now: number): RumIssueDetail {
  const to = asNumber(q.to, now);
  const from = asNumber(q.from, to - DAY);
  const own = rows.filter((row) => text(row['error.issue_id']) === issue.id).sort((a, b) => b.timestamp - a.timestamp);
  const inRange = own.filter((row) => row.timestamp >= from && row.timestamp < to);

  // An error that happens again in a session is the same example.
  const sessions = new Set<string>();
  const latest = own.filter((row) => {
    const session = text(row['session.id']);

    return session === '' || (!sessions.has(session) && sessions.add(session) !== undefined);
  });

  const samples: RumIssueSample[] = latest.slice(0, 3).map((row) => ({
    timestamp: row.timestamp,
    sessionId: text(row['session.id']) || null,
    userId: text(row['usr.id']) || null,
    anonymousId: text(row['usr.anonymous_id']) || null,
    page: text(row['view.url_path']) || null,
    browser: [text(row['browser.name']), text(row['browser.version_major'])].filter(Boolean).join(' ') || null,
    system: [text(row['os.name']), text(row['os.version'])].filter(Boolean).join(' ') || null,
    version: text(row.version) || null,
    hasReplay: row['session.has_replay'] === true,
    retained: row['retention.filter'] !== 'errors',
  }));

  const framed = own.find((row) => Array.isArray(row['error.frames'])) ?? own[0];
  const stack: RumIssueStack | null = framed
    ? {
        message: text(framed['error.message']) || null,
        text: text(framed['error.stack']) || null,
        frames: framesOf(framed),
        symbolication: ((framed['error.symbolication'] as { status?: RumIssueStack['symbolication'] } | undefined)?.status ?? null) as RumIssueStack['symbolication'],
        handling: text(framed['error.handling']) || null,
        isCrash: framed['error.is_crash'] === true,
        category: text(framed['error.category']) || null,
      }
    : null;

  return {
    ...issue,
    sessions: new Set(inRange.map((row) => text(row['session.id']))).size,
    users: new Set(inRange.map(person)).size,
    occurrences: inRange.length,
    samples,
    stack,
  };
}

/** `PATCH /rum/issues/{id}`: what a person sets; resolving takes the newest version the application has. */
export function applyChange(issue: RumIssue, body: Record<string, unknown>, newest: string | null, now: number): RumIssue {
  const status = body.status as RumIssue['status'] | undefined;

  if (status === ('regressed' as string)) throw new Invalid('`regressed` is set by the rules when a resolved issue is seen again, not by a person');
  if (status === undefined && !('assigneeId' in body) && !('teamId' in body)) throw new Invalid('name a status, an assignee or a team to change');

  if ('assigneeId' in body) issue.assigneeId = body.assigneeId ? String(body.assigneeId) : null;
  if ('teamId' in body) issue.teamId = body.teamId ? String(body.teamId) : null;

  if (status !== undefined) {
    issue.status = status;
    issue.resolvedAt = status === 'resolved' ? now : null;
    issue.resolvedVersion = status === 'resolved' ? newest : null;
    issue.resolvedBy = status === 'resolved' ? 'u1' : null;
    issue.regressedAt = null;
    issue.regressedVersion = null;
    issue.regressedEnvironment = null;
  }

  issue.updatedBy = 'u1';
  issue.updatedAt = now;

  return issue;
}
