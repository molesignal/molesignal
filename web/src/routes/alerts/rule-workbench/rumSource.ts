import type { RumAlertPreview } from '@/api/rum';
import type { AlertRule, ComparisonOp, RumAlertMetric, RumAlertQuery, RumAlertSearch, RumAlertStatistic } from '@/types/alerting';
import type { QueryResult } from '@/types/query';

/**
 * The figures a RUM rule can watch, the usual ones first. The server checks every rule against
 * the same table (`domain::rum::RumAlertMetric`): which figures a page or a version can narrow
 * and how many rows a figure needs before it has a value are its rules, said here to shape the
 * form.
 */
export const RUM_METRICS: readonly RumAlertMetric[] = [
  'error_rate',
  'lcp_p75',
  'inp_p75',
  'cls_p75',
  'crash_free_sessions',
  'frustration_rate',
  'failed_request_rate',
  'error_count',
  'new_issues',
  'regressed_issues',
  'issue_impact',
  'ttfb_p75',
  'session_count',
  'view_count',
  'custom_vital',
  'custom_timing',
  'query',
];

/** What can be taken of a figure the application measures itself. */
export const RUM_STATISTICS: readonly RumAlertStatistic[] = ['p75', 'p50', 'p90', 'p95', 'p99', 'avg', 'count'];

/** Whether the figure is one the application measures itself, which a rule names. */
export function isCustomMetric(metric: RumAlertMetric): boolean {
  return metric === 'custom_timing' || metric === 'custom_vital';
}

/** New and regressed issues have no usual level to compare with. */
export const RUM_ANOMALY_METRICS: readonly RumAlertMetric[] = RUM_METRICS.filter((metric) => !isAboutIssues(metric));

/** The windows a RUM rule is evaluated over, in seconds. */
export const RUM_WINDOWS: readonly number[] = [300, 900, 3_600];

export type RumMetricUnit = 'count' | 'percent' | 'milliseconds' | 'score' | 'bytes' | 'none';

export function rumMetricUnit(metric: RumAlertMetric): RumMetricUnit {
  switch (metric) {
    case 'error_rate':
    case 'crash_free_sessions':
    case 'frustration_rate':
    case 'failed_request_rate':
      return 'percent';
    case 'lcp_p75':
    case 'inp_p75':
    case 'ttfb_p75':
    case 'custom_timing':
    case 'custom_vital':
      return 'milliseconds';
    case 'cls_p75':
      return 'score';
    // What a query's number is in is its measure's (`searchUnit`).
    case 'query':
      return 'none';
    default:
      return 'count';
  }
}

/** The query a rule on `query` starts with: a count of every view. */
export function emptySearch(): RumAlertSearch {
  return { type: 'views', statement: '', filters: [], freeText: [], measure: { fn: 'count' } };
}

/** Whether the number of a query adds up, a count or a sum: nought where nothing happened, and no minimum of rows. */
export function searchAddsUp(search: RumAlertSearch | null | undefined): boolean {
  return ['count', 'count_distinct', 'sum'].includes(search?.measure.fn ?? 'count');
}

/** The unit of the number of a query, out of the unit of the field it takes, as the server says it. */
export function searchUnit(search: RumAlertSearch, fieldUnit: string | undefined): RumMetricUnit {
  if (search.measure.fn === 'count' || search.measure.fn === 'count_distinct') return 'count';

  switch (fieldUnit) {
    case 'ns':
    case 'ms':
      return 'milliseconds';
    case 'percent':
    case 'score':
    case 'bytes':
    case 'count':
      return fieldUnit;
    default:
      return 'none';
  }
}

/** The figures about the issues the errors are grouped into, which have an environment but no page and no version. */
export function isAboutIssues(metric: RumAlertMetric): boolean {
  return metric === 'new_issues' || metric === 'regressed_issues';
}

/**
 * Whether the figure can be narrowed to one page: sessions are not on one page, and an issue turns
 * up on many. A query names the page among its filters, and the version too.
 */
export function rumSupportsPage(metric: RumAlertMetric): boolean {
  return !(
    metric === 'session_count' ||
    metric === 'crash_free_sessions' ||
    metric === 'frustration_rate' ||
    metric === 'query' ||
    isAboutIssues(metric)
  );
}

export function rumSupportsVersion(metric: RumAlertMetric): boolean {
  return !isAboutIssues(metric) && metric !== 'query';
}

