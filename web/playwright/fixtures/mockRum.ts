/**
 * The RUM backend, as the explorer, the session page and the applications page see it: the
 * applications of the mock organization, and the rows ten visits left (see `mockRumData`),
 * searched, counted and opened by the same rules as the server's.
 *
 * The field catalogue is the one the backend serves (`data/rum-schema.json`, made from
 * `crates/core/domain/src/domain/rum/event`). The applications live in the mock server of one test,
 * so what a test creates, changes or deletes is gone with it. Every time is relative to the mock
 * session's frozen clock.
 */
import { Buffer } from 'node:buffer';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { Express, Request, Response } from 'express';

import type { RetentionFilter, RumApplication, RumPlatform, RumRow, RumSchema, RumType } from '../../src/api/rum';
import { countsAnswer } from './histogramCounts';
import { funnelAnswer, Invalid as AnalyticsInvalid, pathsAnswer, retentionAnswer } from './mockRumAnalytics';
import { aggregateAnswer, Invalid } from './mockRumAggregate';
import { MOCK_RUM, mockRumData, mockRumEveryRow } from './mockRumData';
import { clicksAnswer, scrollAnswer } from './mockRumHeatmaps';
import { applyChange, detailAnswer, Invalid as IssueInvalid, listAnswer, seedIssues } from './mockRumIssues';
import { registerRumNoteRoutes } from './mockRumNotes';
import { Refusal as PlaylistRefusal, registerPlaylistRoutes } from './mockRumPlaylists';

const DAY = 24 * 60 * 60 * 1_000_000;

/** What an application is made with when its creation names no settings. */
const DEFAULT_APPLICATION_SETTINGS: RumApplication['settings'] = {
  collectIp: true,
  collectGeo: true,
  defaultPrivacy: 'mask',
  issuesPerHour: 100,
  scrub: ['secrets', 'emails', 'phones', 'cards', 'national_ids', 'url_params'],
  sessionSampleRate: 100,
  retentionFilters: [],
  keepErrors: true,
  sdk: {},
};

/** The filters as the server stores them: a filter that has no id is named by one made from its name. */
function namedFilters(filters: RetentionFilter[]): RetentionFilter[] {
  const taken = new Set(filters.map((filter) => filter.id).filter(Boolean));

  return filters.map((filter) => {
    if (filter.id) return filter;

    const slug = filter.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'filter';
    let id = slug;

    for (let number = 2; taken.has(id) || id === 'default' || id === 'errors'; number += 1) id = `${slug}-${number}`;

    taken.add(id);

    return { ...filter, id };
  });
}
/** The settings as the server stores them. */
const settingsOf = (settings: RumApplication['settings']): RumApplication['settings'] => ({
  ...settings,
  retentionFilters: namedFilters(settings.retentionFilters),
});
const HERE = dirname(fileURLToPath(import.meta.url));
const SCHEMAS = JSON.parse(readFileSync(join(HERE, 'data', 'rum-schema.json'), 'utf8')) as Record<RumType, RumSchema>;

const TYPES = Object.keys(SCHEMAS) as RumType[];
/** What a row of a type is called in a timeline. */
const EVENT: Record<RumType, string> = {
  sessions: 'session',
  views: 'view',
  actions: 'action',
  errors: 'error',
  resources: 'resource',
  long_tasks: 'long_task',
  vitals: 'vital',
  logs: 'log',
};
const TIMELINE: readonly RumType[] = ['views', 'actions', 'errors', 'resources', 'long_tasks', 'vitals', 'logs'];

/** The columns free text is looked for in. */
const TEXT_COLUMNS: Record<RumType, readonly string[]> = {
  sessions: ['session.id', 'usr.id', 'usr.name', 'usr.email', 'session.landing_view.url_path', 'session.exit_view.url_path'],
  views: ['view.url', 'view.name', 'view.id', 'usr.id', 'usr.email'],
  actions: ['action.target.name', 'view.url_path', 'usr.id'],
  errors: ['error.message', 'error.type', 'view.url_path', 'usr.id'],
  resources: ['resource.url', 'view.url_path'],
  long_tasks: ['view.url_path', 'usr.id'],
  vitals: ['vital.name', 'view.url_path', 'usr.id'],
  logs: ['message', 'log.logger', 'view.url_path', 'usr.id'],
};

/** What a bar of the histogram counts apart from the rest. */
const TROUBLED: Partial<Record<RumType, (row: RumRow) => boolean>> = {
  sessions: (row) => Number(row['session.error.count'] ?? 0) > 0,
  views: (row) => Number(row['view.error.count'] ?? 0) > 0,
  actions: (row) => Number(row['action.frustration.count'] ?? 0) > 0,
  resources: (row) => {
    const status = Number(row['resource.status_code'] ?? 1);
    return status >= 400 || status === 0;
  },
  logs: (row) => row.level === 'error',
};

