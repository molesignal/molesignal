import { describe, expect, it } from 'vitest';

import type { RumRow } from '@/api/rum';

import {
  axisFor,
  axisOf,
  drawn,
  hasFailed,
  mostCameInUnder,
  placeLabels,
  readResources,
  readTasks,
  readUntil,
  resourcesQuery,
  shareOf,
  tasksQuery,
  typesOf,
  viewFacts,
  type WaterfallResource,
} from './model';

const ms = (value: number) => value * 1_000_000;

const START = 1_760_000_000_000_000;

const view = (extra: Record<string, unknown> = {}): RumRow => ({
  timestamp: START,
  'view.id': 'v-1',
  'view.url_host': 'shop.example.com',
  'view.url_path': '/checkout',
  'view.loading_time': ms(3_800),
  'view.first_byte': ms(400),
  'view.performance.fcp.timestamp': ms(1_100),
  'view.dom_content_loaded': ms(1_600),
  'view.performance.lcp.timestamp': ms(3_400),
  'view.performance.lcp.target_selector': 'img.hero-banner',
  'view.performance.lcp.resource_url': 'https://cdn.example.com/hero.webp#top',
  'view.performance.lcp.sub_parts.load_delay': ms(650),
  'view.performance.lcp.sub_parts.load_time': ms(1_500),
  'view.performance.lcp.sub_parts.render_delay': ms(850),
  'view.resource.count': 63,
  'view.long_task.count': 7,
  ...extra,
});

const resource = (id: string, after: number, extra: Record<string, unknown> = {}): RumRow => ({
  timestamp: START + after * 1_000,
  'resource.id': id,
  'resource.type': 'js',
  'resource.url': `https://cdn.example.com/${id}.js`,
  'resource.url_host': 'cdn.example.com',
  'resource.url_path': `/${id}.js`,
  'resource.status_code': 200,
  'resource.duration': ms(200),
  ...extra,
});

describe('viewFacts', () => {
  it('lists the moments of a load in the order of time, and the rest the view says', () => {
    const facts = viewFacts(view());

    expect(facts.marks).toEqual([
      { key: 'first_byte', at: 400 },
      { key: 'fcp', at: 1_100 },
      { key: 'dcl', at: 1_600 },
      { key: 'lcp', at: 3_400 },
    ]);
    expect(facts.loading).toBe(3_800);
    expect(facts.start).toBe(START);
    expect(facts.host).toBe('shop.example.com');
    expect(facts.lcpElement).toBe('img.hero-banner');
    expect(facts.lcpParts).toEqual({ loadDelay: 650, loadTime: 1_500, renderDelay: 850 });
    expect(facts.resources).toBe(63);
    expect(facts.longTasks).toBe(7);
  });

  it('puts a moment that came before another before it, whatever the columns are called', () => {
    const facts = viewFacts(view({ 'view.performance.fcp.timestamp': ms(300) }));

    expect(facts.marks.map((mark) => mark.key)).toEqual(['fcp', 'first_byte', 'dcl', 'lcp']);
  });

  it('has no parts of the paint unless the browser gave every one of them', () => {
    expect(viewFacts(view({ 'view.performance.lcp.sub_parts.render_delay': undefined })).lcpParts).toBeNull();
  });

  it('says nothing for a view that says nothing', () => {
    const facts = viewFacts({ timestamp: START, 'view.id': 'v-2' });

    expect(facts.marks).toEqual([]);
    expect(facts.loading).toBeNull();
    expect(facts.resources).toBeNull();
  });
});

