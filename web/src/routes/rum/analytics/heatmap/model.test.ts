import { describe, expect, it } from 'vitest';

import type { ClickedElement } from '@/api/rum';

import { deviceFilters, depthReached, heatColor, parseDevice, parseKind, reachedAt, spotsOf } from './model';

const element = (patch: Partial<ClickedElement>): ClickedElement => ({
  element: 'body>main>button',
  name: 'Pay',
  selector: null,
  clicks: 0,
  sessions: 1,
  frustrated: 0,
  pageX: null,
  pageY: null,
  cells: [],
  ...patch,
});

describe('the choices of a map', () => {
  it('opens on the clicks of a desktop window, and takes what it knows', () => {
    expect(parseDevice(null)).toBe('desktop');
    expect(parseDevice('mobile')).toBe('mobile');
    expect(parseDevice('watch')).toBe('desktop');
    expect(parseKind(null)).toBe('clicks');
    expect(parseKind('scroll')).toBe('scroll');
  });

  it('tells a window by its width', () => {
    expect(deviceFilters('desktop')).toEqual([{ field: 'display.viewport.width', op: '>=', value: '1024' }]);
    expect(deviceFilters('tablet')).toEqual([
      { field: 'display.viewport.width', op: '>=', value: '600' },
      { field: 'display.viewport.width', op: '<', value: '1024' },
    ]);
    expect(deviceFilters('mobile')).toEqual([{ field: 'display.viewport.width', op: '<', value: '600' }]);
    expect(deviceFilters('all')).toEqual([]);
  });
});

describe('where clicks fall on a page', () => {
  const box = { left: 100, top: 200, width: 100, height: 20 };

  it('is cell by cell over an element the recording has, the others at its middle', () => {
    const { spots, off } = spotsOf([element({ clicks: 5, cells: [[0, 0, 2], [9, 9, 1]] })], () => box);

    expect(spots).toEqual([
      { x: 105, y: 201, clicks: 2, element: 'body>main>button' },
      { x: 195, y: 219, clicks: 1, element: 'body>main>button' },
      { x: 150, y: 210, clicks: 2, element: 'body>main>button' },
    ]);
    expect(off.size).toBe(0);
  });

  it('is where on the page they landed for an element the recording does not have, which is said', () => {
    const { spots, off } = spotsOf([element({ clicks: 3, pageX: 10, pageY: 20 })], () => null);

    expect(spots).toEqual([{ x: 10, y: 20, clicks: 3, element: 'body>main>button' }]);
    expect(off.has('body>main>button')).toBe(true);
  });

  it('is nowhere when they do not say either', () => {
    const { spots, off } = spotsOf([element({ clicks: 3 })], () => null);

    expect(spots).toEqual([]);
    expect(off.has('body>main>button')).toBe(true);
  });
});

describe('the colour of heat', () => {
  it('goes from cool and see-through to hot and solid', () => {
    const [coolR, , coolB, coolA] = heatColor(0);
    const [hotR, , hotB, hotA] = heatColor(1);

    expect(coolB).toBeGreaterThan(coolR);
    expect(hotR).toBeGreaterThan(hotB);
    expect(hotA).toBeGreaterThan(coolA);
    expect(heatColor(2)).toEqual(heatColor(1));
    expect(heatColor(-1)).toEqual(heatColor(0));
  });
});

describe('how far down people went', () => {
  const bands = [
    { depth: 0, reached: 1 },
    { depth: 100, reached: 0.8 },
    { depth: 200, reached: 0.5 },
    { depth: 300, reached: 0.2 },
  ];

  it('finds the depth a share of the views got to', () => {
    expect(depthReached(bands, 0.75)).toBe(100);
    expect(depthReached(bands, 0.5)).toBe(200);
    expect(depthReached(bands, 0.25)).toBe(200);
    expect(depthReached([], 0.5)).toBeNull();
  });

  it('finds the share of the views that got to a depth', () => {
    expect(reachedAt(bands, 150)).toBe(0.8);
    expect(reachedAt(bands, 300)).toBe(0.2);
    expect(reachedAt(bands, 9_000)).toBe(0.2);
    expect(reachedAt([], 10)).toBe(0);
  });
});
