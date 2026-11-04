import { describe, expect, it } from 'vitest';

import type { FieldType, StreamRuntime, StreamSummary, StreamType } from '@/api/streams';

import {
  groupStreamsByName,
  logicalFieldType,
  selectStreamVariant,
  streamVariantsForDetail,
} from './model';

function summary(id: string, name: string, streamType: StreamType, retentionDays = 30): StreamSummary {
  return {
    id,
    label: name,
    name,
    streamType: streamType,
    type: streamType,
    schema: { fields: [] },
    retention: null,
    effectiveRetention: { days: retentionDays },
    settings: {
      description: `${streamType} stream`,
      indexRules: [],
      maxQueryRangeHours: null,
      queryable: true,
    },
    createdAt: 1,
    updatedAt: 1,
  };
}

function runtime(
  id: string,
  name: string,
  streamType: Exclude<StreamType, 'extend'>,
  rows: number,
  status: StreamRuntime['status'],
): StreamRuntime {
  return {
    id,
    name,
    streamType: streamType,
    status,
    rows,
    storedBytes: rows * 2,
    currentStoredBytes: rows * 3,
    totalRows: rows * 4,
    collectedBytes: null,
    indexBytes: rows,
    firstReceivedAt: 100 + rows,
    lastReceivedAt: 1_000 + rows,
    statsAvailable: true,
    buckets: [],
  };
}

describe('groupStreamsByName', () => {
  it('merges equal names across signal types and preserves each concrete variant', () => {
    const grouped = groupStreamsByName(
      [
        summary('logs-id', '_molesignal', 'logs', 7),
        summary('metrics-id', '_molesignal', 'metrics', 30),
        summary('traces-id', '_molesignal', 'traces', 7),
      ],
      [
        runtime('logs-id', '_molesignal', 'logs', 10, 'interrupted'),
        runtime('metrics-id', '_molesignal', 'metrics', 30, 'healthy'),
        runtime('traces-id', '_molesignal', 'traces', 0, 'unused'),
      ],
    );

    expect(grouped).toHaveLength(1);
    expect(grouped[0]?.types).toEqual(['logs', 'metrics', 'traces']);
    expect(grouped[0]?.variants.map((variant) => variant.id)).toEqual([
      'logs-id',
      'metrics-id',
      'traces-id',
    ]);
    expect(grouped[0]?.runtime).toMatchObject({
      status: 'healthy',
      rows: 40,
      stored_bytes: 80,
      current_stored_bytes: 120,
      total_rows: 160,
      collected_bytes: null,
      index_bytes: 40,
      stats_available: true,
    });
    expect(grouped[0]?.retentionDays).toEqual([7, 30]);
  });

  it('aggregates collected bytes and preserves incomplete history', () => {
    const definitions = [summary('logs-id', 'metered', 'logs'), summary('metrics-id', 'metered', 'metrics')];
    const runtimes = [
      { ...runtime('logs-id', 'metered', 'logs', 10, 'healthy'), collectedBytes: 1200, collectedBytesComplete: true },
      { ...runtime('metrics-id', 'metered', 'metrics', 20, 'healthy'), collectedBytes: 800, collectedBytesComplete: true },
    ];
    expect(groupStreamsByName(definitions, runtimes)[0]?.runtime).toMatchObject({ collected_bytes: 2000, collected_bytes_complete: true });
    runtimes[1]!.collectedBytesComplete = false;
    expect(groupStreamsByName(definitions, runtimes)[0]?.runtime).toMatchObject({ collected_bytes: 2000, collected_bytes_complete: false });
  });

  it('uses the active type when choosing a concrete definition', () => {
    const [stream] = groupStreamsByName(
      [
        summary('logs-id', 'default', 'logs'),
        summary('profiles-id', 'default', 'profiles'),
      ],
      [],
    );

    expect(selectStreamVariant(stream!, 'profiles')?.id).toBe('profiles-id');
    expect(selectStreamVariant(stream!, 'metrics')?.id).toBe('logs-id');
  });
});

describe('stream detail variants', () => {
  it('shows every same-name signal in stable order and keeps the current stream', () => {
    const current = summary('metrics-id', '_molesignal', 'metrics');
    const variants = streamVariantsForDetail(current, [
      summary('traces-id', '_molesignal', 'traces'),
      summary('other-id', 'app_logs', 'logs'),
      summary('logs-id', '_molesignal', 'logs'),
    ]);

    expect(variants.map((variant) => [variant.id, variant.streamType])).toEqual([
      ['logs-id', 'logs'],
      ['metrics-id', 'metrics'],
      ['traces-id', 'traces'],
    ]);
  });

  it('does not combine extend tables with observable stream variants', () => {
    const current = summary('extend-id', 'shared', 'extend');

    expect(
      streamVariantsForDetail(current, [summary('logs-id', 'shared', 'logs')]),
    ).toEqual([current]);
  });
});

describe('logicalFieldType', () => {
  it.each<[FieldType, string]>([
    ['bool', 'boolean'],
    ['int64', 'integer'],
    ['float64', 'decimal'],
    ['utf8', 'string'],
    ['timestamp', 'timestamp'],
    ['json', 'json'],
  ])('maps storage type %s to logical type %s', (storageType, logicalType) => {
    expect(logicalFieldType(storageType)).toBe(logicalType);
  });
});
