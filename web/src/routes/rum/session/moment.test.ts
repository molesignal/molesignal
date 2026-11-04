import { describe, expect, it } from 'vitest';

import type { RumRow } from '@/api/rum';

import { neighbours, sessionPath } from './moment';

const START = Date.parse('2026-05-23T09:13:00.000Z') * 1_000;

const session = { timestamp: START, 'session.duration': 12 * 60 * 1_000_000_000 } as RumRow;

describe('sessionPath', () => {
  it('is the page of the session when it is given nothing else', () => {
    expect(sessionPath('abc-1')).toBe('/rum/sessions/view/abc-1');
    expect(sessionPath('a b/c')).toBe('/rum/sessions/view/a%20b%2Fc');
  });

  it('opens the replay at a moment, rounded to a microsecond', () => {
    expect(sessionPath('s-1', { at: 1_779_529_000_000_000.5 })).toBe('/rum/sessions/view/s-1?at=1779529000000001');
    expect(sessionPath('s-1', { at: null })).toBe('/rum/sessions/view/s-1');
    expect(sessionPath('s-1', { at: 0 })).toBe('/rum/sessions/view/s-1');
    expect(sessionPath('s-1', { at: Number.NaN })).toBe('/rum/sessions/view/s-1');
  });

  it('says the stretch of time the session ran in, with a minute to spare on each side', () => {
    const params = new URL(sessionPath('s-1', { session, at: START + 5_000_000 }), 'https://x.example').searchParams;

    expect(params.get('at')).toBe(String(START + 5_000_000));
    expect(params.get('from')).toBe('2026-05-23T09:12:00.000Z');
    expect(params.get('to')).toBe('2026-05-23T09:26:00.000Z');
  });

  it('names the playlist the session is gone through in, and nothing for a session whose start is not known', () => {
    expect(sessionPath('s-1', { playlist: 'pl-1' })).toBe('/rum/sessions/view/s-1?playlist=pl-1');
    expect(sessionPath('s-1', { session: {} as RumRow })).toBe('/rum/sessions/view/s-1');
  });
});

describe('neighbours', () => {
  const entries = [{ sessionId: 'a' }, { sessionId: 'b' }, { sessionId: 'c' }];

  it('says where a session is and which ones come before and after it', () => {
    expect(neighbours(entries, 'b')).toEqual({ index: 1, total: 3, previous: entries[0], next: entries[2] });
    expect(neighbours(entries, 'a')).toEqual({ index: 0, total: 3, previous: null, next: entries[1] });
    expect(neighbours(entries, 'c')).toEqual({ index: 2, total: 3, previous: entries[1], next: null });
  });

  it('says nothing of a session that is not in the list', () => {
    expect(neighbours(entries, 'z')).toBeNull();
    expect(neighbours([], 'a')).toBeNull();
  });
});
