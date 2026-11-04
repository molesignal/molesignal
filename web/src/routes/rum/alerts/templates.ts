import type { AlertRuleInput } from '@/api/alerts';
import type { RumPlatform } from '@/api/rum';
import { rumRuleLabels, rumRuleQuery } from '@/routes/alerts/rule-workbench/rumSource';
import type { ComparisonOp, RumAlertMetric, RumAlertQuery, Severity } from '@/types/alerting';

import { isBrowser } from '../overview/model';

/** The label that says which template made a rule, so that the drawer can tell what already exists. */
export const TEMPLATE_LABEL = 'rum.template';

export type TemplateId =
  | 'lcp'
  | 'inp'
  | 'cls'
  | 'error_rate'
  | 'failed_requests'
  | 'frustration'
  | 'crash_free'
  | 'new_issues'
  | 'regressions'
  | 'high_impact'
  | 'traffic';

/** One line a figure is paged on: the figure over the line, in the unit of the figure, raises the severity. */
export interface TemplateBand {
  severity: Severity;
  operator: ComparisonOp;
  threshold: number;
}

/**
 * A rule that is worth having on an application, ready to create: the figure, the window it is
 * read over, and the lines it pages on. The lines are the commonly used ones (the Core Web Vitals
 * have the grades web.dev gives), the windows the ones the figure is best read over.
 */
export interface AlertTemplate {
  id: TemplateId;
  metric: RumAlertMetric;
  /** The window the figure is read over, in seconds. */
  window: number;
  /** The lines, the least severe first; a figure that is never fine has one. */
  bands: readonly TemplateBand[];
  /** The starting set: listed first, and marked as such. */
  recommended: boolean;
  /** Only worth having where the application has traffic all the time. */
  optional?: boolean;
}

const HOUR = 3_600;
const QUARTER = 900;

const band = (severity: Severity, operator: ComparisonOp, threshold: number): TemplateBand => ({ severity, operator, threshold });

const LCP: AlertTemplate = { id: 'lcp', metric: 'lcp_p75', window: HOUR, bands: [band('warning', 'gt', 2_500), band('critical', 'gt', 4_000)], recommended: true };
const INP: AlertTemplate = { id: 'inp', metric: 'inp_p75', window: HOUR, bands: [band('warning', 'gt', 200), band('critical', 'gt', 500)], recommended: true };
const CLS: AlertTemplate = { id: 'cls', metric: 'cls_p75', window: HOUR, bands: [band('warning', 'gt', 0.1), band('critical', 'gt', 0.25)], recommended: false };
const ERROR_RATE: AlertTemplate = { id: 'error_rate', metric: 'error_rate', window: QUARTER, bands: [band('warning', 'gt', 2), band('critical', 'gt', 5)], recommended: true };
const FAILED_REQUESTS: AlertTemplate = { id: 'failed_requests', metric: 'failed_request_rate', window: QUARTER, bands: [band('warning', 'gt', 2), band('critical', 'gt', 5)], recommended: true };
const FRUSTRATION: AlertTemplate = { id: 'frustration', metric: 'frustration_rate', window: HOUR, bands: [band('warning', 'gt', 3), band('critical', 'gt', 8)], recommended: false };
const CRASH_FREE: AlertTemplate = { id: 'crash_free', metric: 'crash_free_sessions', window: HOUR, bands: [band('warning', 'lt', 99.5), band('critical', 'lt', 99)], recommended: true };
const NEW_ISSUES: AlertTemplate = { id: 'new_issues', metric: 'new_issues', window: QUARTER, bands: [band('warning', 'gt', 0)], recommended: true };
const REGRESSIONS: AlertTemplate = { id: 'regressions', metric: 'regressed_issues', window: QUARTER, bands: [band('critical', 'gt', 0)], recommended: false };
const HIGH_IMPACT: AlertTemplate = { id: 'high_impact', metric: 'issue_impact', window: HOUR, bands: [band('warning', 'gt', 20), band('critical', 'gt', 100)], recommended: false };
const TRAFFIC: AlertTemplate = { id: 'traffic', metric: 'session_count', window: QUARTER, bands: [band('critical', 'lt', 1)], recommended: false, optional: true };

/** What a browser application is told by: the vitals, then the errors, the requests and the people. */
const BROWSER_TEMPLATES: readonly AlertTemplate[] = [LCP, INP, ERROR_RATE, FAILED_REQUESTS, NEW_ISSUES, CLS, FRUSTRATION, REGRESSIONS, HIGH_IMPACT, TRAFFIC];

/** An app sends no web vitals, so crashes stand where they do. */
const APP_TEMPLATES: readonly AlertTemplate[] = [CRASH_FREE, ERROR_RATE, FAILED_REQUESTS, NEW_ISSUES, FRUSTRATION, REGRESSIONS, HIGH_IMPACT, TRAFFIC];

/** The templates of an application, the starting set first. */
export function templatesFor(platform: RumPlatform): readonly AlertTemplate[] {
  return isBrowser(platform) ? BROWSER_TEMPLATES : APP_TEMPLATES;
}

/** What a rule made from a template is of. */
export interface TemplateScope {
  application: string;
  /** None is every environment. */
  environment: string;
}

/** The figure a template watches, in the scope it is made for. */
export function templateQuery(template: AlertTemplate, scope: TemplateScope): RumAlertQuery {
  return {
    applicationId: scope.application,
    environment: scope.environment || null,
    page: null,
    version: null,
    metric: template.metric,
  };
}

/**
 * The rule a template makes: enabled, scheduled, with the template's lines as its severities, the
 * labels the rule editor puts on a rule of the same figure, and the template it came from. The
 * escalation policy is left to the default; the rule is changed in the alert module afterwards.
 */
export function templateRule(template: AlertTemplate, scope: TemplateScope, name: string): AlertRuleInput {
  const query = templateQuery(template, scope);
  const bands = template.bands.map((entry) => ({ ...entry, forPeriods: 1 }));
  const first = bands[0]!;

  return {
    name,
    description: '',
    enabled: true,
    kind: 'scheduled',
    query: rumRuleQuery(query, template.window),
    trigger: { operator: first.operator, threshold: first.threshold, forPeriods: 1, silenceSecs: 300 },
    thresholds: bands,
    severity: null,
    labels: { ...rumRuleLabels(query), [TEMPLATE_LABEL]: template.id },
    annotations: {},
  };
}