describe('readResources', () => {
  const facts = viewFacts(view());

  it('places a request by when it began, and splits it into the wait and the download', () => {
    const [found] = readResources(
      [resource('app', 250, { 'resource.duration': ms(520), 'resource.download.duration': ms(400), 'resource.render_blocking_status': 'blocking' })],
      facts,
    );

    expect(found).toMatchObject({ id: 'app', start: 250, duration: 520, wait: 120, download: 400, blocking: true, slow: true, failed: false });
  });

  it('draws a request of which the phases are not known whole', () => {
    const [found] = readResources([resource('opaque', 10)], facts);

    expect(found).toMatchObject({ wait: null, download: null });
  });

  it('does not split a request whose download is longer than the request', () => {
    const [found] = readResources([resource('odd', 10, { 'resource.download.duration': ms(900) })], facts);

    expect(found).toMatchObject({ wait: null, download: null });
  });

  it('puts the handshake apart from the connection it is inside', () => {
    const [found] = readResources(
      [resource('tls', 10, { 'resource.connect.duration': ms(40), 'resource.ssl.duration': ms(25), 'resource.dns.duration': ms(8) })],
      facts,
    );

    expect(found?.phases).toMatchObject({ dns: 8, connect: 15, tls: 25 });
  });

  it('knows the file the largest paint was of, by its address without the fragment', () => {
    const found = readResources(
      [resource('hero', 10, { 'resource.url': 'https://cdn.example.com/hero.webp', 'resource.type': 'image' }), resource('other', 20)],
      facts,
    );

    expect(found.map((entry) => [entry.id, entry.lcp])).toEqual([
      ['hero', true],
      ['other', false],
    ]);
  });

  it('counts a request as failed on an error status or none at all', () => {
    const found = readResources(
      [
        resource('ok', 10),
        resource('missing', 20, { 'resource.status_code': 404 }),
        resource('none', 30, { 'resource.status_code': 0 }),
        resource('unknown', 40, { 'resource.status_code': undefined }),
      ],
      facts,
    );

    expect(found.map((entry) => [entry.id, entry.failed])).toEqual([
      ['ok', false],
      ['missing', true],
      ['none', true],
      ['unknown', false],
    ]);
    expect(hasFailed(null)).toBe(false);
    expect(hasFailed(302)).toBe(false);
  });

  it('orders by the time they began and keeps one that began before the view at its start', () => {
    const found = readResources([resource('late', 900), resource('early', 5), resource('before', -300)], facts);

    expect(found.map((entry) => [entry.id, entry.start])).toEqual([
      ['before', 0],
      ['early', 5],
      ['late', 900],
    ]);
  });

  it('leaves out a row that cannot be drawn', () => {
    expect(readResources([resource('no-duration', 10, { 'resource.duration': undefined }), { timestamp: START }], facts)).toEqual([]);
  });
});

describe('readTasks', () => {
  it('places a long task and says its script', () => {
    const [task] = readTasks(
      [
        {
          timestamp: START + 1_200_000,
          'long_task.id': 't-1',
          'long_task.duration': ms(210),
          'long_task.blocking_duration': ms(160),
          'long_task.top_script.source_url': 'https://shop.example.com/checkout.js',
          'long_task.top_script.function': 'validateAddress',
          'long_task.top_script.invoker_type': 'event-listener',
        },
      ],
      viewFacts(view()),
    );

    expect(task).toEqual({
      id: 't-1',
      start: 1_200,
      duration: 210,
      blocking: 160,
      script: 'https://shop.example.com/checkout.js',
      function: 'validateAddress',
      invoker: 'event-listener',
    });
  });

  it('draws a long task that no script was found for', () => {
    const [task] = readTasks([{ timestamp: START + 500_000, 'long_task.id': 't-2', 'long_task.duration': ms(80) }], viewFacts(view()));

    expect(task).toMatchObject({ script: null, function: null, invoker: null, blocking: null });
  });
});

describe('the axis', () => {
  it('ends at whole steps, five at the most', () => {
    expect(axisOf(3_800)).toEqual({ max: 4_000, step: 1_000, ticks: [0, 1_000, 2_000, 3_000, 4_000] });
    expect(axisOf(820)).toMatchObject({ step: 200, max: 1_000 });
    expect(axisOf(0).max).toBeGreaterThan(0);
    expect(axisOf(90_000).ticks.length).toBeLessThanOrEqual(6);
  });

  it('ends after the last moment the view says, not after its slowest request', () => {
    const facts = viewFacts(view());
    const late = readResources([resource('beacon', 20_000, { 'resource.duration': ms(5_000) })], facts);

    expect(axisFor(facts, late).max).toBe(4_000);
  });

  it('is framed by the first requests when the view says no moment', () => {
    const facts = viewFacts({ timestamp: START, 'view.id': 'v-3' });
    const found = readResources([resource('a', 0, { 'resource.duration': ms(1_800) }), resource('z', 60_000)], facts);

    expect(axisFor(facts, found).max).toBe(2_000);
    expect(axisFor(facts, []).max).toBe(1_250);
  });

  it('keeps a moment inside it', () => {
    const axis = axisOf(3_800);

    expect(shareOf(2_000, axis)).toBe(0.5);
    expect(shareOf(9_000, axis)).toBe(1);
    expect(shareOf(-5, axis)).toBe(0);
  });

  it('reads a view until its load had ended, or a quarter of a minute where it does not say', () => {
    expect(readUntil(viewFacts(view()))).toBe(4_000);
    expect(readUntil(viewFacts({ timestamp: START, 'view.id': 'v-4' }))).toBe(15_000);
  });
});

