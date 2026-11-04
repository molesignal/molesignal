import { describe, expect, it } from 'vitest';

import type { TimelineItem } from '@/api/rum';

import {
  axisOf,
  barOf,
  defaultTab,
  errorRows,
  inView,
  lastBefore,
  logRows,
  networkRows,
  parseDisplay,
  parseShow,
  parseTab,
  replayMarks,
  shown,
  splitUrl,
  toolRows,
  viewAt,
  viewRows,
} from './model';

const item = (type: TimelineItem['type'], at: number, values: Record<string, unknown> = {}): TimelineItem => ({ type, timestamp: at, ...values });

const SECOND = 1_000_000;

describe('the requests of a session', () => {
  it('reads what a request was, how long it took and whether it failed', () => {
    const [row] = networkRows([
      item('resource', 5 * SECOND, {
        'resource.id': 'r-1',
        'view.id': 'v-1',
        'resource.method': 'POST',
        'resource.url': 'https://api.example.com/v2/checkout/confirm?cart=9',
        'resource.url_host': 'api.example.com',
        'resource.status_code': 500,
        'resource.type': 'fetch',
        'resource.duration': 800_000_000,
        'resource.size': 1_024,
      }),
    ]);

    expect(row).toEqual({
      key: 'r-1',
      at: 5 * SECOND,
      view: 'v-1',
      method: 'POST',
      host: 'api.example.com',
      path: '/v2/checkout/confirm?cart=9',
      url: 'https://api.example.com/v2/checkout/confirm?cart=9',
      status: 500,
      type: 'fetch',
      duration: 800,
      size: 1_024,
      failed: true,
    });
  });

  it('calls a request failed from a client error up, and when nothing answered', () => {
    const rows = networkRows([200, 304, 404, 0].map((status, index) => item('resource', index, { 'resource.status_code': status, 'resource.url': 'https://a.test/x' })));

    expect(rows.map((row) => row.failed)).toEqual([false, false, true, true]);
    expect(networkRows([item('resource', 1, { 'resource.url': 'https://a.test/x' })])[0]?.failed).toBe(false);
  });

  it('leaves out what is not a request, and tells a request with no id by when it happened', () => {
    const rows = networkRows([item('view', 1), item('resource', 2, { 'resource.url': 'https://a.test/' })]);

    expect(rows).toHaveLength(1);
    expect(rows[0]?.key).toBe('resource-2-1');
  });

  it('splits an address into its host and the rest, and keeps what does not parse', () => {
    expect(splitUrl('https://cdn.example.com/img/hero.webp?v=2')).toEqual({ host: 'cdn.example.com', path: '/img/hero.webp?v=2' });
    expect(splitUrl('/relative/path')).toEqual({ host: null, path: '/relative/path' });
  });
});

describe('the errors of a session', () => {
  it('reads the message, the issue it was grouped into and whether it was a crash', () => {
    const [row] = errorRows([
      item('error', 3 * SECOND, {
        'error.id': 'e-1',
        'view.id': 'v-1',
        'error.type': 'TypeError',
        'error.message': "Cannot read properties of undefined (reading 'price')",
        'error.source': 'source',
        'error.handling': 'unhandled',
        'error.issue_id': '3f9c2a71',
        'error.is_crash': false,
      }),
    ]);

    expect(row).toMatchObject({ key: 'e-1', view: 'v-1', type: 'TypeError', issue: '3f9c2a71', crash: false, handling: 'unhandled' });
    expect(row?.message).toContain('price');
  });

  it('is named by its type when it has no message', () => {
    expect(errorRows([item('error', 1, { 'error.type': 'Abort' })])[0]?.message).toBe('Abort');
  });
});

