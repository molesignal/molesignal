/**
 * For tests only. Figures count up to their value unless the reader prefers
 * reduced motion; a test that reads a figure asks for reduced motion first, so
 * it sees the final number instead of wherever the count-up has got to.
 *
 * Returns the function that puts the real `matchMedia` back.
 */
export function reduceMotionInTests(): () => void {
  const original = window.matchMedia;
  window.matchMedia = ((query: string) => ({
    matches: query.includes('prefers-reduced-motion'),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
  return () => {
    window.matchMedia = original;
  };
}
