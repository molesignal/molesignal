import type { RumAlertMetric } from '@/types/alerting';

import type { IncidentFigure } from './samples';
import { rumTabOf } from '../explorer/model';
import { explorerPath } from '../overview/model';

const MINUTE_MS = 60_000;

/** The vital of the performance page that stands for a figure, and the three that have a drawer. */
const VITAL: Partial<Record<RumAlertMetric, { vital: string; drawer: boolean }>> = {
  lcp_p75: { vital: 'lcp', drawer: true },
  inp_p75: { vital: 'inp', drawer: true },
  cls_p75: { vital: 'cls', drawer: true },
  ttfb_p75: { vital: 'ttfb', drawer: false },
};

/**
 * The time of an incident as RUM pages are opened on it: from before the window the rule read when
 * it opened, to a little after it was resolved, or to now while it is going on. The incident was
 * found over the window the rule reads, so the page needs the time before it to show what was found.
 */
export function incidentRange(
  incident: { createdAt: number; resolvedAt?: number | undefined },
  periodSecs: number | undefined,
  nowMs: number = Date.now(),
): { from: string; to: string } {
  const from = Math.floor(incident.createdAt / 1_000) - (periodSecs ?? 3_600) * 1_000 - 15 * MINUTE_MS;
  const to = incident.resolvedAt ? Math.floor(incident.resolvedAt / 1_000) + 15 * MINUTE_MS : nowMs;

  return { from: new Date(from).toISOString(), to: new Date(to).toISOString() };
}

/** What the application measures itself has no page of its own: the rows it is read from do. */
function customFigurePath(figure: IncidentFigure, range: { from: string; to: string }): string {
  const name = figure.customName ?? '';
  const vital = figure.metric === 'custom_vital';

  const path = explorerPath(
    vital ? 'vitals' : 'views',
    { application: figure.application, environment: figure.environment ?? '' },
    name ? [vital ? `vital.name = '${name}'` : `view.custom_timings.${name} != ''`] : [],
  );

  return `${path}&${new URLSearchParams({ time: `${range.from}..${range.to}` })}`;
}

/**
 * A rule made from the explorer opens the explorer on its query, in the application and the
 * environment of the rule, drawn over time as the rule watches it. The query is bracketed, so an
 * alternative in it stays one.
 */
function queryFigurePath(figure: IncidentFigure, search: NonNullable<IncidentFigure['search']>, range: { from: string; to: string }): string {
  const statement = search.statement?.trim() ?? '';
  const measure = search.measure.field ? `${search.measure.fn}:${search.measure.field}` : search.measure.fn;

  const path = explorerPath(
    rumTabOf(search.type) ?? 'views',
    { application: figure.application, environment: figure.environment ?? '' },
    statement ? [`(${statement})`] : [],
  );

  return `${path}&${new URLSearchParams({ viz: 'timeseries', m: measure, time: `${range.from}..${range.to}` })}`;
}

/**
 * Where a figure of an incident is read in RUM: the page that tells it (the performance page for a
 * vital, the resources for the requests, the errors, the frustration), of the same application,
 * environment and page, over the time of the incident. A figure that no page tells, or that is not
 * known, opens the overview.
 */
export function rumOpenPath(figure: IncidentFigure, range: { from: string; to: string }): string {
  const params = new URLSearchParams({ app: figure.application });

  if (figure.environment) params.set('env', figure.environment);

  params.set('from', range.from);
  params.set('to', range.to);
  params.set('time', `${range.from}..${range.to}`);

  const vital = figure.metric ? VITAL[figure.metric] : undefined;

  if (vital) {
    params.set('vital', vital.vital);

    // A page narrows the figure: the table is of the pages, and the page opens in its drawer.
    if (figure.page) {
      params.set('by', 'page');
      params.set('open', figure.page);
      if (vital.drawer) params.set('tab', vital.vital);
    }

    return `/rum/performance?${params}`;
  }

  switch (figure.metric) {
    case 'failed_request_rate':
      return `/rum/performance/resources?${params}`;
    case 'error_rate':
    case 'error_count':
    case 'crash_free_sessions':
    case 'new_issues':
    case 'regressed_issues':
    case 'issue_impact':
      return `/rum/errors?${params}`;
    case 'frustration_rate':
      return `/rum/frustration?${params}`;
    case 'custom_vital':
    case 'custom_timing':
      return customFigurePath(figure, range);
    case 'query':
      return figure.search ? queryFigurePath(figure, figure.search, range) : `/rum/overview?${params}`;
    default:
      return `/rum/overview?${params}`;
  }
}
