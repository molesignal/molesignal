import type { RumFilter } from '@/api/rum';
import { rumNewAnomalyHref, rumNewRuleHref } from '@/routes/alerts/rule-workbench/rumSource';
import type { RumAlertSearch } from '@/types/alerting';

import type { RumTab } from '../model';
import type { RumViz } from '../viz/address';
import { COUNT, COUNTRY_FIELD, type RumMeasureChoice, type RumVizSpec } from '../viz/spec';

/** What a rule takes of what the explorer draws, and what it cannot watch. */
export interface RumAlertPlan {
  search: RumAlertSearch;
  /** The number the rule watches. */
  measure: RumMeasureChoice;
  /** The application the query names, or the only one there is; empty for the editor to ask. */
  application: string;
  /** The environment the query names. */
  environment: string | null;
  /** The way of drawing, which a rule does not have; none for the list. */
  viz: RumViz | null;
  /** The groups, which a rule on RUM is not split by. */
  groups: string[];
  /** The measures after the first: a rule watches one number. */
  measures: RumMeasureChoice[];
  /** A distribution is watched by a percentile of its field. */
  distribution: boolean;
}

interface PlanInput {
  tab: RumTab;
  viz: RumViz;
  spec: RumVizSpec | null;
  /** The query applied, as it was written. */
  statement: string;
  filters: readonly RumFilter[];
  freeText: readonly string[];
  /** The applications there are, by id. */
  applications: readonly string[];
}

/** The value a query compares a field with, when it says the field is that one value and nothing else. */
function onlyValue(filters: readonly RumFilter[], field: string): string | null {
  const named = filters.filter((filter) => filter.field === field);

  return named.length === 1 && named[0]!.op === '=' && named[0]!.value !== '' ? named[0]!.value : null;
}

/**
 * The rule of what the explorer draws: its query, as it was written and as the filters and words
 * it was read into, and the first number it asks for. The list is watched by how many rows it
 * matches, a distribution by the 75th percentile of its field. A rule is about one application,
 * the one the query names or the only one there is.
 */
export function rumAlertPlan({ tab, viz, spec, statement, filters, freeText, applications }: PlanInput): RumAlertPlan {
  const distribution = spec?.viz === 'distribution';
  const measure: RumMeasureChoice =
    !spec || viz === 'list'
      ? COUNT
      : distribution
        ? { fn: 'p75', field: spec.field ?? '' }
        : (spec.measures[0] ?? COUNT);

  const named = onlyValue(filters, 'application.id');

  return {
    search: {
      type: tab,
      statement: statement.trim(),
      filters: filters.map(({ field, op, value }) => ({ field, op, value })),
      freeText: [...freeText],
      measure: { fn: measure.fn, ...(measure.field ? { field: measure.field } : {}) },
    },
    measure,
    application: named ?? (applications.length === 1 ? applications[0]! : ''),
    environment: onlyValue(filters, 'env'),
    viz: viz === 'list' ? null : viz,
    groups: !spec || distribution ? [] : spec.viz === 'map' ? [COUNTRY_FIELD] : spec.groups,
    measures: spec && !distribution ? spec.measures.slice(1) : [],
    distribution,
  };
}

/** The editor of a new rule on the query, against a threshold or against the same time on the days before. */
export function rumAlertHref(plan: RumAlertPlan, kind: 'threshold' | 'anomaly'): string {
  const source = {
    application: plan.application,
    environment: plan.environment,
    metric: 'query' as const,
    search: plan.search,
  };

  return kind === 'threshold' ? rumNewRuleHref(source) : rumNewAnomalyHref(source);
}
