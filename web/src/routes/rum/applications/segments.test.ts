import { describe, expect, it } from 'vitest';

import { newSegment, sameSegments, validSegments } from './segments';

const named = (number: number) => `Segment ${number}`;

describe('segments of an application', () => {
  it('starts one with a name nobody has', () => {
    expect(newSegment(['Segment 2'], named).name).toBe('Segment 3');
    expect(newSegment(['segment 2', 'Segment 1'], named).name).toBe('Segment 3');
  });

  it('takes those that are named apart and say something', () => {
    const segment = { id: '', name: 'Germany', conditions: [{ field: 'geo.country_iso_code', op: 'is' as const, values: ['DE'] }] };

    expect(validSegments([segment])).toBe(true);
    expect(validSegments([segment, { ...segment, name: 'germany' }])).toBe(false);
    expect(validSegments([{ ...segment, name: ' ' }])).toBe(false);
    expect(validSegments([{ ...segment, conditions: [] }])).toBe(false);
    expect(validSegments([{ ...segment, conditions: [{ ...segment.conditions[0]!, values: [] }] }])).toBe(false);
  });

  it('knows a property of the person from what no row says', () => {
    const on = (field: string) => [{ id: '', name: 'x', conditions: [{ field, op: 'is' as const, values: ['1'] }] }];

    expect(validSegments(on('usr.extra.plan'))).toBe(true);
    expect(validSegments(on('usr.extra.'))).toBe(false);
    expect(validSegments(on('view.url_path'))).toBe(false);
  });

  it('tells a change from none', () => {
    const one = [{ id: 'a', name: 'A', conditions: [] }];

    expect(sameSegments(one, [{ id: 'a', name: 'A', conditions: [] }])).toBe(true);
    expect(sameSegments(one, [])).toBe(false);
  });
});
