import { describe, expect, it } from 'vitest';

import type { AlertRule, Incident } from '@/types/alerting';

import {
  createdRule,
  crosses,
  formatFigure,
  gradeOfFigure,
  incidentsOf,
  lineOf,
  ruleStatus,
  rulesOf,
  tripped,
} from './model';
import { TEMPLATE_LABEL, templatesFor } from './templates';

function incident(patch: Partial<Incident> & { labels?: Record<string, string> }): Incident {
  return {
    id: 'inc',
    orgId: 'org',
    ruleId: 'rule',
    escalationPolicyId: 'default',
    status: 'open',
    severity: 'warning',
    summary: 'Slow',
    fingerprint: 'f',
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: 0,
    assignees: [],
    createdAt: 1_000,
    labels: { 'rum.application': 'storefront' },
    annotations: {},
    traceIds: [],
    hostIds: [],
    affectedServices: [],
    triggeringQuery: null,
    ...patch,
  };
}

function rule(patch: Partial<AlertRule> & { rum?: Partial<NonNullable<AlertRule['query']['rum']>> | null }): AlertRule {
  const { rum, ...rest } = patch;

  return {
    id: 'r',
    orgId: 'org',
    name: 'Rule',
    description: '',
    enabled: true,
    query: {
      language: 'sql',
      statement: '',
      periodSecs: 3_600,
      stream: null,
      rum: rum === null ? null : { applicationId: 'storefront', environment: null, metric: 'lcp_p75', ...rum },
    },
    trigger: { operator: 'gt', threshold: 2_500, forPeriods: 1, silenceSecs: 300 },
    escalationPolicyId: 'default',
    labels: {},
    annotations: {},
    ...rest,
  };
}

describe('the incidents of an application', () => {
  const scope = { application: 'storefront', environment: '' };

  it('are the ones going on that name it, the worst first and the newest first among equals', () => {
    const found = incidentsOf(
      [
        incident({ id: 'a', severity: 'warning', createdAt: 10 }),
        incident({ id: 'b', severity: 'critical', createdAt: 5 }),
        incident({ id: 'c', severity: 'warning', createdAt: 20 }),
        incident({ id: 'resolved', status: 'resolved', severity: 'critical' }),
        incident({ id: 'other', labels: { 'rum.application': 'checkout' } }),
        incident({ id: 'nobody', labels: {} }),
      ],
      scope,
    );

    expect(found.map((entry) => entry.id)).toEqual(['b', 'c', 'a']);
  });

  it('count an acknowledged one, which is still going on', () => {
    expect(incidentsOf([incident({ status: 'acknowledged' })], scope)).toHaveLength(1);
  });

  it('are those of the environment in front, and those of a rule that names none', () => {
    const list = [
      incident({ id: 'prod', labels: { 'rum.application': 'storefront', 'deployment.environment': 'prod' } }),
      incident({ id: 'staging', labels: { 'rum.application': 'storefront', 'deployment.environment': 'staging' } }),
      incident({ id: 'every' }),
    ];

    expect(incidentsOf(list, { ...scope, environment: 'prod' }).map((entry) => entry.id).sort()).toEqual(['every', 'prod']);
    expect(incidentsOf(list, scope)).toHaveLength(3);
    expect(incidentsOf(undefined, scope)).toEqual([]);
  });
});

describe('the rules of an application', () => {
  const scope = { application: 'storefront', environment: 'prod' };

  it('are those on its figures, in the environment in front or in every one, those that fire first', () => {
    const found = rulesOf(
      [
        rule({ id: 'healthy', name: 'B', lastState: { kind: 'healthy' } }),
        rule({ id: 'firing', name: 'C', lastState: { kind: 'firing' } }),
        rule({ id: 'off', name: 'A', enabled: false, lastState: { kind: 'firing' } }),
        rule({ id: 'staging', rum: { environment: 'staging' } }),
        rule({ id: 'prod', name: 'D', rum: { environment: 'prod' }, lastState: { kind: 'pending', consecutive: 1 } }),
        rule({ id: 'other', rum: { applicationId: 'checkout' } }),
        rule({ id: 'query', rum: null }),
      ],
      scope,
    );

    expect(found.map((entry) => entry.id)).toEqual(['firing', 'prod', 'healthy', 'off']);
  });

  it('say how they stand: off beats what the evaluator found', () => {
    expect(ruleStatus(rule({ lastState: { kind: 'firing' } }))).toBe('firing');
    expect(ruleStatus(rule({ lastState: { kind: 'dropped' } }))).toBe('dropped');
    expect(ruleStatus(rule({}))).toBe('healthy');
    expect(ruleStatus(rule({ enabled: false, lastState: { kind: 'firing' } }))).toBe('off');
  });
});