interface Filter {
  field: string;
  op: string;
  value?: string;
}

interface Asked {
  type: RumType;
  from: number;
  to: number;
  filters: Filter[];
  freeText: string[];
  columns: string[];
  limit: number;
  offset: number;
}

class Refused extends Error {
  constructor(
    message: string,
    readonly status = 400,
  ) {
    super(message);
  }
}

const list = <T>(value: unknown): T[] =>
  Array.isArray(value) ? (value as T[]) : typeof value === 'string' && value !== '' ? (JSON.parse(value) as T[]) : [];

const typeOf = (value: unknown): RumType => {
  const found = TYPES.find((type) => type === value);
  if (!found) throw new Refused(`\`${String(value)}\` is not a kind of RUM row`, 404);

  return found;
};

/** What a read asked for, whichever way it was sent; a cursor stands for the read it continues. */
function askedOf(req: Request): Asked {
  const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;

  if (typeof source.cursor === 'string' && source.cursor !== '') {
    const kept = JSON.parse(Buffer.from(source.cursor, 'base64url').toString('utf8')) as Asked;
    return { ...kept, limit: Number(source.limit ?? kept.limit) };
  }

  return {
    type: typeOf(source.type),
    from: Number(source.from ?? 0),
    to: Number(source.to ?? Number.MAX_SAFE_INTEGER),
    filters: list<Filter>(source.filters),
    freeText: list<string>(source.freeText),
    columns: list<string>(source.columns),
    limit: Number(source.limit ?? 100),
    offset: 0,
  };
}

const present = (value: unknown) => value !== undefined && value !== null && value !== '';

/** The list of values an `in` filter holds: a JSON list of one to a hundred. */
function values(value: string): string[] {
  let items: unknown;
  try {
    items = JSON.parse(value);
  } catch {
    throw new Refused('`in` takes a JSON list of values');
  }
  if (!Array.isArray(items) || items.length === 0 || items.length > 100) throw new Refused('`in` takes between 1 and 100 values');

  return items.map(String);
}

/** Whether a row satisfies a filter, by the operators the backend knows. */
function holds(row: RumRow, filter: Filter, schema: RumSchema): boolean {
  const field = schema.fields.find((column) => column.name === filter.field.trim()) ?? memberOf(filter.field.trim(), schema);
  if (!field) throw new Refused(`\`${filter.field}\` is not a field of ${schema.stream}`);

  const raw = field.name in row ? row[field.name] : memberValue(row, field.name);
  const value = (filter.value ?? '').trim();
  const equals = (candidate: string) =>
    field.dataType === 'int64' || field.dataType === 'float64'
      ? Number(raw) === Number(candidate)
      : String(raw) === candidate;

  switch (filter.op.trim().toLowerCase()) {
    case 'exists':
      return present(raw);
    case 'missing':
    case 'not_exists':
      return !present(raw);
    case '=':
    case 'eq':
      return value === '' ? !present(raw) : present(raw) && equals(value);
    case '!=':
    case 'ne':
      return value === '' ? present(raw) : !(present(raw) && equals(value));
    case 'in':
      return present(raw) && values(value).some(equals);
    case 'not_in':
    case 'nin':
      return !(present(raw) && values(value).some(equals));
    case 'contains':
    case 'like':
    case 'match':
    case 'match_text':
      return present(raw) && String(raw).toLowerCase().includes(value.toLowerCase());
    case '>':
    case 'gt':
      return present(raw) && Number(raw) > Number(value);
    case '>=':
    case 'gte':
      return present(raw) && Number(raw) >= Number(value);
    case '<':
    case 'lt':
      return present(raw) && Number(raw) < Number(value);
    case '<=':
    case 'lte':
      return present(raw) && Number(raw) <= Number(value);
    default:
      throw new Refused(`unsupported filter operator \`${filter.op}\``);
  }
}

/** The JSON documents whose keys can be asked for by name, as the server has them. */
const DOCUMENTS = ['feature_flags', 'context', 'view.custom_timings', 'usr.extra'];

/** A key of a document a stream has, as a filter or a group names it. */
function memberOf(name: string, schema: RumSchema): RumSchema['fields'][number] | undefined {
  const document = DOCUMENTS.find((parent) => name.startsWith(`${parent}.`) && schema.fields.some((field) => field.name === parent));

  return document ? { name, dataType: document === 'view.custom_timings' ? 'int64' : 'utf8', unit: document === 'view.custom_timings' ? 'ns' : 'none', facetable: true, indexed: false } : undefined;
}

/** What a key of a document holds in a row, as text unless it is a number. */
export function memberValue(row: RumRow, name: string): unknown {
  const document = DOCUMENTS.find((parent) => name.startsWith(`${parent}.`));

  if (!document) return row[name];

  const held = (row[document] as Record<string, unknown> | undefined)?.[name.slice(document.length + 1)];

  return typeof held === 'boolean' ? String(held) : held;
}

