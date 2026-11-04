import { describe, expect, it } from 'vitest';

import { compareEnvironment, compareShiftMicros, DEFAULT_APM_SCOPE, parseApmScope, serviceKey, servicePath, serviceSectionOf, writeApmScope } from './scope';

describe('APM scope in the address', () => {
  it('reads the defaults from an empty address and ignores values it does not know', () => {
    expect(parseApmScope('')).toEqual(DEFAULT_APM_SCOPE);
    expect(parseApmScope('resolution=second&status=done&compare=year')).toEqual(DEFAULT_APM_SCOPE);
  });

  it('reads what the address says', () => {
    expect(parseApmScope('environment=prod&version=2.1&q=check&resolution=hour&status=resolved&compare=week')).toEqual({
      environment: 'prod',
      version: '2.1',
      search: 'check',
      resolution: 'hour',
      status: 'resolved',
      compare: 'week',
    });
  });

  it('writes a value and drops it again at its default, forgetting the page of a list', () => {
    const written = writeApmScope(new URLSearchParams('cursor=abc&team=t1'), { environment: 'prod', status: 'unresolved', compare: 'day' });
    expect(written.toString()).toBe('team=t1&environment=prod&compare=day');
    expect(writeApmScope(written, { environment: '', compare: 'off' }).toString()).toBe('team=t1');
  });

  it('builds a service section path that carries the version, resolution and comparison', () => {
    const carry = new URLSearchParams('version=2.1&resolution=minute&compare=week&q=drop&endpoint=x');
    expect(servicePath({ namespace: 'shop', name: 'check out', environment: 'prod' }, 'endpoints', carry)).toBe(
      '/apm/services/check%20out/endpoints?namespace=shop&environment=prod&version=2.1&resolution=minute&compare=week',
    );
    expect(servicePath({ namespace: 'shop', name: 'checkout', environment: 'prod' })).toBe('/apm/services/checkout?namespace=shop&environment=prod');
  });

  it('knows its sections and comparison periods', () => {
    expect(serviceSectionOf(undefined)).toBe('overview');
    expect(serviceSectionOf('runtime')).toBe('runtime');
    expect(serviceSectionOf('transactions')).toBeNull();
    expect(compareShiftMicros('off')).toBe(0);
    expect(compareShiftMicros('day')).toBe(86_400_000_000);
    expect(compareShiftMicros('week')).toBe(604_800_000_000);
    expect(serviceKey({ namespace: 'shop', name: 'checkout' })).toBe('shop/checkout');
  });

  it('compares with another environment over the same time', () => {
    expect(parseApmScope('compare=env:staging').compare).toBe('env:staging');
    expect(parseApmScope('compare=env:').compare).toBe('off');
    expect(parseApmScope('status=investigating').status).toBe('investigating');
    expect(compareEnvironment('env:staging')).toBe('staging');
    expect(compareEnvironment('week')).toBeNull();
    expect(compareShiftMicros('env:staging')).toBe(0);
  });
});
