import { describe, expect, it } from 'vitest';

import { buildSpec, defaultDraft, type SpecDraft } from './spec';

const draft = (overrides: Partial<SpecDraft>): SpecDraft => ({ ...defaultDraft(), ...overrides });

describe('task spec form', () => {
  it('writes dropwatch with only the keys the node reads', () => {
    expect(buildSpec('tool', draft({ filter: ' tcp and port 80 ', deviceScope: 'only', devices: 'eth0, eth1' }))).toEqual({
      spec: {
        tool: 'dropwatch',
        duration_secs: 60,
        filter: 'tcp and port 80',
        max_events_per_second: 100,
        devices: ['eth0', 'eth1'],
      },
    });

    expect(buildSpec('tool', draft({ deviceScope: 'except', devices: 'lo' }))).toMatchObject({
      spec: { devices_excluded: ['lo'] },
    });

    expect(buildSpec('tool', draft({ deviceScope: 'only', devices: ' ' }))).toEqual({
      problem: 'devices',
      field: 'devices',
    });
  });

  it('writes tcpshark in retransmit mode', () => {
    expect(buildSpec('tool', draft({ tool: 'tcpshark', enableTlp: true, maxEventsPerSecond: '' }))).toEqual({
      spec: { tool: 'tcpshark', duration_secs: 60, mode: 'retransmit', enable_tlp: true },
    });
  });

  it('writes iotracing devices as major:minor and leaves empty limits to the node', () => {
    expect(buildSpec('tool', draft({ tool: 'iotracing', durationSecs: '10', devices: '259:3', maxStack: '10' }))).toEqual({
      spec: { tool: 'iotracing', duration_secs: 10, devices: ['259:3'], max_stack: 10 },
    });

    expect(buildSpec('tool', draft({ tool: 'iotracing', devices: 'sda' }))).toEqual({
      problem: 'block_devices',
      field: 'devices',
    });
  });

  it('writes perf for the host, a process or a container', () => {
    expect(buildSpec('tool', draft({ tool: 'perf', durationSecs: '10' }))).toEqual({
      spec: { tool: 'perf', duration_secs: 10, frequency_hz: 99, mode: 'oncpu' },
    });

    expect(buildSpec('tool', draft({ tool: 'perf', target: 'container', containerId: 'c86df4740fab' }))).toMatchObject({
      spec: { container_id: 'c86df4740fab' },
    });

    expect(buildSpec('tool', draft({ tool: 'perf', target: 'pid', pid: '0' }))).toEqual({ problem: 'pid', field: 'pid' });
  });

  it('keeps the window within 1 to 3600 seconds and numbers whole', () => {
    expect(buildSpec('tool', draft({ durationSecs: '3601' }))).toEqual({ problem: 'duration', field: 'durationSecs' });
    expect(buildSpec('tool', draft({ maxEventsPerSecond: '1.5' }))).toEqual({
      problem: 'number',
      field: 'maxEventsPerSecond',
    });
  });

  it('writes an AutoTracing run and a profile', () => {
    expect(buildSpec('autotracing_run', draft({ rule: 'memburst' }))).toEqual({ spec: { rule: 'memburst' } });
    expect(buildSpec('profile', draft({ durationSecs: '30', mode: 'offcpu', target: 'pid', pid: '42' }))).toEqual({
      spec: { duration_secs: 30, pid: 42, frequency_hz: 99, mode: 'offcpu' },
    });
  });
});
