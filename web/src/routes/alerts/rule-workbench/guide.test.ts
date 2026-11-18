import { describe, expect, it } from 'vitest';

import {
  charCount,
  EMPTY_GUIDE_DRAFT,
  GUIDE_LIMITS,
  type GuideDraft,
  guideDraftOf,
  guidePayload,
  guideProblem,
  newGuideQuery,
} from './guide';

function draft(patch: Partial<GuideDraft>): GuideDraft {
  return { ...EMPTY_GUIDE_DRAFT, queries: [], dashboards: [], ...patch };
}

describe('rule guide draft', () => {
  it('reads a saved guide and writes it back the same', () => {
    const saved = {
      steps: '1. Check the gateway.',
      queries: [
        { name: 'Errors', language: 'sql' as const, stream: { name: 'app_logs', streamType: 'logs' as const }, statement: 'SELECT 1' },
        { name: 'Rate', language: 'promql' as const, statement: 'sum(rate(x[5m]))' },
      ],
      dashboards: ['dash-1'],
    };

    expect(guidePayload(guideDraftOf(saved))).toEqual(saved);
  });

  it('reads an absent guide as an empty draft and an empty draft as an empty guide', () => {
    const empty = guideDraftOf(undefined);

    expect(empty).toEqual({ steps: '', queries: [], dashboards: [] });
    expect(guidePayload(empty)).toEqual({});
  });

  it('trims what was typed, drops a stream from PromQL and repeats from dashboards', () => {
    const query = { ...newGuideQuery('promql'), name: '  Rate ', streamName: 'kept from SQL', statement: ' up ' };

    expect(guidePayload(draft({ steps: '\n Steps \n', queries: [query], dashboards: ['a', ' a ', 'b'] }))).toEqual({
      steps: 'Steps',
      queries: [{ name: 'Rate', language: 'promql', statement: 'up' }],
      dashboards: ['a', 'b'],
    });
  });

  it('names the first problem, query by query', () => {
    const named = { ...newGuideQuery('promql'), name: 'Rate', statement: 'up' };

    expect(guideProblem(draft({ queries: [named] }))).toBeNull();
    expect(guideProblem(draft({ queries: [named, { ...named, name: ' ' }] }))).toEqual({
      kind: 'query_name_required',
      index: 1,
    });
    expect(guideProblem(draft({ queries: [{ ...named, statement: '' }] }))).toEqual({
      kind: 'query_statement_required',
      index: 0,
    });
    expect(guideProblem(draft({ queries: [{ ...named, language: 'sql', streamName: ' ' }] }))).toEqual({
      kind: 'query_stream_required',
      index: 0,
    });
    expect(
      guideProblem(draft({ queries: Array.from({ length: GUIDE_LIMITS.queries + 1 }, () => named) })),
    ).toEqual({ kind: 'too_many_queries', limit: GUIDE_LIMITS.queries });
    expect(
      guideProblem(draft({ dashboards: Array.from({ length: GUIDE_LIMITS.dashboards + 1 }, (_, index) => `d${index}`) })),
    ).toEqual({ kind: 'too_many_dashboards', limit: GUIDE_LIMITS.dashboards });
  });

  it('counts characters the way the backend does', () => {
    expect(charCount('查询')).toBe(2);
    expect(charCount('😀')).toBe(1);
    expect(guideProblem(draft({ steps: '查'.repeat(GUIDE_LIMITS.stepsChars) }))).toBeNull();
    expect(guideProblem(draft({ steps: 'x'.repeat(GUIDE_LIMITS.stepsChars + 1) }))).toEqual({
      kind: 'steps_too_long',
      limit: GUIDE_LIMITS.stepsChars,
    });
  });
});
