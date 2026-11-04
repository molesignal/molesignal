import { describe, expect, it } from 'vitest';

import { thresholdConflict } from '@/routes/alerts/alertRuleModel';
import {
  rumDefaultMinSamples,
  rumDefaultThreshold,
  rumDefaultWindow,
  RUM_METRICS,
  rumSupportsPage,
  rumSupportsVersion,
} from '@/routes/alerts/rule-workbench/rumSource';

import { type AlertTemplate, TEMPLATE_LABEL, templateQuery, templateRule, templatesFor } from './templates';

const SCOPE = { application: 'storefront', environment: 'prod' };

describe('RUM alert templates', () => {
  it('has ten for a browser application and eight for an app, the starting set first', () => {
    const browser = templatesFor('browser');
    const app = templatesFor('flutter');

    expect(browser).toHaveLength(10);
    expect(app).toHaveLength(8);
    // The recommended ones lead, and the optional one is last.
    for (const list of [browser, app]) {
      const firstOther = list.findIndex((template) => !template.recommended);

      expect(list.slice(firstOther).every((template) => !template.recommended)).toBe(true);
      expect(list.at(-1)?.optional).toBe(true);
    }
  });

  it('names each template once for an application, and an app has no web vitals', () => {
    for (const list of [templatesFor('browser'), templatesFor('flutter')]) {
      expect(new Set(list.map((template) => template.id)).size).toBe(list.length);
    }

    const vitals = ['lcp_p75', 'inp_p75', 'cls_p75'];

    expect(templatesFor('flutter').some((template) => vitals.includes(template.metric))).toBe(false);
    expect(templatesFor('browser').some((template) => template.metric === 'crash_free_sessions')).toBe(false);
  });

  it('watches a figure the server takes, over a window the server takes, with lines that do not contradict each other', () => {
    for (const template of [...templatesFor('browser'), ...templatesFor('flutter')]) {
      expect(RUM_METRICS).toContain(template.metric);
      expect(template.window).toBeGreaterThanOrEqual(60);
      expect(template.window).toBeLessThanOrEqual(21_600);
      expect(thresholdConflict(template.bands.map((entry) => ({ ...entry, forPeriods: 1 })))).toBeFalsy();
    }
  });

  it('starts from the lines and the windows the rule editor starts a figure from', () => {
    const lines = new Map<string, AlertTemplate>(
      [...templatesFor('browser'), ...templatesFor('flutter')].map((template) => [template.metric, template]),
    );

    for (const [metric, template] of lines) {
      // The least severe line is where a person would start, except a figure that is never fine.
      const line = rumDefaultThreshold(metric as AlertTemplate['metric']);

      if (template.bands.length > 1) {
        expect(template.bands[0]).toMatchObject({ operator: line.operator, threshold: line.threshold });
      }

      expect(template.window).toBe(rumDefaultWindow(metric as AlertTemplate['metric']));
    }
  });

  it('makes an enabled rule on the figure, with the lines as its severities and the template in its labels', () => {
    const template = templatesFor('browser')[0]!;
    const rule = templateRule(template, SCOPE, 'Storefront · prod · Slow page loads');

    expect(rule).toMatchObject({
      name: 'Storefront · prod · Slow page loads',
      enabled: true,
      kind: 'scheduled',
      query: {
        stream: null,
        periodSecs: 3_600,
        rum: { applicationId: 'storefront', environment: 'prod', metric: 'lcp_p75' },
      },
      trigger: { operator: 'gt', threshold: 2_500, forPeriods: 1 },
      thresholds: [
        { severity: 'warning', operator: 'gt', threshold: 2_500, forPeriods: 1 },
        { severity: 'critical', operator: 'gt', threshold: 4_000, forPeriods: 1 },
      ],
      labels: {
        'rum.application': 'storefront',
        'deployment.environment': 'prod',
        [TEMPLATE_LABEL]: 'lcp',
      },
    });
    // The default escalation policy is the server's to choose.
    expect(rule).not.toHaveProperty('escalation_policy_id');
  });

  it('leaves out the environment of a rule on every one, and the page and version of every rule', () => {
    for (const template of templatesFor('browser')) {
      const everywhere = templateQuery(template, { application: 'storefront', environment: '' });

      expect(everywhere.environment).toBeNull();
      expect(everywhere.page).toBeNull();
      expect(everywhere.version).toBeNull();
      expect(templateRule(template, { application: 'storefront', environment: '' }, 'name').labels).not.toHaveProperty('deployment.environment');
    }
  });

  it('never narrows a figure to what it cannot be narrowed to, nor asks a count for rows', () => {
    for (const template of [...templatesFor('browser'), ...templatesFor('flutter')]) {
      const query = templateRule(template, SCOPE, 'name').query.rum!;

      if (!rumSupportsPage(template.metric)) expect(query.page).toBeNull();
      if (!rumSupportsVersion(template.metric)) expect(query.version).toBeNull();
      if (rumDefaultMinSamples(template.metric) === 0) expect(query.minSamples).toBeNull();
    }
  });
});