/** How many rows a figure needs before it has a value, unless the rule says; none for a count. */
export function rumDefaultMinSamples(metric: RumAlertMetric): number {
  switch (metric) {
    case 'error_rate':
    case 'crash_free_sessions':
    case 'frustration_rate':
      return 100;
    case 'failed_request_rate':
      return 200;
    case 'lcp_p75':
    case 'inp_p75':
    case 'cls_p75':
    case 'ttfb_p75':
      return 50;
    case 'custom_timing':
    case 'custom_vital':
    case 'query':
      return 30;
    default:
      return 0;
  }
}

/** What the rows a figure is made of are called, to say how many a figure needs. */
export function rumSampleKind(metric: RumAlertMetric): 'views' | 'sessions' | 'requests' | 'views_with_value' | 'rows' | null {
  switch (metric) {
    case 'error_rate':
      return 'views';
    case 'crash_free_sessions':
    case 'frustration_rate':
      return 'sessions';
    case 'failed_request_rate':
      return 'requests';
    case 'lcp_p75':
    case 'inp_p75':
    case 'cls_p75':
    case 'ttfb_p75':
    case 'custom_timing':
    case 'custom_vital':
      return 'views_with_value';
    case 'query':
      return 'rows';
    default:
      return null;
  }
}

/** The window a figure is best watched over: a percentile or a share needs the rows of a longer stretch. */
export function rumDefaultWindow(metric: RumAlertMetric): number {
  switch (metric) {
    case 'lcp_p75':
    case 'inp_p75':
    case 'cls_p75':
    case 'ttfb_p75':
    case 'frustration_rate':
    case 'crash_free_sessions':
    case 'custom_timing':
    case 'custom_vital':
    case 'issue_impact':
      return 3_600;
    default:
      return 900;
  }
}

/** A threshold to start from for a figure: a line people commonly page on. */
export function rumDefaultThreshold(metric: RumAlertMetric): { operator: ComparisonOp; threshold: number } {
  switch (metric) {
    case 'error_rate':
    case 'failed_request_rate':
      return { operator: 'gt', threshold: 2 };
    case 'lcp_p75':
      return { operator: 'gt', threshold: 2_500 };
    case 'inp_p75':
      return { operator: 'gt', threshold: 200 };
    case 'cls_p75':
      return { operator: 'gt', threshold: 0.1 };
    case 'ttfb_p75':
      return { operator: 'gt', threshold: 800 };
    case 'crash_free_sessions':
      return { operator: 'lt', threshold: 99.5 };
    case 'frustration_rate':
      return { operator: 'gt', threshold: 3 };
    case 'error_count':
      return { operator: 'gt', threshold: 10 };
    case 'new_issues':
    case 'regressed_issues':
      return { operator: 'gt', threshold: 0 };
    case 'issue_impact':
      return { operator: 'gt', threshold: 20 };
    case 'session_count':
    case 'view_count':
      return { operator: 'lt', threshold: 1 };
    case 'custom_timing':
    case 'custom_vital':
      return { operator: 'gt', threshold: 1_000 };
    // A query's number can be anything: the line starts at nought and the preview says where it runs.
    case 'query':
      return { operator: 'gt', threshold: 0 };
  }
}

/** A draft of the query of a rule on a RUM figure. */
export function emptyRumDraft(metric: RumAlertMetric = 'error_rate'): RumAlertQuery {
  return { applicationId: '', environment: null, page: null, version: null, metric };
}

/**
 * The query of a draft whose narrowing fits its figure: a page or a version the figure cannot be
 * narrowed to is dropped, and so is a minimum of rows for a count.
 */
export function fitted(draft: RumAlertQuery): RumAlertQuery {
  const { customName, statistic, search, ...rest } = draft;
  const searched = draft.metric === 'query' ? (search ?? emptySearch()) : null;
  const counted =
    rumDefaultMinSamples(draft.metric) === 0 || draft.statistic === 'count' || (searched !== null && searchAddsUp(searched));

  return {
    ...rest,
    page: rumSupportsPage(draft.metric) ? (draft.page ?? null) : null,
    version: rumSupportsVersion(draft.metric) ? (draft.version ?? null) : null,
    minSamples: counted ? null : (draft.minSamples ?? null),
    // Only a figure the application measures itself names what it measures.
    ...(isCustomMetric(draft.metric) ? { customName: customName ?? null, statistic: statistic ?? null } : {}),
    // Only a rule made from the explorer has a query.
    ...(searched ? { search: searched } : {}),
  };
}

/** A list in the address, as JSON; none when it is not one. */
function listOf<T>(text: string | null, item: (value: unknown) => value is T): T[] {
  if (!text) return [];

  try {
    const parsed: unknown = JSON.parse(text);

    return Array.isArray(parsed) ? parsed.filter(item) : [];
  } catch {
    return [];
  }
}

