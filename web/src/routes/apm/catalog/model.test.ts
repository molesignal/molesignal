import { describe, expect, it } from 'vitest';

import type { ServiceDefinition, ServiceSummary } from '@/api/apm';

import { groupCatalogRows, parseGrouping, tagKeys } from './groups';
import { catalogRows, filterCatalogRows, isInactive, missingMetadata, sortCatalogRows, tagChoices, throughputOf } from './model';

function summary(name: string, environment: string, requests: number, p95?: number): ServiceSummary {
  const handle = { namespace: 'shop', service: name, environment, from: 0, to: 1 };
  return {
    service: { namespace: 'shop', name, environment },
    firstSeenAt: 0,
    lastSeenAt: 1,
    instrumentation: { recentInstanceCount: 1 },
    versions: [],
    red: {
      requestCount: requests,
      errorCount: 0,
      errorRate: 0,
      durationSum: 0,
      latencyPartial: false,
      exemplars: [],
      ...(p95 === undefined ? {} : { p95Time: p95 }),
    },
    health: 'healthy',
    traces: handle,
  } as ServiceSummary;
}

function definition(name: string, patch: Partial<ServiceDefinition> = {}): ServiceDefinition {
  return { namespace: 'shop', name, description: '', updatedBy: 'u', updatedAt: 0, ...patch };
}

const names = { team: (id: string) => (id === 't1' ? 'Payments' : 'Orders'), user: (id: string) => id };