/** Whether filters ask about what ran a session, or for sessions by name; else a read is of the sessions people ran. */
const asksAboutTraffic = (filters: readonly Filter[]) =>
  filters.some(
    (filter) =>
      filter.field === 'session.type' ||
      filter.field.startsWith('synthetics.') ||
      filter.field.startsWith('ci_test.') ||
      (filter.field === 'session.id' && (filter.op === '=' || filter.op === 'in')),
  );

/** The rows a read matches, newest first. */
function matching(data: Record<RumType, RumRow[]>, asked: Asked): RumRow[] {
  const schema = SCHEMAS[asked.type];

  return data[asked.type]
    .filter((row) => row.timestamp >= asked.from && row.timestamp < asked.to)
    .filter((row) => asked.filters.every((filter) => holds(row, filter, schema)))
    .filter((row) =>
      asked.freeText.every((term) =>
        TEXT_COLUMNS[asked.type].some((column) => String(row[column] ?? '').toLowerCase().includes(term.trim().toLowerCase())),
      ),
    );
}

/** A row with the keys of its documents that an aggregate names, as columns of their own. */
function withMembers(row: RumRow, source: Record<string, unknown>): RumRow {
  const named = [...list<string>(source.groupBy), ...list<{ field?: string }>(source.measures).map((measure) => measure.field ?? '')];
  const members = named.filter((name) => DOCUMENTS.some((parent) => name.startsWith(`${parent}.`)));

  return members.length === 0 ? row : { ...row, ...Object.fromEntries(members.map((name) => [name, memberValue(row, name)])) };
}

/** A row with the columns asked for, and the one that names it. */
function pick(row: RumRow, columns: readonly string[]): RumRow {
  const picked: RumRow = { timestamp: row.timestamp };
  for (const column of columns) if (column in row) picked[column] = row[column];

  return picked;
}

const whole = (type: RumType, row: RumRow) => pick(row, SCHEMAS[type].fields.map((field) => field.name));

/** The columns of a row in a session's timeline: the defaults without what the session says once. */
function timelineColumns(type: RumType): string[] {
  const dimension = /^(application\.id|service|env|version|source|usr\.|browser\.|os\.|device\.|geo\.|network\.)/;

  return SCHEMAS[type].defaultColumns.filter((name) => !dimension.test(name));
}

/**
 * The recording of a visit that has one: a page of one heading and a button, a pointer that finds it and
 * clicks, and then the pointer now and then until the visit ended, so that the recording is as long as the visit was.
 */
function recording(session: RumRow) {
  const start = Number(session['session.start']);
  const lasted = Math.max(5_000, Math.round(Number(session['session.duration']) / 1_000_000));
  const url = String(session['session.landing_view.url']);
  const text = (id: number, textContent: string) => ({ type: 3, textContent, id });
  const element = (id: number, tagName: string, childNodes: unknown[] = []) => ({ type: 2, tagName, attributes: {}, childNodes, id });
  const pointer = (at: number, x: number, y: number, id = 8) => ({ type: 3, timestamp: start + at, data: { source: 1, positions: [{ x, y, id, timeOffset: 0 }] } });

  const page = {
    type: 0,
    id: 1,
    childNodes: [
      { type: 1, name: 'html', publicId: '', systemId: '', id: 2 },
      element(3, 'html', [
        element(4, 'head', [element(5, 'style', [{ ...text(6, 'body{font-family:sans-serif;margin:32px}button{margin-top:16px;padding:8px 16px}'), isStyle: true }])]),
        element(7, 'body', [element(8, 'h1', [text(9, String(session['session.landing_view.name']))]), element(10, 'button', [text(11, 'Continue')])]),
      ]),
    ],
  };

  return [
    { type: 4, timestamp: start, data: { href: url, width: 1280, height: 720 } },
    { type: 2, timestamp: start + 1, data: { node: page, initialOffset: { top: 0, left: 0 } } },
    pointer(1_500, 160, 140, 10),
    pointer(2_500, 96, 118, 10),
    { type: 3, timestamp: start + 3_000, data: { source: 2, type: 2, id: 10, x: 96, y: 118 } },
    pointer(5_000, 300, 300),
    ...Array.from({ length: Math.max(0, Math.floor((lasted - 5_000) / 20_000)) }, (_, k) => pointer(5_000 + (k + 1) * 20_000 - 1_000, 120 + ((k * 37) % 400), 120 + ((k * 53) % 300))),
    pointer(lasted, 300, 300),
  ];
}

