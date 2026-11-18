import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import type { RumField } from '@/api/rum';
import { setActiveTimePreference } from '@/lib/time';

import { columnLayout, describeCell, isNumericColumn, RumCell, TIME_TRACK } from './cells';

const field = (
  name: string,
  data_type: RumField['dataType'] = 'utf8',
  unit: RumField['unit'] = 'none',
): RumField => ({ name, dataType: data_type, unit, facetable: true, indexed: false });

describe('describeCell', () => {
  it('has nothing to say of an empty value', () => {
    expect(describeCell(field('usr.id'), null, 'en')).toBeNull();
    expect(describeCell(field('usr.id'), '', 'en')).toBeNull();
    expect(describeCell(field('usr.id'), undefined, 'en')).toBeNull();
  });

  it('tells nanoseconds in the unit they read best in, and the other units as they are', () => {
    expect(describeCell(field('resource.duration', 'int64', 'ns'), 120_000_000, 'en')?.text).toBe('120 ms');
    expect(describeCell(field('resource.size', 'int64', 'bytes'), 4_300, 'en')?.text).toBe('4.2 KB');
    expect(describeCell(field('view.performance.cls.score', 'float64', 'score'), 0.12345, 'en')?.text).toBe('0.123');
    expect(describeCell(field('session.view.count', 'int64', 'count'), 12_345, 'en')?.text).toBe('12,345');
    expect(describeCell(field('x.rate', 'float64', 'percent'), 12.5, 'en')?.text).toBe('12.5%');
  });

  it('writes a status in the colour of its class, and a failed request red', () => {
    const status = field('resource.status_code', 'int64');

    expect(describeCell(status, 200, 'en')).toEqual({ text: '200', tone: 'green' });
    expect(describeCell(status, 301, 'en')).toEqual({ text: '301', tone: 'yellow' });
    expect(describeCell(status, 404, 'en')).toEqual({ text: '404', tone: 'orange' });
    expect(describeCell(status, 503, 'en')).toEqual({ text: '503', tone: 'red' });
    expect(describeCell(status, 0, 'en')).toEqual({ text: '0', tone: 'red' });
  });

  it('names a country in the language of the product', () => {
    expect(describeCell(field('geo.country_iso_code'), 'DE', 'en-us')?.text).toBe('Germany');
    expect(describeCell(field('geo.country_iso_code'), 'DE', 'zh-cn')?.text).toBe('德国');
  });

  it('colours the words that mean something and leaves the rest', () => {
    expect(describeCell(field('error.handling'), 'unhandled', 'en')?.tone).toBe('orange');
    expect(describeCell(field('error.handling'), 'handled', 'en')?.tone).toBeNull();
    expect(describeCell(field('action.frustration.type'), 'rage_click', 'en')?.tone).toBe('yellow');
    expect(describeCell(field('error.is_crash', 'bool'), true, 'en')).toEqual({ text: 'true', tone: 'red' });
    expect(describeCell(field('session.has_replay', 'bool'), true, 'en')).toEqual({ text: 'true', tone: 'blue' });
    expect(describeCell(field('session.has_replay', 'bool'), false, 'en')).toEqual({ text: 'false', tone: null });
  });

  it('shortens a document to one line', () => {
    const cell = describeCell(field('context', 'json'), { plan: 'x'.repeat(400) }, 'en');

    expect(cell?.text.length).toBeLessThanOrEqual(161);
    expect(cell?.text.endsWith('…')).toBe(true);
  });

  it('writes a moment as the time of a row is, and not as a length of time', () => {
    const lastActive = field('session.last_activity', 'int64', 'epoch_ms');
    const at = Date.parse('2026-05-23T10:00:00Z');
    const asked: number[] = [];
    const clock = (millis: number) => {
      asked.push(millis);
      return 'when';
    };

    expect(describeCell(lastActive, at, 'en', clock)).toEqual({ text: 'when', tone: null });
    expect(asked).toEqual([at]);

    // Without the page's clock, in the zone and the way of the reader's preferences.
    setActiveTimePreference('UTC', 'iso_24h', 'yyyy_mm_dd_dash');
    try {
      expect(describeCell(lastActive, at, 'en')?.text).toBe('2026-05-23 10:00:00');
    } finally {
      setActiveTimePreference('', 'iso_24h');
    }
  });
});

describe('columns', () => {
  it('put numbers at the right and give text the room it needs', () => {
    expect(isNumericColumn(field('session.duration', 'int64', 'ns'))).toBe(true);
    expect(isNumericColumn(field('resource.status_code', 'int64'))).toBe(false);
    expect(columnLayout(field('session.view.count', 'int64')).align).toBe('end');
    // How long the row lasted is the time and its bar, as wide as a trace list's column.
    expect(columnLayout(field('session.duration', 'int64', 'ns'))).toEqual({ track: '148px', align: 'start' });
    expect(columnLayout(field('error.message')).track).toContain('2.4fr');
    expect(columnLayout(field('view.url_path')).track).toContain('2.4fr');
    expect(columnLayout(field('browser.name')).track).toBe('minmax(96px, 1fr)');
    expect(columnLayout(field('view.id')).align).toBe('start');
  });

  it('give a moment the room and the side of the time every row starts with', () => {
    const start = field('session.start', 'int64', 'epoch_ms');

    expect(isNumericColumn(start)).toBe(false);
    expect(columnLayout(start)).toEqual({ track: TIME_TRACK, align: 'start' });
  });
});

describe('RumCell', () => {
  it('draws a dash for an empty cell and the words of a full one', () => {
    const empty = render(<RumCell field={field('usr.id')} value={null} language="en" />);
    expect(empty.container.textContent).toBe('—');
    empty.unmount();

    const full = render(<RumCell field={field('resource.status_code', 'int64')} value={503} language="en" />);
    expect(full.container.textContent).toBe('503');
    expect(full.container.querySelector('span')?.className).toContain('red-ink');
  });

  it('writes a moment by the clock it is given, in figures of one width', () => {
    const cell = render(
      <RumCell
        field={field('session.last_activity', 'int64', 'epoch_ms')}
        value={Date.parse('2026-05-23T10:00:00Z')}
        language="en"
        moment={(millis) => new Date(millis).toISOString()}
      />,
    );

    expect(cell.container.textContent).toBe('2026-05-23T10:00:00.000Z');
    expect(cell.container.querySelector('span')?.className).toContain('tabular-nums');
  });

  it('draws how long a row lasted as a bar against the longest listed, red for a row that went wrong', () => {
    const half = render(
      <RumCell field={field('session.duration', 'int64', 'ns')} value={30_000_000_000} longest={60_000_000_000} language="en" />,
    );
    expect(half.container.querySelector('[data-duration-bar]')?.textContent).toBe('30.0 s');
    const fill = half.container.querySelector<HTMLElement>('[data-duration-bar] [aria-hidden] > span');
    expect(fill?.style.width).toBe('50%');
    expect(fill?.className).toContain('chart-1');
    half.unmount();

    const failed = render(
      <RumCell field={field('session.duration', 'int64', 'ns')} value={1} longest={60_000_000_000} failed language="en" />,
    );
    // The shortest still shows a little bar.
    const shortest = failed.container.querySelector<HTMLElement>('[data-duration-bar] [aria-hidden] > span');
    expect(shortest?.style.width).toBe('2%');
    expect(shortest?.className).toContain('bg-red');
  });
});