describe('the logs of a session', () => {
  it('reads what a log says, who wrote it and its stack', () => {
    expect(
      logRows([
        {
          type: 'log',
          timestamp: 5,
          'log.id': 'l-1',
          level: 'warn',
          message: 'cart is stale',
          'log.logger': 'cart',
          'log.stack': 'at cart.ts:1',
        },
      ]),
    ).toEqual([{ key: 'l-1', at: 5, level: 'warn', message: 'cart is stale', logger: 'cart', stack: 'at cart.ts:1' }]);
  });

  it('leaves out what is not a log, and tells one with no id or level by what it has', () => {
    const rows = logRows([{ type: 'view', timestamp: 1 }, { type: 'log', timestamp: 7, message: 'x' }]);

    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ key: 'log-7-1', level: 'info', message: 'x' });
  });
});

describe('the views of a session', () => {
  it('reads the levels of a view in milliseconds, and the counts of what it met', () => {
    const [row] = viewRows([
      item('view', 0, {
        'view.id': 'v-1',
        'view.url_path': '/checkout',
        'view.name': 'Checkout',
        'view.performance.lcp.timestamp': 3_400_000_000,
        'view.performance.inp.duration': 310_000_000,
        'view.performance.cls.score': 0.12,
        'view.loading_time': 2_240_000_000,
        'view.resource.count': 63,
        'view.long_task.count': 7,
        'view.error.count': 2,
        'view.frustration.count': 1,
      }),
    ]);

    expect(row).toEqual({
      id: 'v-1',
      at: 0,
      name: 'Checkout',
      path: '/checkout',
      lcp: 3_400,
      inp: 310,
      cls: 0.12,
      loading: 2_240,
      resources: 63,
      longTasks: 7,
      errors: 2,
      frustrations: 1,
    });
  });

  it('has no level where the browser sent none, and no count where the view has none', () => {
    const [row] = viewRows([item('view', 0, { 'view.id': 'v-2', 'view.url_path': '/' })]);

    expect(row).toMatchObject({ lcp: null, inp: null, cls: null, resources: 0, longTasks: 0, name: '/' });
  });

  it('leaves out rows that are not views, and views without an id', () => {
    expect(viewRows([item('action', 0, { 'view.id': 'v-1' }), item('view', 1, {})])).toEqual([]);
  });
});

describe('which rows are in view', () => {
  const rows = toolRows([
    item('resource', 30, { 'resource.id': 'b', 'view.id': 'v-2', 'resource.url': 'https://a.test/b' }),
    item('resource', 10, { 'resource.id': 'a', 'view.id': 'v-1', 'resource.url': 'https://a.test/a' }),
    item('error', 20, { 'error.id': 'e', 'view.id': 'v-1' }),
    item('view', 25, { 'view.id': 'v-2' }),
    item('view', 5, { 'view.id': 'v-1' }),
  ]);

  it('puts every list in the order things happened', () => {
    expect(rows.network.map((row) => row.key)).toEqual(['a', 'b']);
    expect(rows.views.map((row) => row.id)).toEqual(['v-1', 'v-2']);
  });

  it('takes the rows of one view, or all of them', () => {
    expect(inView(rows.network, 'v-1').map((row) => row.key)).toEqual(['a']);
    expect(inView(rows.network, null)).toHaveLength(2);
    expect(inView(rows.errors, 'v-2')).toEqual([]);
  });

  it('opens on the errors where the session ran into any, else on the requests', () => {
    expect(defaultTab(rows)).toBe('errors');
    expect(defaultTab({ ...rows, errors: [] })).toBe('network');
    expect(parseTab('performance')).toBe('performance');
    expect(parseTab('console')).toBeNull();
    expect(parseDisplay('waterfall')).toBe('waterfall');
    expect(parseDisplay('x')).toBe('list');
  });
});

