import { rumMetricUnit } from '@/routes/alerts/rule-workbench/rumSource';
import type { AlertRule, ComparisonOp, Incident, RumAlertMetric, Severity } from '@/types/alerting';

import { formatDurationMs, formatShare } from '../_helpers';
import { type AlertTemplate, type TemplateBand, type TemplateScope, TEMPLATE_LABEL } from './templates';
import { formatCount } from '../overview/format';
import { type Grade, gradeOf, VITALS, type VitalKey } from '../overview/model';

const SEVERITY_ORDER: Record<Severity, number> = { critical: 0, error: 1, warning: 2, info: 3 };

/** A severity in the ink of its word: labels are coloured words here, not chips. */
export const SEVERITY_TEXT: Record<Severity, string> = {
  critical: 'text-[var(--red-ink)]',
  error: 'text-[var(--red-ink)]',
  warning: 'text-[var(--yellow-ink)]',
  info: 'text-[var(--blue-ink)]',
};

const OPERATOR: Record<ComparisonOp, string> = { gt: '>', gte: '≥', lt: '<', lte: '≤', eq: '=', neq: '≠' };

export const operatorSymbol = (operator: ComparisonOp): string => OPERATOR[operator];

/** The incidents of an application that are going on, the worst first and the newest first among equals. */
export function incidentsOf(incidents: readonly Incident[] | undefined, scope: Pick<TemplateScope, 'application' | 'environment'>): Incident[] {
  return (incidents ?? [])
    .filter((incident) => incident.status === 'open' || incident.status === 'acknowledged')
    .filter((incident) => incident.labels?.['rum.application'] === scope.application)
    .filter((incident) => {
      // A rule that names no environment watches them all, and so is about whichever is in front.
      const environment = incident.labels?.['deployment.environment'];

      return !environment || !scope.environment || environment === scope.environment;
    })
    .sort((left, right) => SEVERITY_ORDER[left.severity] - SEVERITY_ORDER[right.severity] || right.createdAt - left.createdAt);
}

/** How a rule stands: what the evaluator found last, or that the rule is off. */
export type RuleStatus = 'firing' | 'dropped' | 'no_data' | 'pending' | 'healthy' | 'off';

export function ruleStatus(rule: AlertRule): RuleStatus {
  if (!rule.enabled) return 'off';

  return rule.lastState?.kind ?? 'healthy';
}

const STATUS_ORDER: Record<RuleStatus, number> = { firing: 0, dropped: 1, no_data: 2, pending: 3, healthy: 4, off: 5 };

/** The rules that watch an application, in the environment in front or in every one; those that fire first. */
export function rulesOf(rules: readonly AlertRule[] | undefined, scope: Pick<TemplateScope, 'application' | 'environment'>): AlertRule[] {
  return (rules ?? [])
    .filter((rule) => {
      const figure = rule.query.rum;

      if (figure?.applicationId !== scope.application) return false;

      return !scope.environment || !figure.environment || figure.environment === scope.environment;
    })
    .sort((left, right) => STATUS_ORDER[ruleStatus(left)] - STATUS_ORDER[ruleStatus(right)] || left.name.localeCompare(right.name));
}

/** The rule a template already made for an application and an environment, if there is one. */
export function createdRule(rules: readonly AlertRule[] | undefined, template: AlertTemplate, scope: TemplateScope): AlertRule | null {
  return (
    (rules ?? []).find(
      (rule) =>
        rule.labels?.[TEMPLATE_LABEL] === template.id &&
        rule.query.rum?.applicationId === scope.application &&
        (rule.query.rum.environment ?? '') === scope.environment,
    ) ?? null
  );
}

/** A value of a figure in the words of its unit: `2.4 s`, `1.8%`, `0.13`, `38`. */
export function formatFigure(metric: RumAlertMetric, value: number, language: string): string {
  switch (rumMetricUnit(metric)) {
    case 'milliseconds':
      return formatDurationMs(value);
    case 'percent':
      return formatShare(value / 100);
    case 'score':
      return String(Number(value.toFixed(2)));
    default:
      return formatCount(value, language);
  }
}

const VITAL_OF: Partial<Record<RumAlertMetric, VitalKey>> = { lcp_p75: 'lcp', inp_p75: 'inp', cls_p75: 'cls' };

/** How a vital is graded, in the words people know; none for a figure that is not one of the three. */
export function gradeOfFigure(metric: RumAlertMetric, value: number): Grade | null {
  const key = VITAL_OF[metric];
  const vital = key ? VITALS.find((candidate) => candidate.key === key) : undefined;

  return vital ? gradeOf(vital, value) : null;
}

/** Whether a value is over a line the way the line says. */
export function crosses(operator: ComparisonOp, value: number, threshold: number): boolean {
  switch (operator) {
    case 'gt':
      return value > threshold;
    case 'gte':
      return value >= threshold;
    case 'lt':
      return value < threshold;
    case 'lte':
      return value <= threshold;
    case 'eq':
      return value === threshold;
    case 'neq':
      return value !== threshold;
  }
}

/** The most severe of the lines a value is over now, or none: what a rule made from the template would do first. */
export function tripped(bands: readonly TemplateBand[], value: number): Severity | null {
  const over = bands.filter((entry) => crosses(entry.operator, value, entry.threshold));

  return over.sort((left, right) => SEVERITY_ORDER[left.severity] - SEVERITY_ORDER[right.severity])[0]?.severity ?? null;
}

/** The line a rule pages on at a severity: its tier of that severity, else its first tier, else its one condition. */
export function lineOf(rule: AlertRule, severity: Severity): { operator: ComparisonOp; threshold: number } {
  const tier = rule.thresholds?.find((candidate) => candidate.severity === severity) ?? rule.thresholds?.[0];

  return tier ? { operator: tier.operator, threshold: tier.threshold } : { operator: rule.trigger.operator, threshold: rule.trigger.threshold };
}
