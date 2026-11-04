import type { AlertRule, ApmAlertQuery, ComparisonOp, RumAlertQuery } from '@/types/alerting';

import {
  APM_ANOMALY_METRICS,
  APM_METRICS,
  apmDefaultThreshold,
  apmDraftFromSearch,
  apmRuleLabels,
  apmRuleQuery,
} from './apmSource';
import {
  RUM_ANOMALY_METRICS,
  RUM_METRICS,
  rumDefaultThreshold,
  rumDefaultWindow,
  rumDraftFromSearch,
  rumRuleLabels,
  rumRuleQuery,
} from './rumSource';

/**
 * A figure of another product watched in place of a query: a rule names the figure, not a
 * statement, and the server reads it as that product's pages read it. A page of the product opens
 * the editor with one, or the person picks the product in the editor.
 */
export type TypedSource = { kind: 'apm'; query: ApmAlertQuery } | { kind: 'rum'; query: RumAlertQuery };

export type SourceKind = TypedSource['kind'];

/** The windows a rule is evaluated over for each product, in seconds. */
export const SOURCE_WINDOWS: Record<SourceKind, readonly number[]> = {
  apm: [60, 300, 600],
  rum: [300, 900, 3_600],
};

/** The source a product's page asked for through the address, or none. The anomaly editor offers fewer figures. */
export function sourceFromSearch(search: string, anomaly = false): TypedSource | null {
  const kind = new URLSearchParams(search).get('source');

  if (kind === 'apm') {
    const query = apmDraftFromSearch(search, anomaly ? APM_ANOMALY_METRICS : APM_METRICS);

    return query && { kind: 'apm', query };
  }

  if (kind === 'rum') {
    const query = rumDraftFromSearch(search, anomaly ? RUM_ANOMALY_METRICS : RUM_METRICS);

    return query && { kind: 'rum', query };
  }

  return null;
}

/** The source a saved rule watches, or none when it runs a statement. */
export function sourceOfRule(rule: Pick<AlertRule, 'query'>): TypedSource | null {
  if (rule.query.apm) return { kind: 'apm', query: rule.query.apm };
  if (rule.query.rum) return { kind: 'rum', query: rule.query.rum };

  return null;
}

/** The first threshold of a rule on a figure. */
export function sourceThreshold(source: TypedSource): { operator: ComparisonOp; threshold: number } {
  return source.kind === 'apm' ? apmDefaultThreshold(source.query.metric) : rumDefaultThreshold(source.query.metric);
}

/** The window a rule on this figure starts with. */
export function sourceWindow(source: TypedSource): number {
  return source.kind === 'rum' ? rumDefaultWindow(source.query.metric) : 60;
}

/**
 * How many evaluations in a row must be over the line before the rule fires. A RUM rule's window
 * is already the stretch the figure must be over the line for, so one is enough; an APM rule is
 * evaluated over a short window and waits for a few.
 */
export function sourceForPeriods(source: TypedSource): number {
  return source.kind === 'rum' ? 1 : 5;
}

/** The query a rule on this figure is saved with. */
export function sourceRuleQuery(source: TypedSource, periodSecs: number): AlertRule['query'] {
  return source.kind === 'apm' ? apmRuleQuery(source.query, periodSecs) : rumRuleQuery(source.query, periodSecs);
}

/** The labels the rule carries, so its incidents are found again from the product's pages. */
export function sourceLabels(source: TypedSource): Record<string, string> {
  return source.kind === 'apm' ? apmRuleLabels(source.query) : rumRuleLabels(source.query);
}

/** Whether the source names what it watches: the service or the application. */
export function sourceIsNamed(source: TypedSource): boolean {
  if (source.kind === 'apm') return source.query.service.trim() !== '';

  const custom = source.query.metric === 'custom_timing' || source.query.metric === 'custom_vital';

  return source.query.applicationId.trim() !== '' && (!custom || (source.query.customName ?? '').trim() !== '');
}

/** A short name for the source, to tell one preview from another. */
export function sourceKey(source: TypedSource): string {
  return `${source.kind}:${JSON.stringify(source.query)}`;
}
