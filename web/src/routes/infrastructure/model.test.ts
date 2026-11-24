import { describe, expect, it } from 'vitest';

import { entityHref, isCurrent, logsHref, metricsHref, placeOf, serviceHref } from './model';

const NOW = 1_795_514_400_000_000;

describe('infrastructure model', () => {
  it('keys a pod by its cluster and namespace, and a host only outside Kubernetes', () => {
    expect(placeOf({ 'k8s.cluster.name': 'prod', 'k8s.namespace.name': 'shop', 'k8s.pod.name': 'api-1', 'host.name': 'api-1' })).toEqual({
      kind: 'pod',
      key: 'prod/shop/api-1',
    });
    expect(placeOf({ 'k8s.node.name': 'n1' })).toEqual({ kind: 'node', key: '/n1' });
    expect(placeOf({ 'host.name': 'db-1' })).toEqual({ kind: 'host', key: 'db-1' });
    expect(placeOf({ 'k8s.cluster.name': 'prod', 'host.name': 'x' })).toBeNull();
    expect(placeOf({})).toBeNull();
  });

  it('links an entity to its page, its metrics and, for pods and hosts, its logs', () => {
    expect(entityHref('pod', 'prod/shop/api-1')).toBe('/infrastructure/pod?key=prod%2Fshop%2Fapi-1');
    expect(decodeURIComponent(metricsHref({ kind: 'node', name: 'n1', key: 'prod/n1' }, NOW))).toContain('promql={"k8s.node.name"="n1"}');
    expect(decodeURIComponent(metricsHref({ kind: 'container', name: 'app', key: 'abc' }, NOW))).toContain('{"container.id"="abc"}');
    expect(logsHref({ kind: 'pod', name: 'api-1' }, NOW)).toMatch(/^\/logs\?pod=api-1&from=/);
    expect(logsHref({ kind: 'cluster', name: 'prod' }, NOW)).toBeNull();
  });

  it('tells what runs now from what was last seen', () => {
    expect(isCurrent({ lastSeenAt: NOW - 60_000_000 }, NOW)).toBe(true);
    expect(isCurrent({ lastSeenAt: NOW - 3_600_000_000 }, NOW)).toBe(false);
  });

  it('opens a service on its APM page', () => {
    expect(serviceHref({ name: 'checkout', namespace: 'shop', environment: 'prod' })).toBe('/apm/services/checkout?namespace=shop&environment=prod');
  });
});