describe('what a template already made', () => {
  const template = templatesFor('browser')[0]!;
  const scope = { application: 'storefront', environment: 'prod' };
  const made = rule({ id: 'made', rum: { environment: 'prod' }, labels: { [TEMPLATE_LABEL]: template.id } });

  it('is found by the template, the application and the environment', () => {
    expect(createdRule([made], template, scope)?.id).toBe('made');
    expect(createdRule([made], template, { ...scope, environment: 'staging' })).toBeNull();
    expect(createdRule([made], template, { ...scope, environment: '' })).toBeNull();
    expect(createdRule([made], { ...template, id: 'inp' }, scope)).toBeNull();
    expect(createdRule([made], template, { ...scope, application: 'checkout' })).toBeNull();
    expect(createdRule(undefined, template, scope)).toBeNull();
  });

  it('is told apart for every environment from one environment', () => {
    const everywhere = rule({ id: 'everywhere', labels: { [TEMPLATE_LABEL]: template.id } });

    expect(createdRule([everywhere], template, { ...scope, environment: '' })?.id).toBe('everywhere');
    expect(createdRule([everywhere], template, scope)).toBeNull();
  });
});

describe('figures', () => {
  it('are said in the words of their unit', () => {
    expect(formatFigure('lcp_p75', 2_400, 'en-US')).toBe('2.4 s');
    expect(formatFigure('inp_p75', 180, 'en-US')).toBe('180 ms');
    expect(formatFigure('error_rate', 2.6, 'en-US')).toBe('2.6%');
    expect(formatFigure('crash_free_sessions', 99.97, 'en-US')).toBe('99.97%');
    expect(formatFigure('cls_p75', 0.1349, 'en-US')).toBe('0.13');
    expect(formatFigure('session_count', 38, 'en-US')).toBe('38');
  });

  it('grade as the vitals are graded, and no other figure is graded', () => {
    expect(gradeOfFigure('lcp_p75', 2_400)).toBe('good');
    expect(gradeOfFigure('cls_p75', 0.13)).toBe('needs_improvement');
    expect(gradeOfFigure('inp_p75', 900)).toBe('poor');
    expect(gradeOfFigure('error_rate', 2)).toBeNull();
  });

  it('cross a line the way the line says', () => {
    expect(crosses('gt', 3, 2)).toBe(true);
    expect(crosses('gt', 2, 2)).toBe(false);
    expect(crosses('gte', 2, 2)).toBe(true);
    expect(crosses('lt', 99.2, 99.5)).toBe(true);
    expect(crosses('lte', 99.5, 99.5)).toBe(true);
    expect(crosses('eq', 0, 0)).toBe(true);
    expect(crosses('neq', 0, 0)).toBe(false);
  });

  it('trip the most severe line they are over', () => {
    const bands = templatesFor('browser')[0]!.bands;

    expect(tripped(bands, 2_400)).toBeNull();
    expect(tripped(bands, 3_000)).toBe('warning');
    expect(tripped(bands, 4_300)).toBe('critical');
  });
});

describe('the line a rule pages on', () => {
  it('is the tier of the severity, the first tier otherwise, and the condition of a rule without tiers', () => {
    const tiered = rule({
      thresholds: [
        { severity: 'warning', operator: 'gt', threshold: 2_500, forPeriods: 1 },
        { severity: 'critical', operator: 'gt', threshold: 4_000, forPeriods: 1 },
      ],
    });

    expect(lineOf(tiered, 'critical')).toEqual({ operator: 'gt', threshold: 4_000 });
    expect(lineOf(tiered, 'error')).toEqual({ operator: 'gt', threshold: 2_500 });
    expect(lineOf(rule({}), 'warning')).toEqual({ operator: 'gt', threshold: 2_500 });
  });
});
