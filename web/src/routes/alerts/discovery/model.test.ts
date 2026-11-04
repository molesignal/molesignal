import { describe, expect, it } from 'vitest';

import { findingHref, formatFindingValue, subjectName } from './model';

describe('a discovery finding as the page shows it', () => {
  it('reads its figure in the unit of its metric', () => {
    expect(formatFindingValue('error_rate', 0.123)).toBe('12.3%');
    expect(formatFindingValue('p95', 2_400_000)).toBe('2.40 s');
    expect(formatFindingValue('p95', null)).toBe('—');
  });

  it('names its service without the default namespace and links to where it is looked into', () => {
    const apm = { kind: 'apm_anomaly' as const, subject: 'default/checkout', environment: 'prod', detail: {} };
    expect(subjectName(apm)).toBe('checkout');
    expect(findingHref(apm)).toBe('/apm/services/checkout?namespace=default&environment=prod');

    const rum = { kind: 'rum_new_issue' as const, subject: 'web-shop', environment: null, detail: { application: 'Web shop' } };
    expect(subjectName(rum)).toBe('Web shop');
    expect(findingHref(rum)).toBe('/rum/errors?application=web-shop');
  });
});
