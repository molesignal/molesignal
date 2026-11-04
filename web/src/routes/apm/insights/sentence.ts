import type { TFunction } from 'i18next';

import type { BaselineFigures, Insight, NearbyChange } from '@/api/apmInsights';

import { formatCount, formatDuration, formatRate, formatThroughput } from '../format';

/** A figure of an insight in its unit: a rate, a duration, requests a second. */
export function insightFigure(insight: Pick<Insight, 'metric'>, value: number): string {
  if (insight.metric === 'error_rate') return formatRate(value);
  if (insight.metric === 'p95') return formatDuration(value);
  return formatThroughput(value);
}

/** What `insight` says of its service, in one sentence that leaves the service's name to its row. */
export function insightSentence(t: TFunction<'apm'>, insight: Insight): string {
  const dependency = insight.dependency;
  const operation = dependency?.operation ?? dependency?.target ?? '';

  switch (insight.kind) {
    case 'anomaly': {
      const metric = insight.metric ?? 'throughput';
      const base = insight.previous ?? insight.lastWeek;
      const direction = base !== undefined && insight.value < base ? 'down' : 'up';
      const before = [
        insight.previous !== undefined ? t('insights.before', { value: insightFigure(insight, insight.previous) }) : null,
        insight.lastWeek !== undefined ? t('insights.week_before', { value: insightFigure(insight, insight.lastWeek) }) : null,
      ].filter(Boolean);

      return t(`insights.sentence.anomaly_${direction}`, {
        metric: t(`insights.metric.${metric}`),
        value: insightFigure(insight, insight.value),
        before: before.join(t('insights.and')),
      });
    }
    case 'regression':
      return t('insights.sentence.regression', {
        version: insight.version ?? '',
        previous: insight.previousVersion ?? '',
        metric: t(`insights.metric.${insight.metric ?? 'error_rate'}`),
        value: insightFigure(insight, insight.value),
        before: insight.previous !== undefined ? insightFigure(insight, insight.previous) : '—',
      });
    case 'slow_dependency':
      return t('insights.sentence.slow_dependency', { share: formatRate(insight.value), target: dependency?.target ?? '' });
    case 'repeated_call':
      return t('insights.sentence.repeated_call', {
        each: Math.round(insight.value),
        operation,
        calls: formatCount(insight.calls ?? 0),
      });
    case 'slow_query':
      return t('insights.sentence.slow_query', { operation, p95: formatDuration(insight.value), calls: formatCount(insight.calls ?? 0) });
    case 'hot_endpoint':
      return t('insights.sentence.hot_endpoint', {
        endpoint: insight.transaction?.name ?? '',
        share: formatRate(insight.value),
        p95: formatDuration(insight.p95Time),
      });
  }
}

/** A change near a move, in a few words. */
export function changeWords(t: TFunction<'apm'>, change: NearbyChange): string {
  const what = t(`insights.change.${change.kind}`);
  const version = change.version ? ` ${change.version}` : '';
  const commit = change.commit ? ` (${change.commit.slice(0, 7)})` : '';
  const seen = change.source === 'observed' ? t('insights.change.observed') : '';

  return `${what} ${change.service}${version}${commit}${seen}`;
}

/** How a stretch compares with now: throughput by share, error rate in points, P95 in time. */
export function baselineDeltas(now: BaselineFigures, before: BaselineFigures): { throughput: number | null; errorRate: number; p95: number | null } {
  return {
    throughput: before.throughput > 0 ? now.throughput / before.throughput - 1 : null,
    errorRate: now.errorRate - before.errorRate,
    p95: now.p95Time !== undefined && before.p95Time !== undefined ? now.p95Time - before.p95Time : null,
  };
}
