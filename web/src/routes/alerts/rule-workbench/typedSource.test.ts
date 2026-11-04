import { describe, expect, it } from 'vitest';

import type { AlertRule } from '@/types/alerting';

import {
  sourceFromSearch,
  sourceIsNamed,
  sourceKey,
  sourceLabels,
  sourceOfRule,
  sourceRuleQuery,
  sourceThreshold,
  sourceWindow,
  SOURCE_WINDOWS,
  type TypedSource,
} from './typedSource';

const apm: TypedSource = { kind: 'apm', query: { service: 'checkout', metric: 'error_rate' } };
const rum: TypedSource = {
  kind: 'rum',
  query: { applicationId: 'shop-web', environment: 'prod', page: null, version: null, metric: 'lcp_p75' },
};

describe('typed alert sources', () => {
  it('reads the source a product page asked for from the address', () => {
    expect(sourceFromSearch('?source=apm&service=checkout')).toMatchObject({ kind: 'apm', query: { service: 'checkout' } });
    expect(sourceFromSearch('?source=rum&app=shop-web&metric=lcp_p75')).toMatchObject({ kind: 'rum', query: { applicationId: 'shop-web', metric: 'lcp_p75' } });
    expect(sourceFromSearch('?source=other')).toBeNull();
    expect(sourceFromSearch('')).toBeNull();
  });

  it('offers the anomaly editor fewer figures than the rule editor, for each product', () => {
    expect(sourceFromSearch('?source=apm&service=a&metric=new_error_groups')).toMatchObject({ query: { metric: 'new_error_groups' } });
    expect(sourceFromSearch('?source=apm&service=a&metric=new_error_groups', true)).toMatchObject({ query: { metric: 'error_rate' } });
    expect(sourceFromSearch('?source=rum&app=a&metric=new_issues', true)).toMatchObject({ query: { metric: 'error_rate' } });
  });

  it('finds the source of a saved rule, and none for a rule that runs a statement', () => {
    const base = { language: 'sql' as const, statement: '', periodSecs: 300 };

    expect(sourceOfRule({ query: { ...base, apm: apm.query } } as Pick<AlertRule, 'query'>)).toEqual(apm);
    expect(sourceOfRule({ query: { ...base, rum: rum.query } } as Pick<AlertRule, 'query'>)).toEqual(rum);
    expect(sourceOfRule({ query: { ...base, stream: { name: 'app', streamType: 'logs' } } })).toBeNull();
  });

  it('starts a rule on each figure with its own threshold and window', () => {
    expect(sourceThreshold(apm)).toEqual({ operator: 'gt', threshold: 5 });
    expect(sourceThreshold(rum)).toEqual({ operator: 'gt', threshold: 2_500 });
    expect(sourceWindow(apm)).toBe(60);
    expect(sourceWindow(rum)).toBe(3_600);

    for (const source of [apm, rum]) {
      expect(SOURCE_WINDOWS[source.kind]).toContain(sourceWindow(source));
    }
  });

  it('saves each as a query with no stream and labels the incidents with what it watches', () => {
    expect(sourceRuleQuery(apm, 60)).toMatchObject({ stream: null, periodSecs: 60, apm: { service: 'checkout' } });
    expect(sourceRuleQuery(rum, 3_600)).toMatchObject({ stream: null, periodSecs: 3_600, rum: { applicationId: 'shop-web' } });
    expect(sourceLabels(apm)).toEqual({ 'service.name': 'checkout' });
    expect(sourceLabels(rum)).toEqual({ 'rum.application': 'shop-web', 'deployment.environment': 'prod' });
  });

  it('knows whether it names what it watches, and tells one figure from another', () => {
    expect(sourceIsNamed(apm)).toBe(true);
    expect(sourceIsNamed({ kind: 'rum', query: { ...rum.query, applicationId: ' ' } })).toBe(false);
    expect(sourceKey(apm)).not.toBe(sourceKey(rum));
    expect(sourceKey(rum)).toBe(sourceKey({ kind: 'rum', query: { ...rum.query } }));
  });
});