function seedApplications(now: number): RumApplication[] {
  const settings = DEFAULT_APPLICATION_SETTINGS;

  return [
    { id: MOCK_RUM.applications.web, name: 'Storefront', platform: 'browser', settings, createdBy: 'u1', createdAt: now - 40 * DAY, updatedAt: now - 40 * DAY },
    { id: MOCK_RUM.applications.app, name: 'Storefront app', platform: 'flutter', settings: { ...settings, collectIp: false }, createdBy: 'u1', createdAt: now - 12 * DAY, updatedAt: now - 3 * DAY },
  ];
}

const slug = (name: string) =>
  name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 64);

const refuse = (res: Response, error: unknown) => {
  if (error instanceof PlaylistRefusal) {
    res.status(error.status).json({ error: error.status === 404 ? 'not_found' : error.status === 409 ? 'conflict' : 'invalid', message: error.message });

    return;
  }

  if (!(error instanceof Refused)) throw error;

  res.status(error.status).json({ error: error.status === 404 ? 'not_found' : 'invalid', message: error.message });
};

/** Registers the RUM routes on the mock server of one test. */
export function registerRumRoutes(app: Express, nowIso: string): void {
  const now = Date.parse(nowIso) * 1_000;
  const every = mockRumEveryRow(now);
  // What is read of an application is of the sessions people ran, unless a read asks about the others.
  const data = mockRumData(now);
  const applications = seedApplications(now);

  const guarded = (handler: (req: Request, res: Response) => void) => (req: Request, res: Response) => {
    try {
      handler(req, res);
    } catch (error) {
      refuse(res, error);
    }
  };

  app.get('/api/v1/rum/applications', (_req, res) => res.json(applications));

  // The sessions people have put aside to go through one after the other.
  registerPlaylistRoutes(app, {
    now,
    applications: () => applications.map((application) => application.id),
    guarded,
    seed: [
      {
        name: 'Checkout failures',
        description: 'Sessions that ended at the checkout with an error.',
        application: MOCK_RUM.applications.web,
        ago: 3 * 60 * 60 * 1_000_000,
        sessions: [
          { id: MOCK_RUM.session.checkout, at: (data.sessions.find((row) => row['session.id'] === MOCK_RUM.session.checkout)?.timestamp ?? 0) + 5_000_000 },
          { id: MOCK_RUM.session.frustrated },
          { id: 'ffffffff-0000-4000-8000-000000000000' },
        ],
      },
      { name: 'Slow product pages', application: MOCK_RUM.applications.web, ago: 26 * 60 * 60 * 1_000_000, sessions: [] },
    ],
  });

  app.post(
    '/api/v1/rum/applications',
    guarded((req, res) => {
      const body = req.body as { name?: string; id?: string; platform?: RumPlatform; settings?: RumApplication['settings'] };
      const name = (body.name ?? '').trim();
      if (!name) throw new Refused('an application needs a name');

      const id = (body.id ?? '').trim() || slug(name);
      if (applications.some((application) => application.id === id || application.name === name)) {
        throw new Refused('an application with this id or name already exists', 409);
      }

      const created: RumApplication = {
        id,
        name,
        platform: body.platform ?? 'browser',
        settings: settingsOf({ ...DEFAULT_APPLICATION_SETTINGS, ...body.settings }),
        createdBy: 'u1',
        createdAt: now,
        updatedAt: now,
      };
      applications.push(created);
      res.status(201).json(created);
    }),
  );

  const applicationOf = (req: Request) => {
    const found = applications.find((application) => application.id === req.params.id);
    if (!found) throw new Refused(`application \`${String(req.params.id)}\` does not exist`, 404);

    return found;
  };

  app.get('/api/v1/rum/applications/:id', guarded((req, res) => res.json(applicationOf(req))));

  app.patch(
    '/api/v1/rum/applications/:id',
    guarded((req, res) => {
      const found = applicationOf(req);
      const body = req.body as Partial<Pick<RumApplication, 'name' | 'platform' | 'settings'>>;
      Object.assign(found, body, { settings: settingsOf({ ...found.settings, ...body.settings }), updatedAt: now });
      res.json(found);
    }),
  );

  app.delete(
    '/api/v1/rum/applications/:id',
    guarded((req, res) => {
      applications.splice(applications.indexOf(applicationOf(req)), 1);
      res.status(204).end();
    }),
  );

  app.get(
    '/api/v1/auth/tokens/rum',
    guarded((req, res) => {
      const id = String(req.query.applicationId ?? '');
      if (!applications.some((application) => application.id === id)) throw new Refused(`application \`${id}\` does not exist`, 404);

      res.json({
        id: `tok-rum-${id}`,
        prefix: '8f3a2c9d41',
        token: `mspat-8f3a2c9d41e6b075a9d2c4e8f1b30a76ab${id.length.toString(16).padStart(2, '0')}`,
        permissions: ['rum.write'],
        tokenKind: 'rum_client',
        applicationId: id,
        createdAt: now - 30 * DAY,
      });
    }),
  );

  // The intake takes whatever it is sent: what the datasource guide sends is not added to the rows.
  app.post('/api/v1/rum/events', (req, res) => {
    const events = Array.isArray(req.body) ? req.body : [req.body];

    res.json({ accepted: events.length, rejected: [] });
  });

  app.get('/api/v1/rum/schema/:type', guarded((req, res) => res.json(SCHEMAS[typeOf(req.params.type)])));

  const search = guarded((req, res) => {
    const asked = askedOf(req);
    const schema = SCHEMAS[asked.type];
    const found = matching(of(asked), asked);
    const columns = asked.columns.length > 0 ? asked.columns : schema.defaultColumns;

    for (const column of columns) {
      if (!schema.fields.some((field) => field.name === column)) throw new Refused(`\`${column}\` is not a field of ${schema.stream}`);
    }

    const page = found.slice(asked.offset, asked.offset + asked.limit);
    const cursor = (offset: number) => Buffer.from(JSON.stringify({ ...asked, offset })).toString('base64url');
    const more = asked.offset + asked.limit < found.length;

    res.json({
      items: page.map((row) => pick(row, [...columns, schema.idField])),
      hasMore: more,
      nextCursor: more ? cursor(asked.offset + asked.limit) : null,
      previousCursor: asked.offset > 0 ? cursor(Math.max(0, asked.offset - asked.limit)) : null,
    });
  });
  app.get('/api/v1/rum/search', search);
  app.post('/api/v1/rum/search', search);

  const facets = guarded((req, res) => {
    const asked = askedOf(req);
    const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;
    const field =
      SCHEMAS[asked.type].fields.find((column) => column.name === source.field) ?? memberOf(String(source.field), SCHEMAS[asked.type]);
    if (!field?.facetable) throw new Refused(`\`${String(source.field)}\` cannot be counted by value`);

    // The kinds of session are read of every kind: else the other kinds could not be found.
    const counted = new Map<string, number>();
    for (const row of matching(field.name === 'session.type' ? every : of(asked), asked)) {
      const value = memberValue(row, field.name);
      if (present(value)) counted.set(String(value), (counted.get(String(value)) ?? 0) + 1);
    }

    const values = [...counted]
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .slice(0, Number(source.limit ?? 10))
      .map(([value, count]) => ({ value, count }));

    res.json({ field: field.name, values });
  });
  app.get('/api/v1/rum/facets', facets);
  app.post('/api/v1/rum/facets', facets);

  const histogram = guarded((req, res) => {
    const asked = askedOf(req);
    const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;
    const troubled = TROUBLED[asked.type];
    const events = matching(of(asked), asked).map((row) => ({ micros: row.timestamp, error: troubled?.(row) === true }));

    res.json({
      ...countsAnswer(
        {
          from: asked.from,
          to: asked.to,
          bucketTime: Number(source.bucketTime),
          originTime: Number(source.originTime),
        },
        { events },
      ),
      split: troubled !== undefined,
    });
  });
  app.get('/api/v1/rum/histogram', histogram);
  app.post('/api/v1/rum/histogram', histogram);

  const aggregate = guarded((req, res) => {
    const asked = askedOf(req);
    const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;
    const schema = SCHEMAS[asked.type];
    const number = (value: unknown) => (present(value) ? Number(value) : undefined);

    try {
      res.json(
        aggregateAnswer(
          matching(of(asked), asked).map((row) => withMembers(row, source)),
          schema,
          {
            measures: list(source.measures),
            groupBy: list<string>(source.groupBy),
            bucketTime: number(source.bucketTime),
            originTime: number(source.originTime),
            orderBy: present(source.orderBy) ? String(source.orderBy) : undefined,
            order: present(source.order) ? String(source.order) : undefined,
            limit: number(source.limit),
            estimate: present(source.estimate) ? String(source.estimate) !== 'false' : undefined,
          },
          (row, filter) => holds(row, filter, schema),
          { from: asked.from },
        ),
      );
    } catch (error) {
      throw error instanceof Invalid ? new Refused(error.message) : error;
    }
  });
  app.get('/api/v1/rum/aggregate', aggregate);
  app.post('/api/v1/rum/aggregate', aggregate);

  // The statement a dashboard panel runs for a chart of the explorer, and the columns it answers
  // with, as the server names them. What the statement reads is checked against the real engine in
  // Rust; here it only carries the stream and the filters, for a spec to see where they went.
  app.post(
    '/api/v1/rum/panel-statement',
    guarded((req, res) => {
      const body = req.body as Record<string, unknown>;
      const schema = SCHEMAS[typeOf(body.type)];
      const chart = String(body.chart ?? '');
      const measures = list<{ fn: string; field?: string; name?: string }>(body.measures);
      const groups = list<string>(body.groupBy);
      const names = list<string>(body.groupNames);
      const fieldOf = (name: string) => schema.fields.find((field) => field.name === name) ?? memberOf(name, schema);

      if (!['timeseries', 'toplist', 'table', 'distribution'].includes(chart)) throw new Refused(`unknown chart \`${chart}\``);

      for (const name of [...groups, ...measures.flatMap((measure) => (measure.field ? [measure.field] : [])), ...(present(body.field) ? [String(body.field)] : [])]) {
        if (!fieldOf(name)) throw new Refused(`\`${name}\` is not a field of ${schema.stream}`);
      }

      if (chart === 'timeseries' && !present(body.interval)) throw new Refused('a chart over time needs an interval');
      if (chart === 'toplist' && groups.length !== 1) throw new Refused('a top list is of one group');

      const unitOf = (measure: { fn: string; field?: string }) => {
        if (measure.fn === 'count' || measure.fn === 'count_distinct') return 'count';

        const unit = measure.field ? fieldOf(measure.field)?.unit : undefined;

        return unit === 'ns' ? 'ms' : (unit ?? 'none');
      };

      const columns =
        chart === 'distribution'
          ? [
              { name: names[0] ?? String(body.field), role: 'group', unit: 'none' },
              { name: measures[0]?.name ?? 'count', role: 'measure', unit: 'count' },
            ]
          : [
              ...(chart === 'timeseries' ? [{ name: 'time', role: 'time', unit: 'none' }] : []),
              ...groups.map((group, index) => ({ name: names[index] ?? group, role: 'group', unit: 'none' })),
              ...measures.map((measure) => ({
                name: measure.name ?? (measure.field ? `${measure.fn}(${measure.field})` : measure.fn),
                role: 'measure',
                unit: unitOf(measure),
              })),
            ];

      const where = list<Filter>(body.filters).map((filter) => `"${filter.field}" ${filter.op} '${filter.value}'`);
      const relation = schema.versioned ? `${schema.stream}_latest` : schema.stream;
      const bar = chart === 'timeseries' ? `date_bin(INTERVAL '${String(body.interval)}', _timestamp) AS "time", ` : '';

      res.json({
        stream: schema.stream,
        statement: `SELECT ${bar}… FROM "${relation}"${where.length > 0 ? ` WHERE ${where.join(' AND ')}` : ''}`,
        columns,
        limit: chart === 'timeseries' ? 10_000 : chart === 'distribution' ? 100 : Number(body.limit ?? 10),
      });
    }),
  );

  // Funnels, paths and retention over the rows of the organization.
  const analytics = (answer: (source: Record<string, unknown>) => unknown) =>
    guarded((req, res) => {
      const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;

      try {
        res.json(answer(source));
      } catch (error) {
        throw error instanceof AnalyticsInvalid ? new Refused(error.message) : error;
      }
    });

  const asking = (source: Record<string, unknown>) => ({
    from: Number(source.from ?? 0),
    to: Number(source.to ?? Number.MAX_SAFE_INTEGER),
    filters: list<Filter>(source.filters),
  });
  const optional = (value: unknown) => (present(value) ? Number(value) : undefined);
  const heldBy = (kind: RumType, row: RumRow, filter: Filter) => holds(row, filter, SCHEMAS[kind]);

  const funnel = analytics((source) =>
    funnelAnswer(
      data,
      {
        ...asking(source),
        steps: list(source.steps),
        windowSecs: optional(source.windowSecs),
        breakdown: present(source.breakdown) ? String(source.breakdown) : undefined,
      },
      heldBy,
      (kind) => SCHEMAS[kind],
    ),
  );
  app.get('/api/v1/rum/analytics/funnel', funnel);
  app.post('/api/v1/rum/analytics/funnel', funnel);

  const paths = analytics((source) =>
    pathsAnswer(
      data.views,
      {
        ...asking(source),
        anchor: present(source.anchor) ? String(source.anchor) : undefined,
        backward: source.backward === true || source.backward === 'true',
        depth: optional(source.depth),
        by: present(source.by) ? String(source.by) : undefined,
      },
      heldBy,
    ),
  );
  app.get('/api/v1/rum/analytics/paths', paths);
  app.post('/api/v1/rum/analytics/paths', paths);

  const retention = analytics((source) =>
    retentionAnswer(
      data.sessions,
      {
        ...asking(source),
        period: source.period === 'week' ? 'week' : 'day',
        periods: optional(source.periods),
      },
      heldBy,
    ),
  );
  app.get('/api/v1/rum/analytics/retention', retention);
  app.post('/api/v1/rum/analytics/retention', retention);

  const sessionRow = (id: string) => every.sessions.find((row) => row['session.id'] === id);
  const of = (asked: Asked) => (asksAboutTraffic(asked.filters) ? every : data);

  registerRumNoteRoutes(app, sessionRow, () => every.views, now);

  // What the settings of the application decide for an SDK that asks at start.
  app.get('/api/v1/rum/config', (_req, res) => res.json(applications[0]?.settings.sdk ?? {}));

  const keys = guarded((req, res) => {
    const asked = askedOf(req);
    const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;
    const column = String(source.column ?? '');

    if (!DOCUMENTS.includes(column)) throw new Refused(`column must be one of ${DOCUMENTS.join(', ')}`);

    const seen = new Map<string, Set<unknown>>();

    for (const row of matching(of(asked), asked)) {
      for (const key of Object.keys((row[column] as Record<string, unknown> | undefined) ?? {})) {
        seen.set(key, (seen.get(key) ?? new Set()).add(row['session.id']));
      }
    }

    res.json({
      column,
      keys: [...seen]
        .map(([key, sessions]) => ({ key, sessions: sessions.size }))
        .sort((a, b) => b.sessions - a.sessions || a.key.localeCompare(b.key))
        .slice(0, Number(source.limit ?? 100)),
    });
  });
  app.get('/api/v1/rum/keys', keys);
  app.post('/api/v1/rum/keys', keys);

  const distribution = guarded((req, res) => {
    const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;
    const asked = askedOf(req);
    const field = String(source.field ?? '');
    const schema = SCHEMAS[asked.type];
    const known = schema?.fields.find((entry) => entry.name === field);

    if (!known || (known.dataType !== 'int64' && known.dataType !== 'float64')) throw new Refused(`\`${field}\` is not a number of ${asked.type}`);

    const values = matching(of(asked), asked)
      .map((row) => row[field])
      .filter((value): value is number => typeof value === 'number')
      .sort((left, right) => left - right);
    const at = (share: number) => (values.length > 0 ? values[Math.min(values.length - 1, Math.floor(values.length * share))]! : null);
    const low = values[0] ?? null;
    const high = at(0.99);
    const count = Number(source.bars ?? 40);
    const width = low !== null && high !== null && high > low ? (high - low) / count : 0;
    const bars = low === null
      ? []
      : width === 0
        ? [{ from: low, to: values.at(-1) ?? low, rows: values.length }]
        : Array.from({ length: count }, (_, index) => ({
            from: low + index * width,
            to: index === count - 1 ? Math.max(low + (index + 1) * width, values.at(-1) ?? 0) : low + (index + 1) * width,
            rows: values.filter((value) => Math.min(count - 1, Math.floor((value - low) / width)) === index).length,
          }));

    res.json({ field, unit: known.unit, rows: values.length, min: low, max: values.at(-1) ?? null, p50: at(0.5), p75: at(0.75), p95: at(0.95), p99: high, bars });
  });
  app.get('/api/v1/rum/distribution', distribution);
  app.post('/api/v1/rum/distribution', distribution);

  const heatmap = (type: RumType, answer: (rows: RumRow[], limit: number) => unknown) =>
    guarded((req, res) => {
      const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;
      const asked: Asked = {
        type,
        from: Number(source.from ?? 0),
        to: Number(source.to ?? Number.MAX_SAFE_INTEGER),
        filters: list<Filter>(source.filters),
        freeText: [],
        columns: [],
        limit: 0,
        offset: 0,
      };

      res.json({ ...(answer(matching(of(asked), asked), Number(source.limit ?? 100)) as object), from: asked.from, to: asked.to });
    });
  const clicks = heatmap('actions', clicksAnswer);
  const scroll = heatmap('views', scrollAnswer);
  app.get('/api/v1/rum/heatmaps/clicks', clicks);
  app.post('/api/v1/rum/heatmaps/clicks', clicks);
  app.get('/api/v1/rum/heatmaps/scroll', scroll);
  app.post('/api/v1/rum/heatmaps/scroll', scroll);

  const snapshot = guarded((req, res) => {
    const source = (req.method === 'POST' ? req.body : req.query) as Record<string, unknown>;
    const asked: Asked = {
      type: 'views',
      from: Number(source.from ?? 0),
      to: Number(source.to ?? Number.MAX_SAFE_INTEGER),
      filters: list<Filter>(source.filters),
      freeText: [],
      columns: [],
      limit: 0,
      offset: 0,
    };

    const candidates = matching(of(asked), asked)
      .filter((row) => row['session.has_replay'] === true)
      .slice(0, 20)
      .map((row) => ({
        sessionId: String(row['session.id']),
        viewId: String(row['view.id']),
        atTime: (Number(row['session.start']) + 2_000) * 1_000,
        width: row['display.viewport.width'] as number,
        height: row['display.viewport.height'] as number,
      }));

    const chosen = candidates.find((candidate) => candidate.viewId === source.viewId) ?? candidates[0];
    const session = chosen && sessionRow(chosen.sessionId);

    res.json({
      snapshot: chosen && session ? { ...chosen, events: recording(session).filter((event) => event.timestamp <= chosen.atTime / 1_000) } : null,
      candidates,
    });
  });
  app.get('/api/v1/rum/heatmaps/snapshot', snapshot);
  app.post('/api/v1/rum/heatmaps/snapshot', snapshot);

  app.get(
    '/api/v1/rum/sessions/:id/timeline',
    guarded((req, res) => {
      const id = String(req.params.id);
      const session = sessionRow(id);
      if (!session) throw new Refused('session not found', 404);

      const items = TIMELINE.flatMap((type) =>
        every[type]
          .filter((row) => row['session.id'] === id)
          .map((row) => ({ ...pick(row, timelineColumns(type)), type: EVENT[type] })),
      ).sort((a, b) => a.timestamp - b.timestamp);

      res.json({ sessionId: id, session: whole('sessions', session), items, truncated: [] });
    }),
  );

  const traced = data.resources.find((row) => row['resource.trace_id'] === MOCK_RUM.trace);

  // The traces a session carried; or, when it carried none, the ones that ran while it did.
  app.get('/api/v1/rum/sessions/:id/related-traces', (req, res) => {
    const id = String(req.params.id);

    if (traced?.['session.id'] === id) {
      res.json({
        sessionId: id,
        primaryService: 'checkout-api',
        traces: [
          { traceId: MOCK_RUM.trace, service: 'checkout-api', spanCount: 14, durationMs: 482, startedAt: traced.timestamp, relation: 'direct' },
        ],
      });
    } else if (id === MOCK_RUM.session.frustrated) {
      const session = sessionRow(id);
      const at = session?.timestamp ?? now;

      res.json({
        sessionId: id,
        primaryService: 'payments',
        traces: [
          { traceId: '7d5c0e2a91b84f36a0c1e8d4b2f97a63', service: 'payments', spanCount: 9, durationMs: 3_012, startedAt: at + 60_000_000, relation: 'time-correlated' },
          { traceId: 'c3a8f1d27e4b49a6b0d5e2f917a4c836', service: 'inventory', spanCount: 6, durationMs: 96, startedAt: at + 90_000_000, relation: 'time-correlated' },
        ],
      });
    } else {
      res.json({ sessionId: id, primaryService: null, traces: [] });
    }
  });

  app.get('/api/v1/rum/replay/:id', (req, res) => {
    const session = sessionRow(String(req.params.id));
    const recorded = session?.['session.has_replay'] === true;

    res.json({ sessionId: req.params.id, segmentCount: recorded ? 1 : 0, events: session && recorded ? recording(session) : [] });
  });

  app.get('/api/v1/rum/traces/:traceId/session', (req, res) => {
    const found = traced && req.params.traceId === MOCK_RUM.trace ? traced : null;

    res.json({
      traceId: req.params.traceId,
      session: found && {
        sessionId: found['session.id'],
        timestamp: found.timestamp,
        application: found['application.id'],
        page: found['view.url_path'],
        viewId: found['view.id'],
        actionId: null,
        url: found['resource.url'],
      },
    });
  });

  // The issues the errors are grouped into, and what people have done about them.
  const issues = seedIssues(data.errors, now);
  const errorsHold = (row: RumRow, filter: Filter) => holds(row, filter, SCHEMAS.errors);

  const newestVersion = (application: string): string | null => {
    const firsts = new Map<string, number>();

    for (const row of data.sessions) {
      const version = String(row.version ?? '');
      if (row['application.id'] === application && version) firsts.set(version, Math.min(firsts.get(version) ?? Number.POSITIVE_INFINITY, row.timestamp));
    }

    return [...firsts].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
  };

  const issueOf = (req: Request) => {
    const found = issues.get(String(req.params.id));
    if (!found) throw new Refused('RUM issue not found', 404);

    return found;
  };

  const askingIssues = <T>(read: () => T): T => {
    try {
      return read();
    } catch (error) {
      throw error instanceof IssueInvalid ? new Refused(error.message) : error;
    }
  };

  app.get(
    '/api/v1/rum/issues',
    guarded((req, res) => res.json(askingIssues(() => listAnswer(data.errors, issues, req.query as Record<string, unknown>, errorsHold, now)))),
  );

  app.get(
    '/api/v1/rum/issues/:id',
    guarded((req, res) => res.json(detailAnswer(data.errors, issueOf(req), req.query as Record<string, unknown>, now))),
  );

  app.patch(
    '/api/v1/rum/issues/:id',
    guarded((req, res) => {
      const found = issueOf(req);

      res.json(askingIssues(() => applyChange(found, req.body as Record<string, unknown>, newestVersion(found.applicationId), now)));
    }),
  );

  app.get(
    '/api/v1/rum/:type/:id',
    guarded((req, res) => {
      const type = typeOf(req.params.type);
      const row = every[type].find((candidate) => candidate[SCHEMAS[type].idField] === req.params.id);
      if (!row) throw new Refused(`${EVENT[type]} not found`, 404);

      res.json(whole(type, row));
    }),
  );
}