describe('where the replay is', () => {
  const views = viewRows([item('view', 100, { 'view.id': 'a' }), item('view', 200, { 'view.id': 'b' }), item('view', 300, { 'view.id': 'c' })]);

  it('is in the last view that had begun, and in the first before any had', () => {
    expect(viewAt(views, 250)?.id).toBe('b');
    expect(viewAt(views, 300)?.id).toBe('c');
    expect(viewAt(views, 50)?.id).toBe('a');
    expect(viewAt(views, null)?.id).toBe('a');
    expect(viewAt([], 10)).toBeNull();
  });

  it('is at the last row that had happened, and before them all when none had', () => {
    const rows = [{ at: 10 }, { at: 20 }, { at: 20 }, { at: 40 }];

    expect(lastBefore(rows, 5)).toBe(-1);
    expect(lastBefore(rows, 10)).toBe(0);
    expect(lastBefore(rows, 25)).toBe(2);
    expect(lastBefore(rows, 1_000)).toBe(3);
    expect(lastBefore([], 5)).toBe(-1);
  });
});

describe('the bars of the requests of a view', () => {
  const rows = networkRows([
    item('resource', 1_000_000, { 'resource.url': 'https://a.test/a', 'resource.duration': 500_000_000 }),
    item('resource', 3_000_000, { 'resource.url': 'https://a.test/b', 'resource.duration': 1_000_000_000 }),
  ]);

  it('runs from the start of the view to the end of the last request', () => {
    expect(axisOf(rows, 0)).toEqual({ start: 0, length: 4_000_000 });
  });

  it('is never shorter than a tenth of a second', () => {
    expect(axisOf([], 5)).toEqual({ start: 5, length: 100_000 });
  });

  it('places a request as shares of the axis, and a request before the view at its start', () => {
    const axis = axisOf(rows, 0);

    expect(barOf(rows[1] as (typeof rows)[number], axis)).toEqual({ left: 0.75, width: 0.25 });
    expect(barOf({ at: -10, duration: 100 }, axis).left).toBe(0);
    expect(barOf({ at: 3_900_000, duration: 10_000 }, axis).width).toBeCloseTo(0.025, 6);
  });
});

describe('the ticks on the ruler', () => {
  it('marks the errors, the actions that showed a sign, and every page after the first', () => {
    const marks = replayMarks([
      item('view', 0),
      item('action', 1_000_000, { 'action.frustration.count': 0 }),
      item('action', 2_000_000, { 'action.frustration.count': 1 }),
      item('error', 3_000_000),
      item('resource', 3_500_000),
      item('view', 4_000_000),
    ]);

    expect(marks).toEqual([
      { at: 2_000, kind: 'frustration' },
      { at: 3_000, kind: 'error' },
      { at: 4_000, kind: 'view' },
    ]);
  });
});

describe('which requests are shown', () => {
  const rows = networkRows([
    item('resource', 1, { 'resource.id': 'quick', 'resource.url': 'https://a.test/a', 'resource.status_code': 200, 'resource.duration': 90_000_000 }),
    item('resource', 2, { 'resource.id': 'slow', 'resource.url': 'https://a.test/b', 'resource.status_code': 200, 'resource.duration': 500_000_000 }),
    item('resource', 3, { 'resource.id': 'failed', 'resource.url': 'https://a.test/c', 'resource.status_code': 500, 'resource.duration': 120_000_000 }),
    item('resource', 4, { 'resource.id': 'none', 'resource.url': 'https://a.test/d' }),
  ]);

  it('lets all through, or the slow ones from half a second, or the ones that failed', () => {
    expect(shown(rows, 'all').map((row) => row.key)).toEqual(['quick', 'slow', 'failed', 'none']);
    expect(shown(rows, 'slow').map((row) => row.key)).toEqual(['slow']);
    expect(shown(rows, 'failed').map((row) => row.key)).toEqual(['failed']);
  });

  it('reads the choice from the address, and shows all for what it does not know', () => {
    expect(parseShow('failed')).toBe('failed');
    expect(parseShow('blocking')).toBe('all');
    expect(parseShow(null)).toBe('all');
  });
});
