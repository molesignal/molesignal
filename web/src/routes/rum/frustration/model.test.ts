import { describe, expect, it } from 'vitest';

import type { RumAggregate, RumRow } from '@/api/rum';

import {
  defaultDirection,
  GROUP_KEYS,
  groupFilters,
  groupOf,
  groupsFor,
  kindsFor,
  kindsOf,
  kindsOfRow,
  mainPage,
  parseDirection,
  parseGroup,
  parseKind,
  parseSort,
  readRows,
  readSamples,
  readShare,
  readShares,
  readStatement,
  type Row,
  samplesQuery,
  sessionPath,
  sharesQuery,
  shareQuery,
  sortRows,
  statementQuery,
  tableQuery,
  trendQuery,
} from './model';

const scope = { application: 'shop', environment: 'prod' };
const range = { from: 1_000, to: 9_000 };
const none = { kind: '', search: '' } as const;

const answer = (measures: string[], rows: Array<{ group?: Array<string | null>; values: Array<number | null> }>): RumAggregate => ({
  groupBy: [],
  measures: measures.map((name) => ({ name, fn: 'count' as const, unit: 'count' as const })),
  rows: rows.map((row) => ({ group: row.group ?? [], values: row.values })),
});

const action = (values: Record<string, unknown>, at: number): RumRow => ({ timestamp: at, ...values });

describe('what the table can be of', () => {
  it('reads a grouping, a kind and an order from an address, and falls back to the elements', () => {
    expect(parseGroup('page')).toBe('page');
    expect(parseGroup('x')).toBe('element');
    expect(parseGroup(null)).toBe('element');
    expect(parseKind('dead')).toBe('dead');
    expect(parseKind('x')).toBe('');
    expect(parseSort('rage')).toBe('rage');
    expect(parseSort('x')).toBe('sessions');
    expect(parseDirection('asc')).toBe('asc');
    expect(parseDirection('up')).toBeUndefined();
    expect(defaultDirection()).toBe('desc');
  });

  it('tells a browser element by its text and its selector, and an app one by its text', () => {
    expect(groupOf('browser', 'element').fields).toEqual(['action.target.name', 'action.target.selector']);
    expect(groupOf('flutter', 'element').fields).toEqual(['action.target.name']);

    for (const platform of ['browser', 'flutter'] as const) {
      for (const group of groupsFor(platform)) expect(group.fields.length).toBeLessThanOrEqual(2);
    }
  });

  it('offers the browsers to a browser application and the systems to an app, and pages are screens in an app', () => {
    expect(groupsFor('browser').map((group) => group.key)).toEqual(['element', 'page', 'version', 'browser']);
    expect(groupsFor('flutter').map((group) => group.key)).toEqual(['element', 'page', 'version', 'system']);
    expect(groupOf('browser', 'page').fields).toEqual(['view.url_path_group']);
    expect(groupOf('flutter', 'page').fields).toEqual(['view.name']);
    // A grouping the application does not have is the elements.
    expect(groupOf('flutter', 'browser').key).toBe('element');
    expect(GROUP_KEYS).toContain('system');
  });

  it('has all three signs for a browser, and the two that a tap can show for an app', () => {
    expect(kindsFor('browser')).toEqual(['rage', 'dead', 'error']);
    expect(kindsFor('flutter')).toEqual(['rage', 'error']);
  });

  it('makes the actions of a group by the values of its columns, and by the lack of one', () => {
    expect(groupFilters(groupOf('browser', 'element'), ['Pay', null])).toEqual([
      { field: 'action.target.name', op: '=', value: 'Pay' },
      { field: 'action.target.selector', op: 'missing', value: '' },
    ]);
  });
});

