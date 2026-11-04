import type { TFunction } from 'i18next';

/**
 * The columns that have a name of their own in the table's head, by field name. The key is a
 * message of `explorer.columns`; any other column is headed by the name of its field, which is
 * also what a filter and the Code editor call it.
 */
const COLUMN_LABEL_KEYS: Record<string, string> = {
  'usr.id': 'user',
  'view.url_path': 'page',
  'session.landing_view.url_path': 'landing_page',
  'session.duration': 'duration',
  'session.view.count': 'views',
  'session.action.count': 'actions',
  'session.error.count': 'errors',
  'session.id': 'session_id',
  'browser.name': 'browser',
  'geo.country_iso_code': 'country',
  'view.loading_time': 'loading_time',
  'view.performance.lcp.timestamp': 'lcp',
  'view.performance.inp.duration': 'inp',
  'view.performance.cls.score': 'cls',
  'view.error.count': 'errors',
  'view.id': 'view_id',
  'action.type': 'type',
  'action.target.name': 'target',
  'action.frustration.type': 'frustration',
  'action.loading_time': 'loading_time',
  'action.id': 'action_id',
  'error.message': 'message',
  'error.type': 'error_type',
  'error.source': 'source',
  'error.handling': 'handling',
  version: 'version',
  'error.id': 'error_id',
  'resource.method': 'method',
  'resource.url': 'url',
  'resource.status_code': 'status',
  'resource.duration': 'duration',
  'resource.size': 'size',
  'resource.type': 'resource_type',
  'resource.id': 'resource_id',
  'long_task.duration': 'duration',
  'long_task.entry_type': 'entry_type',
  'long_task.blocking_duration': 'blocking',
  'long_task.id': 'long_task_id',
};

/** What a column is headed with. */
export function columnLabel(t: TFunction<'rum'>, name: string): string {
  const key = COLUMN_LABEL_KEYS[name];
  return key ? t(`explorer.columns.${key}`) : name;
}
