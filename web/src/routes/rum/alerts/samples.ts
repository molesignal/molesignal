import type { RumRow, RumSearchParams } from '@/api/rum';
import type { AlertRule, Incident, RumAlertMetric, RumAlertSearch } from '@/types/alerting';

import { LEAD_IN_MICROS, sessionPath } from '../frustration/model';

/** How many sessions an incident is told by. */
export const SAMPLES = 3;

const SECOND = 1_000_000;
const DAY = 24 * 60 * 60 * SECOND;

/**
 * What an incident of a RUM rule is about: the rule's own figure while the rule is known, else what
 * the labels the rule's editor put on the incident say. The labels cannot say which figure it was.
 */
export interface IncidentFigure {
  application: string;
  environment: string | null;
  page: string | null;
  version: string | null;
  metric: RumAlertMetric | null;
  /** The name of what the application measures, for a figure of its own. */
  customName?: string | null;
  /** The explorer query of a rule made from the explorer. */
  search?: RumAlertSearch | null;
}

export function incidentFigure(incident: Pick<Incident, 'labels'>, rule: Pick<AlertRule, 'query'> | undefined): IncidentFigure | null {
  const query = rule?.query?.rum;

  if (query) {
    return {
      application: query.applicationId,
      environment: query.environment ?? null,
      page: query.page ?? null,
      version: query.version ?? null,
      metric: query.metric,
      ...(query.customName ? { customName: query.customName } : {}),
      ...(query.search ? { search: query.search } : {}),
    };
  }

  const labels = incident.labels ?? {};
  const application = labels['rum.application'];

  if (!application) return null;

  return {
    application,
    environment: labels['deployment.environment'] ?? null,
    page: labels['rum.page'] ?? null,
    version: labels['rum.version'] ?? null,
    metric: null,
  };
}

/** One session an incident was told by, as the incident kept it when it opened. */
export interface SessionSample {
  session: string;
  /** When the thing that made it a sample happened, in microseconds. */
  at: number;
  /** The figure in this session, in the unit of the figure, where there is one. */
  value: number | null;
  /** What went with it: the element, the message, the request. */
  detail: string | null;
}

/**
 * The sessions of an incident: the rows of the sample it kept that name one, each session once, in
 * the order they were found, at most three.
 */
export function sessionSamples(incident: Pick<Incident, 'triggeringQuery'>, limit: number = SAMPLES): SessionSample[] {
  const seen = new Set<string>();
  const found: SessionSample[] = [];

  for (const row of incident.triggeringQuery?.sampleValues ?? []) {
    const session = row.labels?.['rum.session_id']?.trim();

    if (!session || seen.has(session)) continue;

    seen.add(session);

    const raw = row.labels['rum.value'];
    const value = raw === undefined || raw.trim() === '' ? Number.NaN : Number(raw);
    const detail = row.labels['rum.detail']?.trim();

    found.push({ session, at: row.ts, value: Number.isFinite(value) ? value : null, detail: detail ? detail : null });

    if (found.length === limit) break;
  }

  return found;
}

/** The columns of a session that say who it was and whether it can be replayed. */
export const SESSION_COLUMNS = [
  'session.id',
  'session.has_replay',
  'usr.id',
  'usr.name',
  'usr.email',
  'usr.anonymous_id',
  'browser.name',
  'browser.version',
  'os.name',
] as const;

/** The read of the sessions an incident was told by: a day before the first of the samples is as early as any of them began. */
export function sessionsQuery(samples: readonly SessionSample[]): RumSearchParams {
  const times = samples.map((sample) => sample.at);

  return {
    type: 'sessions',
    from: Math.min(...times) - DAY,
    to: Math.max(...times) + SECOND,
    filters: [{ field: 'session.id', op: 'in', value: JSON.stringify(samples.map((sample) => sample.session)) }],
    columns: [...SESSION_COLUMNS],
    limit: samples.length,
  };
}

/** What a row of a session says of who it was. */
export interface SessionPerson {
  /** A name, an address, an id, or the id the SDK keeps of a visitor who never signed in. */
  user: string | null;
  anonymous: boolean;
  browser: string | null;
  system: string | null;
  /** Whether the session was recorded. */
  replay: boolean;
  /** When the session began, in microseconds. */
  began: number | null;
}

const text = (row: RumRow, field: string): string | null => {
  const value = row[field];

  return typeof value === 'string' && value.trim() !== '' ? value.trim() : null;
};

export function readSession(row: RumRow | undefined): SessionPerson {
  if (!row) return { user: null, anonymous: false, browser: null, system: null, replay: false, began: null };

  const person = text(row, 'usr.name') ?? text(row, 'usr.email') ?? text(row, 'usr.id');
  const visitor = text(row, 'usr.anonymous_id');
  const browser = [text(row, 'browser.name'), text(row, 'browser.version')].filter(Boolean).join(' ');

  return {
    user: person ?? visitor,
    anonymous: person === null && visitor !== null,
    browser: browser || null,
    system: text(row, 'os.name'),
    replay: row['session.has_replay'] === true,
    began: Number.isFinite(row.timestamp) ? row.timestamp : null,
  };
}

/**
 * Where a sample session opens: its replay a little before the moment that made it a sample, and
 * not before the session began, when it was recorded; else the session itself.
 */
export function sampleSessionPath(sample: SessionSample, person: SessionPerson): { to: string; offset: number | null } {
  const replayAt = person.began === null ? sample.at - LEAD_IN_MICROS : Math.max(person.began, sample.at - LEAD_IN_MICROS);

  return {
    to: sessionPath({ session: sample.session, at: sample.at, replayAt, replay: person.replay }),
    offset: person.replay && person.began !== null ? Math.round((replayAt - person.began) / 1_000) : null,
  };
}