describe('the reads', () => {
  it('asks each kind with its sessions, over the frustrated actions of the application', () => {
    const asked = statementQuery(scope, range);

    expect(asked.type).toBe('actions');
    expect(asked.filters).toEqual([
      { field: 'application.id', op: '=', value: 'shop' },
      { field: 'env', op: '=', value: 'prod' },
      { field: 'action.frustration.count', op: '>', value: '0' },
    ]);
    expect(asked.measures.map((measure) => measure.name)).toEqual([
      'sessions',
      'rage',
      'rage_sessions',
      'dead',
      'dead_sessions',
      'error',
      'error_sessions',
    ]);
    // A kind is a word the types of an action contain; the sessions are counted by their id.
    expect(asked.measures[1]).toEqual({ fn: 'count', name: 'rage', where: [{ field: 'action.frustration.type', op: 'contains', value: 'rage_' }] });
    expect(asked.measures[2]).toMatchObject({ fn: 'count_distinct', field: 'session.id', name: 'rage_sessions' });
  });

  it('asks how many sessions there were and how many showed a sign', () => {
    const asked = shareQuery(scope, range);

    expect(asked.type).toBe('sessions');
    expect(asked.measures.map((measure) => measure.name)).toEqual(['sessions', 'affected']);
  });

  it('asks the groups of the table ordered by sessions, and narrows them by a kind and by words', () => {
    const asked = tableQuery(scope, range, groupOf('browser', 'element'), { kind: 'dead', search: ' pay ' });

    expect(asked.groupBy).toEqual(['action.target.name', 'action.target.selector']);
    expect(asked.orderBy).toBe('sessions');
    expect(asked.limit).toBe(100);
    expect(asked.freeText).toEqual(['pay']);
    expect(asked.filters).toContainEqual({ field: 'action.frustration.type', op: 'contains', value: 'dead_' });
    expect(tableQuery(scope, range, groupOf('browser', 'page'), none).freeText).toBeUndefined();
  });

  it('asks what a group is made of with the narrowing of the table', () => {
    const group = groupFilters(groupOf('browser', 'element'), ['Pay', 'button.pay']);
    const narrowing = { kind: 'rage', search: 'pay' } as const;
    const shares = sharesQuery(scope, range, narrowing, group, 'version');

    expect(shares.groupBy).toEqual(['version']);
    expect(shares.freeText).toEqual(['pay']);
    expect(shares.filters).toContainEqual({ field: 'action.target.name', op: '=', value: 'Pay' });
    expect(shares.filters).toContainEqual({ field: 'action.frustration.type', op: 'contains', value: 'rage_' });

    const bars = { binMicros: 3_600_000_000, frame: { start: 0, end: 7_200_000_000 } };
    const trend = trendQuery(scope, bars, narrowing, group);

    expect(trend.bucketTime).toBe(3_600_000_000);
    expect(trend.originTime).toBe(0);
    expect(trend.from).toBe(0);
    expect(trend.to).toBe(7_200_000_000);
  });

  it('reads the newest actions of a group, with the columns that tell a session', () => {
    const asked = samplesQuery(scope, range, none, []);

    expect(asked.type).toBe('actions');
    expect(asked.limit).toBe(60);
    expect(asked.columns).toContain('session.start');
    expect(asked.columns).toContain('action.frustration.type');
  });
});