describe('the service catalog', () => {
  it('puts what people keep on each environment of a service and lists declared services that do not report', () => {
    const rows = catalogRows(
      [summary('checkout', 'prod', 10), summary('checkout', 'staging', 2)],
      [definition('checkout', { teamId: 't1', tier: 'p0' }), definition('billing')],
    );

    expect(rows.map((row) => [row.key, row.environment, row.definition?.tier ?? null])).toEqual([
      ['shop/checkout/prod', 'prod', 'p0'],
      ['shop/checkout/staging', 'staging', 'p0'],
      ['shop/billing', null, null],
    ]);
    expect(rows[2]?.summary).toBeNull();
  });

  it('filters by name, environment and team', () => {
    const rows = catalogRows([summary('checkout', 'prod', 10), summary('cart', 'staging', 2)], [definition('cart', { teamId: 't2' })]);
    expect(filterCatalogRows(rows, { search: 'CHECK', environment: '', team: '' }).map((row) => row.name)).toEqual(['checkout']);
    expect(filterCatalogRows(rows, { search: '', environment: 'staging', team: '' }).map((row) => row.name)).toEqual(['cart']);
    expect(filterCatalogRows(rows, { search: '', environment: '', team: 't2' }).map((row) => row.name)).toEqual(['cart']);
  });

  it('sorts by a column and keeps rows without a value last whichever way it goes', () => {
    const rows = catalogRows([summary('a', 'prod', 5, 300), summary('b', 'prod', 50), summary('c', 'prod', 1, 900)], [definition('d')]);
    expect(sortCatalogRows(rows, { key: 'p95', direction: 'desc' }, names).map((row) => row.name)).toEqual(['c', 'a', 'b', 'd']);
    expect(sortCatalogRows(rows, { key: 'p95', direction: 'asc' }, names).map((row) => row.name)).toEqual(['a', 'c', 'b', 'd']);
    expect(sortCatalogRows(rows, { key: 'throughput', direction: 'desc' }, names).map((row) => row.name)).toEqual(['b', 'a', 'c', 'd']);
  });

  it('counts requests a second over the time in view', () => {
    expect(throughputOf(120, { from: new Date(0), to: new Date(60_000) })).toBe(2);
    expect(throughputOf(5, { from: new Date(0), to: new Date(0) })).toBe(5);
  });

  it('narrows by namespace, tag and favorites, and leaves archived services out unless asked', () => {
    const rows = catalogRows(
      [summary('checkout', 'prod', 10), summary('cart', 'prod', 2), summary('legacy', 'prod', 1)],
      [
        definition('checkout', { tags: { domain: 'orders', pci: '' } }),
        definition('cart', { tags: { domain: 'basket' } }),
        definition('legacy', { archived: true }),
      ],
    );
    const filter = { search: '', environment: '', team: '' };

    expect(filterCatalogRows(rows, filter).map((row) => row.name)).toEqual(['checkout', 'cart']);
    expect(filterCatalogRows(rows, { ...filter, archived: true }).map((row) => row.name)).toEqual(['checkout', 'cart', 'legacy']);
    expect(filterCatalogRows(rows, { ...filter, tag: 'domain' }).map((row) => row.name)).toEqual(['checkout', 'cart']);
    expect(filterCatalogRows(rows, { ...filter, tag: 'domain=orders' }).map((row) => row.name)).toEqual(['checkout']);
    expect(filterCatalogRows(rows, { ...filter, tag: 'pci' }).map((row) => row.name)).toEqual(['checkout']);
    expect(filterCatalogRows(rows, { ...filter, namespace: 'ops' })).toEqual([]);
    expect(filterCatalogRows(rows, { ...filter, only: new Set(['shop/cart']) }).map((row) => row.name)).toEqual(['cart']);
    expect(tagChoices(rows)).toEqual(['domain=basket', 'domain=orders', 'pci']);
    expect(tagKeys(rows)).toEqual(['domain', 'pci']);
  });

  it('puts the favorites first, each part in the order asked for', () => {
    const rows = catalogRows([summary('a', 'prod', 5), summary('b', 'prod', 50), summary('c', 'prod', 1)], []);
    const pinned = new Set(['shop/c', 'shop/a']);

    expect(sortCatalogRows(rows, { key: 'throughput', direction: 'desc' }, names, pinned).map((row) => row.name)).toEqual(['a', 'c', 'b']);
  });

  it('groups by team, tier or a tag, the rows with none last', () => {
    const rows = catalogRows(
      [summary('checkout', 'prod', 10), summary('cart', 'prod', 2), summary('search', 'prod', 1)],
      [definition('checkout', { teamId: 't1', tier: 'p1', tags: { domain: 'orders' } }), definition('cart', { teamId: 't2', tier: 'p0' })],
    );
    const grouped = (grouping: string) =>
      groupCatalogRows(rows, parseGrouping(grouping), names).map((group) => [group.key, group.rows.map((row) => row.name)]);

    // Orders before Payments by the teams' names, not their ids.
    expect(grouped('team')).toEqual([
      ['t2', ['cart']],
      ['t1', ['checkout']],
      ['', ['search']],
    ]);
    expect(grouped('tier')).toEqual([
      ['p0', ['cart']],
      ['p1', ['checkout']],
      ['', ['search']],
    ]);
    expect(grouped('tag:domain')).toEqual([
      ['orders', ['checkout']],
      ['', ['cart', 'search']],
    ]);
    expect(grouped('nonsense')).toEqual([['', ['checkout', 'cart', 'search']]]);
  });

  it('calls a service inactive once it has not reported for a day', () => {
    const [row] = catalogRows([{ ...summary('a', 'prod', 1), lastSeenAt: 1_000 }], []);

    expect(isInactive(row!, 1_000 + 86_400_000_000)).toBe(false);
    expect(isInactive(row!, 1_001 + 86_400_000_000)).toBe(true);
  });

  it('says what a service of a tier asked to have is missing', () => {
    const rules = { teamFromTier: 'p1', ownerFromTier: 'p0', repositoryFromTier: null } as const;
    const [critical, minor, untiered] = catalogRows(
      [],
      [definition('checkout', { tier: 'p0', teamId: 't1' }), definition('search', { tier: 'p2' }), definition('cart')],
    );

    expect(missingMetadata(critical!, rules)).toEqual(['owner']);
    expect(missingMetadata(minor!, rules)).toEqual([]);
    // A service without a tier is asked nothing.
    expect(missingMetadata(untiered!, { ...rules, teamFromTier: 'p5' })).toEqual([]);
  });
});
