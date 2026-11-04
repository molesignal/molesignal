import type { RumField, RumRow } from '@/api/rum';

/** The tabs of the explorer, in order: what real users did, from the whole session down to a request. */
export const RUM_TABS = ['sessions', 'views', 'actions', 'errors', 'resources', 'long_tasks', 'logs'] as const;

export type RumTab = (typeof RUM_TABS)[number];

export const DEFAULT_RUM_TAB: RumTab = 'sessions';

export function rumTabOf(value: string | null | undefined): RumTab | null {
  return RUM_TABS.find((tab) => tab === value) ?? null;
}

/** The column that names one row of a type, as the backend says in its schema. */
export const ID_FIELD: Record<RumTab, string> = {
  sessions: 'session.id',
  views: 'view.id',
  actions: 'action.id',
  errors: 'error.id',
  resources: 'resource.id',
  long_tasks: 'long_task.id',
  logs: 'log.id',
};

/**
 * The columns a table opens with, in this order, as far as the schema has them. The time a row
 * happened at is always the first column and is not listed.
 */
const TABLE_COLUMNS: Record<RumTab, readonly string[]> = {
  sessions: [
    'usr.id',
    'session.landing_view.url_path',
    'session.duration',
    'session.view.count',
    'session.action.count',
    'session.error.count',
    'browser.name',
    'geo.country_iso_code',
    'session.id',
  ],
  views: [
    'view.url_path',
    'view.loading_time',
    'view.performance.lcp.timestamp',
    'view.performance.inp.duration',
    'view.performance.cls.score',
    'view.error.count',
    'usr.id',
    'browser.name',
    'view.id',
  ],
  actions: [
    'action.type',
    'action.target.name',
    'view.url_path',
    'action.frustration.type',
    'action.loading_time',
    'usr.id',
    'action.id',
  ],
  errors: [
    'error.message',
    'error.type',
    'error.source',
    'view.url_path',
    'error.handling',
    'version',
    'usr.id',
    'error.id',
  ],
  resources: [
    'resource.method',
    'resource.url',
    'resource.status_code',
    'resource.duration',
    'resource.size',
    'resource.type',
    'view.url_path',
    'resource.id',
  ],
  long_tasks: [
    'long_task.duration',
    'long_task.entry_type',
    'long_task.blocking_duration',
    'view.url_path',
    'usr.id',
    'long_task.id',
  ],
  logs: ['level', 'message', 'log.logger', 'log.origin', 'view.url_path', 'usr.id', 'log.id'],
};

/** The columns a table opens with. */
export function defaultTableColumns(tab: RumTab): readonly string[] {
  return TABLE_COLUMNS[tab];
}

/**
 * The columns of a table: the reader's choice, or else the type's own; only those the schema
 * lists, in the order they were chosen.
 */
export function tableColumns(
  tab: RumTab,
  fields: readonly RumField[],
  chosen: readonly string[] | null,
): RumField[] {
  const byName = new Map(fields.map((field) => [field.name, field]));

  return (chosen ?? TABLE_COLUMNS[tab]).flatMap((name) => {
    const field = byName.get(name);
    return field ? [field] : [];
  });
}

/** What a row is called in a head: the most telling of its columns, else its id. */
export function rowTitle(tab: RumTab, row: RumRow): string {
  const text = (name: string) => {
    const value = row[name];
    return typeof value === 'string' && value.trim() !== '' ? value.trim() : null;
  };
  const id = text(ID_FIELD[tab]) ?? '';

  switch (tab) {
    case 'sessions':
      return text('usr.name') ?? text('usr.email') ?? text('usr.id') ?? text('usr.anonymous_id') ?? id;
    case 'views':
      return text('view.name') ?? text('view.url_path') ?? id;
    case 'actions':
      return text('action.target.name') ?? text('action.type') ?? id;
    case 'errors':
      return text('error.message') ?? text('error.type') ?? id;
    case 'resources':
      return [text('resource.method'), text('resource.url')].filter(Boolean).join(' ') || id;
    case 'long_tasks':
      return text('view.url_path') ?? id;
    case 'logs':
      return text('message') ?? id;
  }
}

/** The id of a row, as the type names it. */
export function rowId(tab: RumTab, row: RumRow): string {
  const value = row[ID_FIELD[tab]];
  return typeof value === 'string' ? value : value == null ? '' : String(value);
}

/**
 * Whether a row is one of the troubled the histogram counts apart: the session or view with an
 * error, the action with a frustration signal, the request that failed. As the server counts them
 * (`trouble` in the query module of the backend); a row that lacks the column counts as fine.
 */
export function isTroubled(tab: RumTab, row: RumRow): boolean {
  const count = (name: string) => {
    const value = row[name];
    return typeof value === 'number' ? value : 0;
  };

  switch (tab) {
    case 'sessions':
      return count('session.error.count') > 0;
    case 'views':
      return count('view.error.count') > 0;
    case 'actions':
      return count('action.frustration.count') > 0;
    case 'resources':
      return typeof row['resource.status_code'] === 'number' && (count('resource.status_code') >= 400 || count('resource.status_code') === 0);
    case 'logs':
      return row.level === 'error';
    case 'errors':
    case 'long_tasks':
      return false;
  }
}
