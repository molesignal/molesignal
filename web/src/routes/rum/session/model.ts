import type { RumRow } from '@/api/rum';

const count = (row: RumRow, name: string): number => {
  const value = row[name];
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
};

const text = (row: RumRow, name: string): string | undefined => {
  const value = row[name];
  return typeof value === 'string' && value.trim() !== '' ? value : undefined;
};

/** What went wrong in a session, as the server counted it from the views. */
export interface SessionIssues {
  errors: number;
  frustrations: number;
  crashes: number;
  longTasks: number;
}

export function sessionIssues(row: RumRow): SessionIssues {
  return {
    errors: count(row, 'session.error.count'),
    frustrations: count(row, 'session.frustration.count'),
    crashes: count(row, 'session.crash.count'),
    longTasks: count(row, 'session.long_task.count'),
  };
}

export function issueTotal(issues: SessionIssues): number {
  return issues.errors + issues.frustrations + issues.crashes + issues.longTasks;
}

/** What names a session first: an error it ran into, a frustration it showed, or neither. */
export function sessionHeadline(row: RumRow): 'error' | 'frustration' | 'plain' {
  const issues = sessionIssues(row);

  if (issues.errors + issues.crashes > 0) return 'error';
  return issues.frustrations > 0 ? 'frustration' : 'plain';
}

/** The page a session is told by: where it ended, else where it began. */
export function sessionPage(row: RumRow): string | undefined {
  return text(row, 'session.exit_view.url_path') ?? text(row, 'session.landing_view.url_path');
}

/** The person the session was of: a name, an address, an id, or nobody known. */
export function sessionUser(row: RumRow): string | undefined {
  return text(row, 'usr.name') ?? text(row, 'usr.email') ?? text(row, 'usr.id') ?? text(row, 'usr.anonymous_id');
}

/** The stretch of time the session ran, for the links that look at what else happened then. */
export function sessionWindow(row: RumRow | null | undefined): { from: string; to: string } | undefined {
  const start = row?.timestamp;
  if (typeof start !== 'number' || !Number.isFinite(start) || start <= 0) return undefined;

  const duration = row ? count(row, 'session.duration') : 0;

  return {
    from: new Date(Math.floor(start / 1_000)).toISOString(),
    to: new Date(Math.floor((start + duration / 1_000) / 1_000)).toISOString(),
  };
}

/** The range an address asks the session to be looked for in (`from` and `to` as instants), if it names one. */
export function addressRange(params: URLSearchParams): { from: number; to: number } | undefined {
  const from = Date.parse(params.get('from') ?? '');
  const to = Date.parse(params.get('to') ?? '');

  if (!Number.isFinite(from) || !Number.isFinite(to) || to <= from) return undefined;

  return { from: from * 1_000, to: to * 1_000 };
}

/** Where an address asks the replay to open: a moment in microseconds since the epoch (`at`), given here in milliseconds. */
export function addressReplayAt(params: URLSearchParams): number | undefined {
  const at = Number(params.get('at'));

  return Number.isFinite(at) && at > 0 ? at / 1_000 : undefined;
}