describe('what the page reads out of the answers', () => {
  it('reads the kinds that occurred in the order rage, dead, error, and leaves out the ones that did not', () => {
    const read = readStatement(
      answer(
        ['sessions', 'rage', 'rage_sessions', 'dead', 'dead_sessions', 'error', 'error_sessions'],
        [{ values: [320, 412, 296, 0, 0, 148, 321] }],
      ),
    );

    expect(read?.sessions).toBe(320);
    expect(read?.kinds).toEqual([
      { kind: 'rage', actions: 412, sessions: 296 },
      { kind: 'error', actions: 148, sessions: 321 },
    ]);
  });

  it('reads no statement from an answer that has no row', () => {
    expect(readStatement(undefined)).toBeNull();
    expect(readStatement(answer(['sessions'], []))).toBeNull();
  });

  it('reads the share of the sessions that showed a sign, none when there were no sessions', () => {
    expect(readShare(answer(['sessions', 'affected'], [{ values: [1000, 8] }]))).toBe(0.008);
    expect(readShare(answer(['sessions', 'affected'], [{ values: [0, 0] }]))).toBeNull();
    expect(readShare(undefined)).toBeNull();
  });

  it('reads the rows with their sessions, and a group without a value as none', () => {
    const rows = readRows(
      answer(['rage', 'dead', 'error', 'sessions'], [{ group: ['Pay', 'button.pay'], values: [143, 21, 66, 198] }, { group: [null, '.hero'], values: [0, 187, 0, 187] }]),
    );

    expect(rows).toEqual([
      { key: '["Pay","button.pay"]', values: ['Pay', 'button.pay'], rage: 143, dead: 21, error: 66, sessions: 198 },
      { key: '[null,".hero"]', values: [null, '.hero'], rage: 0, dead: 187, error: 0, sessions: 187 },
    ]);
    expect(readRows(undefined)).toEqual([]);
  });

  it('names the kinds of a row, the commonest first', () => {
    const row: Row = { key: '', values: [], rage: 143, dead: 21, error: 66, sessions: 198 };

    expect(kindsOfRow(row)).toEqual([
      { kind: 'rage', actions: 143 },
      { kind: 'error', actions: 66 },
      { kind: 'dead', actions: 21 },
    ]);
  });

  it('orders by a column in the way chosen, and a tie goes to the row with more sessions', () => {
    const a: Row = { key: 'a', values: ['a'], rage: 5, dead: 0, error: 0, sessions: 9 };
    const b: Row = { key: 'b', values: ['b'], rage: 5, dead: 3, error: 0, sessions: 20 };
    const c: Row = { key: 'c', values: ['c'], rage: 1, dead: 0, error: 0, sessions: 12 };

    expect(sortRows([a, b, c], 'sessions', 'desc').map((row) => row.key)).toEqual(['b', 'c', 'a']);
    expect(sortRows([a, b, c], 'sessions', 'asc').map((row) => row.key)).toEqual(['a', 'c', 'b']);
    expect(sortRows([a, b, c], 'rage', 'desc').map((row) => row.key)).toEqual(['b', 'a', 'c']);
  });

  it('reads the shares of a column, and the page that held most of the sessions', () => {
    const shares = readShares(answer(['sessions'], [{ group: ['/checkout'], values: [150] }, { group: [null], values: [10] }]));

    expect(shares).toEqual([
      { value: '/checkout', sessions: 150 },
      { value: null, sessions: 10 },
    ]);
    expect(mainPage(shares, 198)).toBe('/checkout');
    expect(mainPage(shares, 400)).toBeNull();
    expect(mainPage([{ value: null, sessions: 5 }], 5)).toBeNull();
  });
});

describe('the kinds an action names', () => {
  it('tells the kind by how its word begins, in a browser and in an app', () => {
    expect(kindsOf(['rage_click', 'error_click'])).toEqual(['rage', 'error']);
    expect(kindsOf(['rage_tap', 'dead_tap', 'error_tap'])).toEqual(['rage', 'dead', 'error']);
    expect(kindsOf(['something_else'])).toEqual([]);
  });

  it('reads the list when it comes written out, and a single word', () => {
    expect(kindsOf('["dead_click"]')).toEqual(['dead']);
    expect(kindsOf('rage_click')).toEqual(['rage']);
    expect(kindsOf(null)).toEqual([]);
    expect(kindsOf(7)).toEqual([]);
  });
});

