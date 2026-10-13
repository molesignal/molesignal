import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useSpotlight } from './useSpotlight';

const originalMatchMedia = window.matchMedia;

function setHover(hover: boolean) {
  window.matchMedia = ((query: string) => ({
    matches: hover && query.includes('hover: hover'),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

function Probe() {
  useSpotlight();
  return (
    <div>
      <div data-testid="card" data-spotlight>
        <span data-testid="inner">card</span>
      </div>
      <div data-testid="plain" />
    </div>
  );
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['requestAnimationFrame', 'cancelAnimationFrame'] });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  window.matchMedia = originalMatchMedia;
});

function move(target: Element, clientX: number, clientY: number) {
  act(() => {
    target.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX, clientY }));
    vi.advanceTimersByTime(32);
  });
}

describe('useSpotlight', () => {
  it('writes the pointer position, relative to the card, onto the card under it', () => {
    setHover(true);
    const { getByTestId } = render(<Probe />);
    const card = getByTestId('card');
    card.getBoundingClientRect = () => ({ left: 100, top: 40 }) as DOMRect;

    // The pointer is over a child; the card, not the child, receives the position.
    move(getByTestId('inner'), 130, 90);
    expect(card.style.getPropertyValue('--mx')).toBe('30px');
    expect(card.style.getPropertyValue('--my')).toBe('50px');
  });

  it('ignores the pointer anywhere else', () => {
    setHover(true);
    const { getByTestId } = render(<Probe />);
    move(getByTestId('plain'), 10, 10);
    expect(getByTestId('card').style.getPropertyValue('--mx')).toBe('');
  });

  it('attaches nothing where there is no hover', () => {
    setHover(false);
    const { getByTestId } = render(<Probe />);
    const card = getByTestId('card');
    card.getBoundingClientRect = () => ({ left: 0, top: 0 }) as DOMRect;
    move(getByTestId('inner'), 5, 5);
    expect(card.style.getPropertyValue('--mx')).toBe('');
  });
});
