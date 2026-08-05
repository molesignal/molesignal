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
    stream_type: streamType,
    type: streamType,
    schema: { fields: [] },
    retention: null,
    effective_retention: { days: retentionDays },
    settings: {
      description: `${streamType} stream`,
      index_rules: [],
      retention_filter: null,
      keep_conditions: [],
      max_query_range_hours: null,
      flatten_level: null,
      use_stream_stats_for_partitioning: false,
      store_original_data: false,
      enable_distinct_values: true,
      queryable: true,
    },
    created_at_micros: 1,
    updated_at_micros: 1,
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
    stream_type: streamType,
    status,
    rows,
    stored_bytes: rows * 2,
    current_stored_bytes: rows * 3,
    first_received_at_micros: 100 + rows,
    last_received_at_micros: 1_000 + rows,
    stats_available: true,
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
      stats_available: true,
    });
    expect(grouped[0]?.retentionDays).toEqual([7, 30]);
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

    expect(variants.map((variant) => [variant.id, variant.stream_type])).toEqual([
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