describe('the sessions to look at', () => {
  /** When the sessions began, in milliseconds; a row happened at `start` plus some milliseconds, in microseconds. */
  const start = 1_760_000_000_000;
  const at = (offsetMs: number) => (start + offsetMs) * 1_000;
  const MINUTE = 60_000;

  it('tells a session by its first sign among the ones read, and shows the newest sessions first', () => {
    const rows = [
      action({ 'session.id': 's2', 'session.start': start + 20 * MINUTE, 'action.frustration.type': ['dead_click'], 'usr.anonymous_id': 'anon-7c2e' }, at(30 * MINUTE)),
      action({ 'session.id': 's1', 'session.start': start, 'action.frustration.type': ['rage_click'], 'usr.id': 'u-1', 'session.has_replay': true }, at(12 * MINUTE)),
      action(
        {
          'session.id': 's1',
          'session.start': start,
          'action.frustration.type': ['rage_click', 'error_click'],
          'action.error.count': 1,
          'usr.id': 'u-1',
          'session.has_replay': true,
        },
        at(11 * MINUTE),
      ),
    ];
    const [newest, older] = readSamples(rows);

    expect(newest?.session).toBe('s2');
    expect(newest?.user).toBe('anon-7c2e');
    expect(newest?.anonymous).toBe(true);
    expect(newest?.replay).toBe(false);

    expect(older?.session).toBe('s1');
    // The first sign of the session, not the last one read.
    expect(older?.at).toBe(at(11 * MINUTE));
    expect(older?.user).toBe('u-1');
    expect(older?.anonymous).toBe(false);
    expect(older?.withError).toBe(true);
    expect(older?.replay).toBe(true);
    expect(older?.kinds).toEqual([
      { kind: 'rage', count: 2 },
      { kind: 'error', count: 1 },
    ]);
  });

  it('opens the replay five seconds before the sign, and not before the session began', () => {
    const [late] = readSamples([action({ 'session.id': 's', 'session.start': start, 'action.frustration.type': ['rage_click'] }, at(38_000))]);

    expect(late?.replayAt).toBe(at(33_000));
    expect(late?.offset).toBe(33_000);

    const [early] = readSamples([action({ 'session.id': 's', 'session.start': start, 'action.frustration.type': ['rage_click'] }, at(2_000))]);

    expect(early?.replayAt).toBe(at(0));
    expect(early?.offset).toBe(0);
  });

  it('opens the replay five seconds before the sign when the row does not say when the session began', () => {
    const [sample] = readSamples([action({ 'session.id': 's', 'action.frustration.type': ['rage_click'] }, 90_000_000)]);

    expect(sample?.replayAt).toBe(85_000_000);
    expect(sample?.offset).toBeNull();
  });

  it('leaves out rows without a session, and shows at most the number asked for', () => {
    const rows = ['a', 'b', 'c', 'd'].map((id, index) => action({ 'session.id': id, 'action.frustration.type': ['rage_click'] }, 100 - index));

    expect(readSamples(rows)).toHaveLength(3);
    expect(readSamples(rows, 2).map((sample) => sample.session)).toEqual(['a', 'b']);
    expect(readSamples([action({ 'action.frustration.type': ['rage_click'] }, 1)])).toEqual([]);
  });
});

describe('where a session is opened', () => {
  it('looks for the session a day either side of the sign, and opens a replay at the moment given', () => {
    const path = sessionPath({ session: 'a/b', at: 1_760_000_000_000_000, replayAt: 1_759_999_995_000_000, replay: true });
    const [pathname, query] = path.split('?') as [string, string];
    const params = new URLSearchParams(query);

    expect(pathname).toBe('/rum/sessions/view/a%2Fb');
    expect(params.get('at')).toBe('1759999995000000');
    expect(params.get('from')).toBe('2025-10-08T08:53:20.000Z');
    expect(params.get('to')).toBe('2025-10-10T08:53:20.000Z');
  });

  it('opens a session that has no replay without a moment to open it at', () => {
    const path = sessionPath({ session: 's', at: 1_760_000_000_000_000, replayAt: 1_759_999_995_000_000, replay: false });

    expect(new URLSearchParams(path.split('?')[1]).has('at')).toBe(false);
  });
});
