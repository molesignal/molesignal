import { describe, expect, it } from 'vitest';

import type { NodeAgentAssets } from '@/api/nodeAgent';

import { assetRows, assetsDone, hasPending } from './model';

function assets(overrides: Partial<NodeAgentAssets> = {}): NodeAgentAssets {
  return {
    streams: { metrics: true, events: true },
    indexes: { state: 'ready' },
    dashboard: { id: 'host-overview-acme', state: 'current' },
    views: ['oom', 'network', 'hardware', 'errors'].map((key) => ({ key, id: `v-${key}`, installed: true })),
    alerts: [
      { key: 'oom_kill', severity: 'error', ruleId: 'r1', enabled: true },
      { key: 'softlockup', severity: 'critical', ruleId: 'r2', enabled: false },
    ],
    ...overrides,
  };
}

const states = (value: NodeAgentAssets) => assetRows(value).map((row) => [row.key, row.state, row.detail]);

describe('the host assets of the Node Agent guide', () => {
  it('needs nothing once everything is installed and the data arrives', () => {
    const value = assets();
    const rows = assetRows(value);

    expect(states(value)).toEqual([
      ['indexes', 'ready', 'ready'],
      ['dashboard', 'ready', 'current'],
      ['views', 'ready', 'count'],
      ['alerts', 'ready', 'count'],
    ]);
    expect(rows[1]!.href).toBe('/dashboards/host-overview-acme');
    expect(rows[2]!.href).toBe('/saved-views');
    expect(rows[3]!.values).toEqual({ made: 2, enabled: 1, total: 2 });
    expect(hasPending(rows)).toBe(false);
    expect(assetsDone(value, rows)).toBe(true);
  });

  it('leaves to the button what is missing, and to the first event the indexes of a stream to come', () => {
    const value = assets({
      indexes: { state: 'noStream' },
      dashboard: { id: 'host-overview-acme', state: 'missing' },
      views: [{ key: 'oom', id: 'v-oom', installed: false }],
      alerts: [{ key: 'oom_kill', severity: 'error', ruleId: null, enabled: false }],
      streams: { metrics: true, events: false },
    });
    const rows = assetRows(value);

    expect(states(value)).toEqual([
      ['indexes', 'waiting', 'no_stream'],
      ['dashboard', 'pending', 'missing'],
      ['views', 'pending', 'count'],
      ['alerts', 'pending', 'count'],
    ]);
    expect(rows[1]!.href).toBeNull();
    expect(rows[2]!.href).toBeNull();
    expect(hasPending(rows)).toBe(true);
    expect(assetsDone(value, rows)).toBe(false);
  });

  it('names the fields that still lack an index and the dashboard it cannot install', () => {
    const value = assets({
      indexes: { state: 'missing', fields: ['host.id', 'event.name'] },
      dashboard: { id: 'host-overview-acme', state: 'taken' },
      alerts: null,
    });

    expect(states(value)).toEqual([
      ['indexes', 'pending', 'missing'],
      ['dashboard', 'blocked', 'taken'],
      ['views', 'ready', 'count'],
      ['alerts', 'blocked', 'unreadable'],
    ]);
    expect(assetRows(value)[0]!.values).toEqual({ fields: 'host.id, event.name' });
  });

  it('offers a newer dashboard to install', () => {
    const value = assets({ dashboard: { id: 'host-overview-acme', state: 'outdated' } });

    expect(assetRows(value)[1]).toMatchObject({ state: 'pending', detail: 'outdated' });
    expect(hasPending(assetRows(value))).toBe(true);
  });
});
