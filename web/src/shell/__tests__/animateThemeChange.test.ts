import { afterEach, describe, expect, it, vi } from 'vitest';

import { animateThemeChange } from '@/shell/ThemeBootstrap';

type ViewDoc = { startViewTransition?: unknown };

const originalMatchMedia = window.matchMedia;

function reducedMotion(reduced: boolean) {
  window.matchMedia = ((query: string) => ({
    matches: reduced && query.includes('prefers-reduced-motion'),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

afterEach(() => {
  delete (document as unknown as ViewDoc).startViewTransition;
  window.matchMedia = originalMatchMedia;
});

describe('animateThemeChange', () => {
  it('just applies the change where the View Transitions API is missing', () => {
    reducedMotion(false);
    const apply = vi.fn();
    animateThemeChange(document.body, apply);
    expect(apply).toHaveBeenCalledOnce();
  });

  it('just applies the change for people who asked for reduced motion', () => {
    reducedMotion(true);
    const startViewTransition = vi.fn();
    (document as unknown as ViewDoc).startViewTransition = startViewTransition;
    const apply = vi.fn();
    animateThemeChange(document.body, apply);
    expect(startViewTransition).not.toHaveBeenCalled();
    expect(apply).toHaveBeenCalledOnce();
  });

  it('runs the change inside a view transition and grows the new theme from the origin', async () => {
    reducedMotion(false);
    const animate = vi.fn();
    document.documentElement.animate = animate as unknown as typeof document.documentElement.animate;
    const startViewTransition = vi.fn((update: () => void) => {
      update();
      return { ready: Promise.resolve() };
    });
    (document as unknown as ViewDoc).startViewTransition = startViewTransition;
    const origin = document.createElement('button');
    origin.getBoundingClientRect = () =>
      ({ left: 100, top: 10, width: 40, height: 20 }) as DOMRect;
    const apply = vi.fn();

    animateThemeChange(origin, apply);
    await Promise.resolve();

    expect(startViewTransition).toHaveBeenCalledOnce();
    expect(apply).toHaveBeenCalledOnce();
    expect(animate).toHaveBeenCalledOnce();
    const [keyframes, options] = animate.mock.calls[0] as [
      { clipPath: string[] },
      { pseudoElement: string },
    ];
    // The circle starts at the toggle's centre (120, 20) with no radius.
    expect(keyframes.clipPath[0]).toBe('circle(0px at 120px 20px)');
    expect(options.pseudoElement).toBe('::view-transition-new(root)');
  });
});
