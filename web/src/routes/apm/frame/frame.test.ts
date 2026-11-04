import { describe, expect, it } from 'vitest';

import type { ApmMeta } from '@/api/apm';

import { issueState } from '../errors/IssueBadge';
import { qualityState } from './QualityChip';
import { moveColumn, settleColumns } from './useApmColumns';

function meta(patch: Partial<ApmMeta> = {}, quality: Partial<ApmMeta['dataQuality']> = {}): ApmMeta {
  return {
    range: { from: 0, to: 1 },
    resolution: 'minute',
    activationBoundary: false,
    dataQuality: { partial: false, gaps: [], overflowDimensions: [], ...quality },
    ...patch,
  };
}

describe('APM frame pieces', () => {
  it('settles kept columns against what the table has, falling back to its defaults', () => {
    const known = ['a', 'b', 'c'] as const;
    expect(settleColumns(['c', 'x', 'a', 'c'], known, ['a'])).toEqual(['c', 'a']);
    expect(settleColumns(['x'], known, ['a', 'b'])).toEqual(['a', 'b']);
    expect(settleColumns(undefined, known, ['b'])).toEqual(['b']);
  });

  it('moves a column before or after another', () => {
    expect(moveColumn(['a', 'b', 'c'], 'c', 'a', 'before')).toEqual(['c', 'a', 'b']);
    expect(moveColumn(['a', 'b', 'c'], 'a', 'c', 'after')).toEqual(['b', 'c', 'a']);
    expect(moveColumn(['a', 'b'], 'x', 'a', 'before')).toEqual(['a', 'b']);
  });

  it('says the most pressing thing about the data, and nothing when there is nothing to say', () => {
    const now = 10 * 60 * 1_000_000;
    expect(qualityState(meta(), now)).toBeNull();
    expect(qualityState(meta({ lastCompleteBucketAt: now - 60_000_000 }), now)).toBeNull();
    expect(qualityState(meta({ lastCompleteBucketAt: now - 5 * 60_000_000 }), now)).toBe('stale');
    expect(qualityState(meta({}, { overflowDimensions: ['transaction'] }), now)).toBe('overflow');
    expect(qualityState(meta({ activationBoundary: true }, { overflowDimensions: ['transaction'] }), now)).toBe('activation');
    const gap = { orgId: 'o', range: { start: 0, end: 1 }, reason: 'queue_full', droppedFacts: 1, recordedAt: 0 } as ApmMeta['dataQuality']['gaps'][number];
    expect(qualityState(meta({ activationBoundary: true }, { gaps: [gap] }), now)).toBe('partial');
  });

  it('reads an untouched error group as open and a resolved one seen again as regressed', () => {
    expect(issueState(null)).toBe('open');
    expect(issueState({ status: 'resolved', regressed: true, updatedBy: 'u', updatedAt: 0 })).toBe('regressed');
    expect(issueState({ status: 'ignored', regressed: false, updatedBy: 'u', updatedAt: 0 })).toBe('ignored');
  });
});
