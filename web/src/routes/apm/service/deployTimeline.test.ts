import { describe, expect, it } from 'vitest';

import type { VersionSummary } from '@/api/apm';
import type { DeploymentEvent } from '@/api/apmDeployments';

import { canaryPair, commitLink, deployTimeline } from './deployTimeline';

const MINUTE = 60_000_000;

function version(name: string, first: number, last: number): VersionSummary {
  return {
    service: { namespace: 'shop', name: 'checkout', environment: 'prod' },
    version: name,
    firstSeenAt: first * MINUTE,
    lastSeenAt: last * MINUTE,
    observationCount: 10,
  };
}

function event(id: string, name: string, at: number, status: DeploymentEvent['status'] = 'succeeded'): DeploymentEvent {
  return {
    id,
    namespace: 'shop',
    service: 'checkout',
    environment: 'prod',
    version: name,
    status,
    deployedAt: at * MINUTE,
    createdBy: 'token-1',
    createdAt: at * MINUTE,
  };
}

describe('the deployment timeline', () => {
  it('puts reported deployments beside versions only spans saw, the newest first', () => {
    const entries = deployTimeline([event('d2', '2.0.0', 60)], [version('1.0.0', 0, 59), version('2.0.0', 61, 120)]);

    expect(entries.map((entry) => [entry.version, entry.kind])).toEqual([
      ['2.0.0', 'deployed'],
      ['1.0.0', 'first_seen'],
    ]);
    // A reported version is read where its report is.
    expect(entries[0]?.observed?.version).toBe('2.0.0');
    expect(entries[0]?.event?.id).toBe('d2');
  });

  it('reads a deployment of a version that ran before another as a rollback', () => {
    const entries = deployTimeline(
      [event('d1', '1.0.0', 0), event('d2', '2.0.0', 30), event('d3', '1.0.0', 45), event('d4', '1.0.0', 50)],
      [version('1.0.0', 1, 120), version('2.0.0', 31, 44)],
    );

    expect(entries.map((entry) => [entry.version, entry.kind, entry.from])).toEqual([
      // The same version again is a deployment, not another rollback.
      ['1.0.0', 'deployed', undefined],
      ['1.0.0', 'rolled_back', '2.0.0'],
      ['2.0.0', 'deployed', undefined],
      ['1.0.0', 'deployed', undefined],
    ]);
    // What spans saw of a version is said once, on its newest entry.
    expect(entries.filter((entry) => entry.observed?.version === '1.0.0')).toHaveLength(1);
    expect(entries[0]?.observed?.version).toBe('1.0.0');
  });

  it('says spans went back to an older version no pipeline reported, and keeps failures off the run', () => {
    const entries = deployTimeline([event('d2', '2.0.0', 30, 'failed')], [version('1.0.0', 0, 120), version('2.0.0', 31, 40)]);

    expect(entries.map((entry) => [entry.version, entry.kind, entry.from])).toEqual([
      ['1.0.0', 'returned', '2.0.0'],
      ['2.0.0', 'failed', undefined],
      ['1.0.0', 'first_seen', undefined],
    ]);
    expect(entries[0]?.at).toBe(40 * MINUTE);
    // A failed deployment's version is read on no entry of its own.
    expect(entries[1]?.observed).toBeUndefined();
  });
});

describe('changes other than deployments', () => {
  it('sit on the line without moving the running version', () => {
    const flag: DeploymentEvent = { ...event('f1', '', 40), kind: 'feature_flag', description: 'new_checkout on' };
    const entries = deployTimeline([event('d1', '1.0.0', 0), flag, event('d2', '2.0.0', 50)], [version('1.0.0', 1, 49), version('2.0.0', 51, 120)]);

    expect(entries.map((entry) => [entry.version, entry.kind])).toEqual([
      ['2.0.0', 'deployed'],
      ['', 'feature_flag'],
      ['1.0.0', 'deployed'],
    ]);
    expect(entries[1]?.observed).toBeUndefined();
  });
});

describe('a canary', () => {
  it('is the newest two versions still seen, read since the newer came', () => {
    const pair = canaryPair([version('1.0.0', 0, 120), version('2.0.0', 100, 119), version('0.9.0', 0, 10)]);

    expect(pair?.candidate.version).toBe('2.0.0');
    expect(pair?.baseline.version).toBe('1.0.0');
    expect(pair?.from).toBe(100 * MINUTE);
    expect(canaryPair([version('1.0.0', 0, 120), version('2.0.0', 100, 110)])).toBeNull();
    expect(canaryPair([])).toBeNull();
  });
});

describe('a commit link', () => {
  it('opens the commit on the repository web page, never for an ssh address', () => {
    expect(commitLink('https://github.com/acme/checkout.git', 'abc123')).toBe('https://github.com/acme/checkout/commit/abc123');
    expect(commitLink('https://gitlab.example.com/acme/checkout', 'abc123')).toBe('https://gitlab.example.com/acme/checkout/-/commit/abc123');
    expect(commitLink('git@github.com:acme/checkout.git', 'abc123')).toBeNull();
    expect(commitLink(undefined, 'abc123')).toBeNull();
  });
});