const isFilter = (value: unknown): value is { field: string; op: string; value: string } =>
  typeof value === 'object' &&
  value !== null &&
  typeof (value as Record<string, unknown>).field === 'string' &&
  typeof (value as Record<string, unknown>).op === 'string' &&
  typeof ((value as Record<string, unknown>).value ?? '') === 'string';

const isText = (value: unknown): value is string => typeof value === 'string' && value.trim() !== '';

/** The query a page of RUM asked a rule to watch, through the address; none unless the figure is `query`. */
function searchFromParams(params: URLSearchParams): RumAlertSearch | null {
  if (params.get('metric') !== 'query') return null;

  const [fn = 'count', ...field] = (params.get('measure') ?? 'count').split(':');

  return {
    type: params.get('rumType') || 'views',
    statement: params.get('q') ?? '',
    filters: listOf(params.get('filters'), isFilter).map(({ field: name, op, value }) => ({ field: name, op, value: value ?? '' })),
    freeText: listOf(params.get('freeText'), isText),
    measure: { fn, ...(field.length > 0 && field.join(':') ? { field: field.join(':') } : {}) },
  };
}

/** The RUM source a RUM page asked for through the address, or none. */
export function rumDraftFromSearch(search: string, metrics: readonly RumAlertMetric[] = RUM_METRICS): RumAlertQuery | null {
  const params = new URLSearchParams(search);

  if (params.get('source') !== 'rum') return null;

  const metric = params.get('metric') as RumAlertMetric | null;

  return fitted({
    ...emptyRumDraft(metric && metrics.includes(metric) ? metric : 'error_rate'),
    applicationId: params.get('app') ?? '',
    environment: params.get('environment') || null,
    page: params.get('page') || null,
    version: params.get('version') || null,
    customName: params.get('customName') || null,
    search: searchFromParams(params),
  });
}

interface RumRuleSource {
  application: string;
  environment?: string | null;
  page?: string | null;
  version?: string | null;
  metric?: RumAlertMetric;
  customName?: string | null;
  /** The explorer query a rule on `query` watches. */
  search?: RumAlertSearch | null;
}

function rumSourceParams(source: RumRuleSource): URLSearchParams {
  const params = new URLSearchParams({ source: 'rum', app: source.application, metric: source.metric ?? 'error_rate' });

  if (source.environment) params.set('environment', source.environment);
  if (source.page) params.set('page', source.page);
  if (source.version) params.set('version', source.version);
  if (source.customName) params.set('customName', source.customName);

  const search = source.metric === 'query' ? source.search : null;

  if (search) {
    params.set('rumType', search.type);
    if (search.statement) params.set('q', search.statement);
    if (search.filters && search.filters.length > 0) params.set('filters', JSON.stringify(search.filters));
    if (search.freeText && search.freeText.length > 0) params.set('freeText', JSON.stringify(search.freeText));
    params.set('measure', search.measure.field ? `${search.measure.fn}:${search.measure.field}` : search.measure.fn);
  }

  return params;
}

/** The address of a new rule on a RUM figure, as RUM pages open it. */
export function rumNewRuleHref(source: RumRuleSource): string {
  return `/alerts/rules/new?${rumSourceParams(source)}`;
}

/** The address of a new anomaly rule on a RUM figure, against the same time on the days before. */
export function rumNewAnomalyHref(source: RumRuleSource): string {
  return `/alerts/anomaly/add?${rumSourceParams(source)}`;
}

/** The labels a RUM rule carries, so its incidents are found again from the application's pages. */
export function rumRuleLabels(draft: RumAlertQuery): Record<string, string> {
  return {
    'rum.application': draft.applicationId.trim(),
    ...(draft.customName ? { 'rum.custom': draft.customName } : {}),
    ...(draft.environment ? { 'deployment.environment': draft.environment } : {}),
    ...(draft.page ? { 'rum.page': draft.page } : {}),
    ...(draft.version ? { 'rum.version': draft.version } : {}),
  };
}

/** The query of a RUM rule: no stream, and a statement the backend writes from the figure. */
export function rumRuleQuery(draft: RumAlertQuery, periodSecs: number): AlertRule['query'] {
  return {
    language: 'sql',
    statement: '',
    periodSecs: periodSecs,
    stream: null,
    rum: fitted({ ...draft, applicationId: draft.applicationId.trim() }),
  };
}

/** A preview read as the rows of a query, so the editor draws it as any rule's: a window without a value has no row. */
export function rumPreviewResult(preview: RumAlertPreview, tookMs: number): QueryResult {
  const rows = preview.points.flatMap((point) => (point.value === null ? [] : [[point.at, point.value]]));

  return { columns: ['_timestamp', 'value'], rows, scannedRows: rows.length, tookMs: tookMs };
}
