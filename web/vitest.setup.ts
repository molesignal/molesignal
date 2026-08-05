import '@testing-library/jest-dom/vitest';

// jsdom missing pieces commonly required by our components.
if (typeof globalThis.ResizeObserver === 'undefined') {
  // @ts-expect-error polyfill
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}
if (typeof window !== 'undefined' && !window.matchMedia) {
  // @ts-expect-error polyfill
  window.matchMedia = (q: string) => ({
    matches: false,
    media: q,
    addEventListener: () => {},
    removeEventListener: () => {},
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  });
}
