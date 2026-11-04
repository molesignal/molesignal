import { describe, expect, it } from 'vitest';

import { DEFAULT_APM_SETTINGS } from '@/api/apmSettings';

import { aliasIssues, caseVariants, generatedBase, limitIssues, mappedTwice, parseNumber, renamed, suffixRule, thresholdIssues } from './form';

describe('the APM settings form', () => {
  it('takes the thresholds the server takes', () => {
    expect(thresholdIssues(DEFAULT_APM_SETTINGS)).toEqual([]);
    expect(
      thresholdIssues({
        ...DEFAULT_APM_SETTINGS,
        health: { ...DEFAULT_APM_SETTINGS.health, errorRateWarning: 0.2 },
        apdexThreshold: 0,
        longTaskTime: 500,
      }),
    ).toEqual(['error_rates', 'apdex', 'long_task']);
    expect(parseNumber(' 12.5 ')).toBe(12.5);
    expect(parseNumber('')).toBeNull();
    expect(parseNumber('ten')).toBeNull();
  });

  it('renames as the first matching rule says, reading Rust group names', () => {
    const rules = [
      { pattern: '^(?P<base>checkout)-[a-z0-9]{5}$', replacement: '${base}' },
      { pattern: '^(cart)-v\\d+$', replacement: '$1' },
      { pattern: '^erase$', replacement: '' },
    ];

    expect(renamed(rules, 'checkout-7d9f2')).toBe('checkout');
    expect(renamed(rules, 'cart-v2')).toBe('cart');
    expect(renamed(rules, 'erase')).toBeNull();
    expect(renamed(rules, 'orders')).toBeNull();
  });

  it('tells a generated suffix from a name', () => {
    expect(generatedBase('checkout-7d9f8b6c5-x2x7k')).toBe('checkout');
    expect(generatedBase('checkout-5f4d2')).toBe('checkout');
    expect(generatedBase('redis-cache')).toBeNull();
    expect(generatedBase('payments-api')).toBeNull();
    expect(generatedBase('7d9f8b6c5')).toBeNull();

    const rule = suffixRule('checkout-7d9f8b6c5-x2x7k');
    expect(rule && renamed([rule], 'checkout-7d9f8b6c5-x2x7k')).toBe('checkout');
    expect(rule && renamed([rule], 'checkout-a1b2c')).toBe('checkout');
  });

  it('finds environments written two ways and mappings said twice', () => {
    expect(caseVariants(['prod', 'Production', 'production', 'staging'])).toEqual([['Production', 'production']]);
    expect([
      ...mappedTwice([
        { from: 'Production', to: 'prod' },
        { from: 'staging', to: 'stage' },
        { from: 'production ', to: 'live' },
      ]),
    ]).toEqual([0, 2]);
  });

  it('refuses aliases as the server does', () => {
    const alias = (name: string, service: string) => ({ aliasNamespace: '', alias: name, namespace: 'default', service });

    expect([...aliasIssues([alias('old', 'new'), alias('new', 'new'), alias('old', 'other'), alias('a', 'old'), alias('', 'x')])]).toEqual([
      [0, 'chained'],
      [1, 'itself'],
      [2, 'twice'],
      [3, 'chained'],
      [4, 'incomplete'],
    ]);
  });

  it('keeps limits whole and within bounds', () => {
    expect(
      limitIssues({
        servicesPerOrgHour: 200,
        transactionsPerServiceHour: 0,
        dependenciesPerServiceHour: 1.5,
        operationsPerDependencyHour: 32,
        errorGroupsPerServiceHour: 16,
        versionsPerServiceHour: 16,
        instancesPerServiceHour: 100_001,
      }),
    ).toEqual(['transactionsPerServiceHour', 'dependenciesPerServiceHour', 'instancesPerServiceHour']);
  });
});