describe('what is drawn', () => {
  const facts = viewFacts(view());
  const axis = axisOf(3_800);
  const all: WaterfallResource[] = readResources(
    [
      resource('a', 100, { 'resource.type': 'css', 'resource.render_blocking_status': 'blocking' }),
      resource('b', 300, { 'resource.type': 'image', 'resource.duration': ms(900) }),
      resource('c', 800, { 'resource.type': 'fetch', 'resource.status_code': 500 }),
      resource('d', 5_000, { 'resource.type': 'fetch' }),
    ],
    facts,
  );

  it('leaves the requests that began after the axis ended out, and counts them', () => {
    const found = drawn(all, axis, { type: '', show: 'all' });

    expect(found.rows.map((entry) => entry.id)).toEqual(['a', 'b', 'c']);
    expect(found.late).toBe(1);
  });

  it('narrows by type and by what the reader looks for', () => {
    expect(drawn(all, axis, { type: 'fetch', show: 'all' }).rows.map((entry) => entry.id)).toEqual(['c']);
    expect(drawn(all, axis, { type: '', show: 'blocking' }).rows.map((entry) => entry.id)).toEqual(['a']);
    expect(drawn(all, axis, { type: '', show: 'slow' }).rows.map((entry) => entry.id)).toEqual(['b']);
    expect(drawn(all, axis, { type: '', show: 'failed' }).rows.map((entry) => entry.id)).toEqual(['c']);
  });

  it('counts the late requests whatever the filters are', () => {
    expect(drawn(all, axis, { type: 'css', show: 'all' }).late).toBe(1);
  });

  it('names the duration most of them came in under, to a figure a person would say', () => {
    const quick = readResources([...Array.from({ length: 9 }, (_, index) => resource(`q${index}`, index, { 'resource.duration': ms(150) })), resource('slow', 1, { 'resource.duration': ms(3_000) })], facts);

    expect(mostCameInUnder(quick)).toBe(200);
    expect(mostCameInUnder([])).toBeNull();
  });

  it('lists the types, the commonest first', () => {
    expect(typesOf(all)).toEqual(['fetch', 'css', 'image']);
  });
});

describe('the reads', () => {
  const facts = viewFacts(view());

  it('asks for the resources of the view in the frame of its load', () => {
    const asked = resourcesQuery(facts);

    expect(asked).toMatchObject({ type: 'resources', filters: [{ field: 'view.id', op: '=', value: 'v-1' }] });
    expect(asked.from).toBe(START - 2_000_000);
    expect(asked.to).toBe(START + 4_000_000 + 2_000_000);
    expect(asked.columns).toContain('resource.first_byte.duration');
  });

  it('asks for the long tasks the same way', () => {
    const asked = tasksQuery(facts);

    expect(asked).toMatchObject({ type: 'long_tasks', filters: [{ field: 'view.id', op: '=', value: 'v-1' }] });
    expect(asked.columns).toContain('long_task.top_script.source_url');
  });
});

describe('the names of the moments', () => {
  it('stand on the first line they fit on, and on the next where they would cover the one before', () => {
    // First byte at a tenth of the axis, then two names a little apart, then one far to the right.
    const lines = placeLabels(
      [
        { share: 0.1, chars: 17 },
        { share: 0.4, chars: 9 },
        { share: 0.47, chars: 9 },
        { share: 0.95, chars: 10 },
      ],
      3,
      420,
    );

    expect(lines).toEqual([0, 0, 1, 0]);
  });

  it('puts a name on the line whose last name ends soonest when no line has room', () => {
    const lines = placeLabels(Array.from({ length: 4 }, () => ({ share: 0.5, chars: 12 })), 2, 420);

    expect(lines).toEqual([0, 1, 0, 1]);
  });
});

describe('the names of the moments in a narrower room', () => {
  it('are set apart on more lines the less room there is', () => {
    const items = [
      { share: 0.1, chars: 17 },
      { share: 0.46, chars: 10 },
    ];

    expect(placeLabels(items, 3, 900)).toEqual([0, 0]);
    expect(placeLabels(items, 3, 300)).toEqual([0, 1]);
  });
});
