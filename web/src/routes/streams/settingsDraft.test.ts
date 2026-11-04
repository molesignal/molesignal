import '@/i18n';

import { describe, expect, it } from 'vitest';

import type { StreamSummary } from '@/api/streams';
import i18n from '@/i18n';

import {
  draftSignature,
  newPartition,
  payloadForDraft,
  toDraft,
  totalShards,
  validateDraft,
} from './settingsDraft';

function metrics(downsample?: { afterDays: number; intervalSecs: number }): StreamSummary {
  return {
    id: 'stream-1',
    label: 'http_requests_total',
    name: 'http_requests_total',
    streamType: 'metrics',
    type: 'metrics',
    schema: { fields: [] },
    retention: { days: 30 },
    effectiveRetention: { days: 30 },
    settings: {
      description: null,
      indexRules: [],
      maxQueryRangeHours: null,
      queryable: true,
      ...(downsample ? { downsample } : {}),
    },
    createdAt: 1,
    updatedAt: 1,
  };
}

const t = i18n.getFixedT('en-us', 'streams');

describe('stream settings draft', () => {
  it('saves a stream that does not downsample unchanged', () => {
    const stream = metrics({ afterDays: 0, intervalSecs: 3600 });
    const draft = toDraft(stream);

    expect(draft.downsample).toBe(false);
    expect(payloadForDraft(draft).settings?.downsample).toEqual({
      afterDays: 0,
      intervalSecs: 3600,
    });
    // The days shown for a switched-off downsampling are not saved.
    expect(draftSignature({ ...draft, downsampleAfterDays: '9' })).toBe(draftSignature(draft));
  });

  it('saves the days and the bucket of a downsampling switched on', () => {
    const draft = { ...toDraft(metrics()), downsample: true, downsampleInterval: 300 };

    expect(draft.downsampleAfterDays).toBe('7');
    expect(payloadForDraft(draft).settings?.downsample).toEqual({
      afterDays: 7,
      intervalSecs: 300,
    });
  });

  it('saves no query range limit for 0 or nothing, and the retention days as typed', () => {
    const draft = toDraft(metrics());

    expect(payloadForDraft({ ...draft, maxQueryRange: '0' }).settings?.maxQueryRangeHours).toBeNull();
    expect(payloadForDraft({ ...draft, maxQueryRange: '' }).settings?.maxQueryRangeHours).toBeNull();
    expect(payloadForDraft({ ...draft, maxQueryRange: '24' }).settings?.maxQueryRangeHours).toBe(24);
    expect(payloadForDraft(draft).retentionDays).toBe(30);
    expect(payloadForDraft({ ...draft, retentionDays: '' }).retentionDays).toBeNull();
  });

  it('keeps up to two partition fields, each with its own way of splitting', () => {
    const stream = metrics();
    stream.settings.partitions = [
      { field: 'service', type: 'hash', buckets: 4 },
      { field: 'pod', type: 'prefix' },
    ];
    const draft = toDraft(stream);

    expect(totalShards(draft.partitions)).toBe(148);
    expect(payloadForDraft(draft).settings?.partitions).toEqual(stream.settings.partitions);
    expect(payloadForDraft({ ...draft, partitions: [] }).settings?.partitions).toEqual([]);
  });

  it('gives a new partition field as many hash shards as the cap leaves, at most 8', () => {
    expect(newPartition('service', [])).toEqual({ field: 'service', scheme: 'hash', buckets: 8 });
    expect(newPartition('service', [{ field: 'pod', scheme: 'prefix', buckets: 8 }]).buckets).toBe(4);
    expect(newPartition('service', [{ field: 'pod', scheme: 'hash', buckets: 64 }]).buckets).toBe(4);
  });

  it('says what cannot be saved', () => {
    const draft = toDraft(metrics());

    expect(validateDraft(draft, t)).toBeNull();
    expect(validateDraft({ ...draft, retentionDays: '0' }, t)).toContain('1 to 3650');
    expect(validateDraft({ ...draft, maxQueryRange: '1.5' }, t)).toContain('hours');
    expect(validateDraft({ ...draft, downsample: true, downsampleAfterDays: '' }, t)).toContain(
      'original samples',
    );
  });
});
