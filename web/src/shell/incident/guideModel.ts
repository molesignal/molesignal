import type { RuleGuide, RuleGuideQuery } from '@/types/alertRuleGuide';

import type { IncidentTimeWindow } from './InvestigationActions';

/** Whether a rule's guide says anything at all; an absent guide says nothing. */
export function guideIsEmpty(guide: RuleGuide | null | undefined): boolean {
  return (
    !guide ||
    (!(guide.steps ?? '').trim() && (guide.queries ?? []).length === 0 && (guide.dashboards ?? []).length === 0)
  );
}

/**
 * Where a query of the guide opens over the incident's time range: PromQL in
 * Metrics, SQL on a trace stream in Traces, any other SQL in Logs, with the
 * stream it names.
 */
export function guideQueryLink(query: RuleGuideQuery, time: IncidentTimeWindow): string {
  const params = new URLSearchParams({
    from: time.from,
    to: time.to,
    time: `${time.from}..${time.to}`,
  });
  if (query.stream?.name) {
    params.set('stream', query.stream.name);
    params.set('streamType', query.stream.streamType);
  }
  if (query.language === 'promql') {
    params.set('promql', query.statement.trim());
    return `/metrics?${params.toString()}`;
  }
  params.set('sql', query.statement.trim());
  return query.stream?.streamType === 'traces' ? `/traces?${params.toString()}` : `/logs?${params.toString()}`;
}

/** Where a dashboard of the guide opens. */
export function guideDashboardLink(id: string): string {
  return `/dashboards/${encodeURIComponent(id)}`;
}
