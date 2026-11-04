import { describe, expect, it } from 'vitest';

import type { RumRow } from '@/api/rum';

import {
  addressRange,
  addressReplayAt,
  issueTotal,
  sessionHeadline,
  sessionIssues,
  sessionPage,
  sessionUser,
  sessionWindow,
} from './model';

const session = (values: Record<string, unknown> = {}): RumRow => ({ timestamp: 1_760_000_000_000_000, ...values });

describe('what went wrong in a session', () => {
  it('is counted from the session row', () => {
    const issues = sessionIssues(
      session({
        'session.error.count': 2,
        'session.frustration.count': 1,
        'session.crash.count': 1,
        'session.long_task.count': 4,
      }),
    );

    expect(issues).toEqual({ errors: 2, frustrations: 1, crashes: 1, longTasks: 4 });
    expect(issueTotal(issues)).toBe(8);
    expect(issueTotal(sessionIssues(session()))).toBe(0);
  });

  it('names the session by its worst, errors before frustration', () => {
    expect(sessionHeadline(session({ 'session.error.count': 1, 'session.frustration.count': 3 }))).toBe('error');
    expect(sessionHeadline(session({ 'session.crash.count': 1 }))).toBe('error');
    expect(sessionHeadline(session({ 'session.frustration.count': 3 }))).toBe('frustration');
    expect(sessionHeadline(session({ 'session.long_task.count': 3 }))).toBe('plain');
  });
});

describe('who and where', () => {
  it('names the user by the most telling of what is known', () => {
    expect(sessionUser(session({ 'usr.name': 'Ada', 'usr.id': 'u-1' }))).toBe('Ada');
    expect(sessionUser(session({ 'usr.id': 'u-1', 'usr.anonymous_id': 'anon' }))).toBe('u-1');
    expect(sessionUser(session({ 'usr.anonymous_id': 'anon' }))).toBe('anon');
    expect(sessionUser(session())).toBeUndefined();
  });

  it('tells the page the session ended on, else the one it began on', () => {
    expect(
      sessionPage(session({ 'session.exit_view.url_path': '/pay', 'session.landing_view.url_path': '/' })),
    ).toBe('/pay');
    expect(sessionPage(session({ 'session.landing_view.url_path': '/' }))).toBe('/');
    expect(sessionPage(session())).toBeUndefined();
  });
});

describe('when a session ran', () => {
  it('is from its start for as long as it lasted', () => {
    expect(sessionWindow(session({ 'session.duration': 60_000_000_000 }))).toEqual({
      from: '2025-10-09T08:53:20.000Z',
      to: '2025-10-09T08:54:20.000Z',
    });
  });

  it('is nothing for a session with no start', () => {
    expect(sessionWindow(null)).toBeUndefined();
    expect(sessionWindow({ timestamp: 0 })).toBeUndefined();
  });
});

describe('the range an address asks for', () => {
  it('is read from from and to as instants', () => {
    expect(addressRange(new URLSearchParams('from=2025-10-09T08:00:00Z&to=2025-10-09T09:00:00Z'))).toEqual({
      from: 1_760_000_000_000_000 - 3_200_000_000,
      to: 1_760_000_000_000_000 + 400_000_000,
    });
  });

  it('is nothing when one end is missing, wrong, or before the other', () => {
    expect(addressRange(new URLSearchParams('from=2025-10-09T08:00:00Z'))).toBeUndefined();
    expect(addressRange(new URLSearchParams('from=x&to=y'))).toBeUndefined();
    expect(addressRange(new URLSearchParams('from=2025-10-09T09:00:00Z&to=2025-10-09T08:00:00Z'))).toBeUndefined();
  });
});

describe('where the replay opens', () => {
  it('is the moment the address names, in milliseconds', () => {
    expect(addressReplayAt(new URLSearchParams('at=1760000000000000'))).toBe(1_760_000_000_000);
  });

  it('is the beginning when the address names none, or names nothing sensible', () => {
    expect(addressReplayAt(new URLSearchParams(''))).toBeUndefined();
    expect(addressReplayAt(new URLSearchParams('at=soon'))).toBeUndefined();
    expect(addressReplayAt(new URLSearchParams('at=-5'))).toBeUndefined();
    expect(addressReplayAt(new URLSearchParams('at=0'))).toBeUndefined();
  });
});
