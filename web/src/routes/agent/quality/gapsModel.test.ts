import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { GapGroup } from '@/api/agent/quality';

import { exampleLink, gapFix, gapNote, gapShare } from './gapsModel';
import { qualityViewOf } from './model';

const t = ((key: string, options?: Record<string, unknown>) =>
  options ? `${key} ${JSON.stringify(options)}` : key) as unknown as TFunction;

function group(patch: Partial<GapGroup>): GapGroup {
  return {
    kind: 'data_map',
    rule: 'repeated_queries',
    stream: 'nginx',
    answers: 3,
    calls: 11,
    misses: 8,
    lastAt: 1,
    examples: [],
    ...patch,
  };
}

describe('data gaps', () => {
  it('says what each rule counted, on what', () => {
    expect(gapNote(t, group({}))).toBe(
      'quality.gaps.notes.repeated_queries {"stream":"nginx","count":11,"misses":8}',
    );
    expect(gapNote(t, group({ rule: 'structure_lookups', stream: undefined, calls: 5 }))).toBe(
      'quality.gaps.notes.structure_lookups {"count":5}',
    );
    expect(gapNote(t, group({ rule: 'structure_lookups', calls: 4 }))).toBe(
      'quality.gaps.notes.structure_lookups_stream {"stream":"nginx","count":4}',
    );
    expect(
      gapNote(t, group({ kind: 'field_alignment', rule: 'service_mismatch', service: 'order-api', stream: 'app_logs' })),
    ).toBe('quality.gaps.notes.service_mismatch {"service":"order-api","stream":"app_logs"}');
    expect(gapNote(t, group({ kind: 'feature_tools', rule: 'raw_metric_pulls', stream: 'up', calls: 2 }))).toBe(
      'quality.gaps.notes.raw_metric_pulls {"stream":"up","count":2}',
    );
  });

  it('names the fix of a kind and the chat of an answer', () => {
    expect(gapFix(t, 'feature_tools')).toBe('quality.gaps.fixes.feature_tools');
    expect(exampleLink({ chatId: 'chat 1' })).toBe('/agent/chat?chat=chat%201');
    expect(exampleLink({})).toBeNull();
  });

  it('gives the share of answers with a gap', () => {
    expect(gapShare(3, 12)).toBe(25);
    expect(gapShare(0, 0)).toBeNull();
    expect(gapShare(5, 4)).toBe(100);
  });

  it('has views of its own in the Quality tab', () => {
    expect(qualityViewOf('/agent/quality/gaps')).toBe('gaps');
    expect(qualityViewOf('/agent/quality/drills')).toBe('drills');
  });
});
