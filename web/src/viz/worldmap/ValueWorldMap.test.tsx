import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { type MapValue, ValueWorldMap, type ValueWorldMapLabels } from './ValueWorldMap';

afterEach(cleanup);

const LABELS: ValueWorldMapLabels = {
  zoomIn: 'Zoom in',
  zoomOut: 'Zoom out',
  reset: 'Show the whole map',
  zoomLevel: (zoom) => `Zoom ${zoom.toFixed(1)}×`,
  noData: 'No data',
  keys: 'Press plus and minus to zoom',
  wheelHint: 'Hold Ctrl and scroll to zoom',
};

const VALUES: MapValue[] = [
  { code: 'US', value: 120, name: 'United States', text: '120 sessions' },
  { code: 'DE', value: 30, name: 'Germany', text: '30 sessions' },
  // Too small to have a shape of its own: a marker.
  { code: 'SG', value: 8, name: 'Singapore', text: '8 sessions' },
];

const nameOf = (code: string | null, english: string) => (code === 'FR' ? 'France!' : english);

function renderMap() {
  return render(
    <ValueWorldMap
      values={VALUES}
      ariaLabel="Sessions by country"
      labels={LABELS}
      legend={{ label: 'Sessions', low: '8', high: '120' }}
      nameOf={nameOf}
    />,
  );
}

const zoomOf = (container: HTMLElement) => container.querySelector('[data-world-map]')?.getAttribute('data-map-zoom');

describe('ValueWorldMap', () => {
  it('draws every country of the world, tints the ones that have a number, and marks the ones too small to draw', () => {
    const { container } = renderMap();

    expect(container.querySelectorAll('path[data-world-country]').length).toBeGreaterThan(200);
    expect([...container.querySelectorAll('[data-map-country]')].map((element) => element.getAttribute('data-map-country')).sort()).toEqual([
      'DE',
      'SG',
      'US',
    ]);
    expect(container.querySelectorAll('[data-map-marker]')).toHaveLength(1);
    expect(screen.getByRole('group', { name: 'Sessions by country' })).toBeTruthy();
    expect(container.querySelector('[data-map-legend]')?.textContent).toContain('120');
  });

  it('says what it shows to a reader who does not see it', () => {
    renderMap();

    const items = screen.getAllByRole('listitem').map((item) => item.textContent);
    expect(items).toEqual(['United States: 120 sessions', 'Germany: 30 sessions', 'Singapore: 8 sessions']);
    expect(screen.getByText('Press plus and minus to zoom')).toBeTruthy();
  });

  it('says what a country is and what it came to when the pointer is on it', () => {
    const { container } = renderMap();

    fireEvent.pointerEnter(container.querySelector('[data-map-country="US"]') as Element, { clientX: 100, clientY: 80 });
    expect(screen.getByRole('tooltip').textContent).toContain('United States');
    expect(screen.getByRole('tooltip').textContent).toContain('120 sessions');

    fireEvent.pointerLeave(container.querySelector('[data-map-country="US"]') as Element);
    expect(screen.queryByRole('tooltip')).toBeNull();

    fireEvent.pointerEnter(container.querySelector('[data-map-marker]') as Element, { clientX: 100, clientY: 80 });
    expect(screen.getByRole('tooltip').textContent).toContain('Singapore');

    // A country with no number is a country all the same.
    fireEvent.pointerLeave(container.querySelector('[data-map-marker]') as Element);
    const land = container.querySelector('path[data-world-country]:not([data-map-country])') as Element;
    fireEvent.pointerEnter(land, { clientX: 100, clientY: 80 });
    expect(screen.getByRole('tooltip').textContent).toContain('No data');
  });

  it('zooms with the buttons, no further than its limits, and shows the whole map again', () => {
    const { container } = renderMap();

    expect(zoomOf(container)).toBe('1.00');
    expect((screen.getByRole('button', { name: 'Zoom out' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Show the whole map' }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }));
    expect(zoomOf(container)).toBe('1.50');
    expect(container.querySelector('[data-map-viewport]')?.getAttribute('transform')).toContain('scale(1.5)');
    expect(screen.getByText('Zoom 1.5×')).toBeTruthy();

    for (let press = 0; press < 12; press += 1) fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }));
    expect(zoomOf(container)).toBe('8.00');
    expect((screen.getByRole('button', { name: 'Zoom in' }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: 'Zoom out' }));
    expect(Number(zoomOf(container))).toBeLessThan(8);

    fireEvent.click(screen.getByRole('button', { name: 'Show the whole map' }));
    expect(zoomOf(container)).toBe('1.00');
  });

  it('zooms with the keys, and moves a zoomed map with the arrows', () => {
    const { container } = renderMap();
    const map = screen.getByRole('group', { name: 'Sessions by country' });

    fireEvent.keyDown(map, { key: '+' });
    fireEvent.keyDown(map, { key: '+' });
    expect(zoomOf(container)).toBe('2.25');

    const before = container.querySelector('[data-map-viewport]')?.getAttribute('transform');
    fireEvent.keyDown(map, { key: 'ArrowRight' });
    expect(container.querySelector('[data-map-viewport]')?.getAttribute('transform')).not.toBe(before);

    fireEvent.keyDown(map, { key: '-' });
    expect(Number(zoomOf(container))).toBeLessThan(2.25);

    fireEvent.keyDown(map, { key: '0' });
    expect(zoomOf(container)).toBe('1.00');
  });

  it('zooms about the pointer with the wheel, and leaves the wheel to the page where it can zoom no further', () => {
    const { container } = renderMap();
    const svg = container.querySelector('svg') as SVGSVGElement;

    // Out, on a map that shows all of itself: the page's.
    const out = new WheelEvent('wheel', { deltaY: 120, bubbles: true, cancelable: true });
    act(() => {
      svg.dispatchEvent(out);
    });
    expect(out.defaultPrevented).toBe(false);
    expect(zoomOf(container)).toBe('1.00');

    const zoomIn = new WheelEvent('wheel', { deltaY: -200, bubbles: true, cancelable: true, clientX: 10, clientY: 10 });
    act(() => {
      svg.dispatchEvent(zoomIn);
    });
    expect(zoomIn.defaultPrevented).toBe(true);
    expect(Number(zoomOf(container))).toBeGreaterThan(1);

    // In, on a map zoomed all the way in: the page's again.
    for (let press = 0; press < 12; press += 1) fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }));
    const further = new WheelEvent('wheel', { deltaY: -200, bubbles: true, cancelable: true });
    act(() => {
      svg.dispatchEvent(further);
    });
    expect(further.defaultPrevented).toBe(false);
  });

  it('leaves the plain wheel to the page when it is asked to, says how to zoom, and zooms with the control key', () => {
    const { container } = render(
      <ValueWorldMap values={VALUES} ariaLabel="Sessions by country" labels={LABELS} nameOf={nameOf} wheelZoom="modifier" />,
    );
    const svg = container.querySelector('svg') as SVGSVGElement;

    const plain = new WheelEvent('wheel', { deltaY: -200, bubbles: true, cancelable: true });
    act(() => {
      svg.dispatchEvent(plain);
    });
    expect(plain.defaultPrevented).toBe(false);
    expect(zoomOf(container)).toBe('1.00');
    expect(screen.getByText('Hold Ctrl and scroll to zoom')).toBeTruthy();

    const held = new WheelEvent('wheel', { deltaY: -200, ctrlKey: true, bubbles: true, cancelable: true });
    act(() => {
      svg.dispatchEvent(held);
    });
    expect(held.defaultPrevented).toBe(true);
    expect(Number(zoomOf(container))).toBeGreaterThan(1);
  });
});
