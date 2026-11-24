import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { ChangeEntry } from '@/api/changes';

import { changeHref, changeSummary, changeTone, shareOf } from './model';

const t = ((key: string) => key.split('.').pop() ?? key) as unknown as TFunction;

const entry = (overrides: Partial<ChangeEntry>): ChangeEntry => ({ id: 'c1', kind: 'deployment', at: 0, ...overrides });

describe('change model', () => {
  it('opens a service change on its deployments, an annotation on its dashboard, a rule edit on the rule', () => {
    expect(changeHref(entry({ service: 'checkout', namespace: 'shop', environment: 'prod' }))).toBe('/apm/services/checkout/deployments?namespace=shop&environment=prod');
    expect(changeHref(entry({ kind: 'annotation', dashboardId: 'd1' }))).toBe('/dashboards/d1');
    expect(changeHref(entry({ kind: 'rule', ruleId: 'r1' }))).toBe('/alerts/rules/r1/edit');
    expect(changeHref(entry({ service: 'checkout' }))).toBeNull();
    expect(changeHref(entry({ kind: 'annotation' }))).toBeNull();
  });

  it('says a change in a line, with how it went when not well', () => {
    expect(changeSummary(entry({ service: 'checkout', version: '2.0.0', status: 'failed' }), t)).toBe('deployment · checkout 2.0.0 (failed)');
    expect(changeSummary(entry({ kind: 'annotation', title: 'Load test' }), t)).toBe('annotation · Load test');
    expect(changeSummary(entry({ kind: 'scaling', service: 'checkout', status: 'succeeded' }), t)).toBe('scaling · checkout');
  });

  it('inks trouble before the kind', () => {
    expect(changeTone(entry({ status: 'failed' }))).toContain('red');
    expect(changeTone(entry({ kind: 'feature_flag' }))).toContain('purple');
  });

  it('keeps a moment inside its frame', () => {
    expect(shareOf(50, 0, 100)).toBe(50);
    expect(shareOf(-10, 0, 100)).toBe(0);
    expect(shareOf(200, 0, 100)).toBe(100);
  });
});
