import { describe, expect, it } from 'vitest';

import { gridMinWidth } from './gridWidth';

describe('the least width of a grid table row', () => {
  it('adds each track at its least to the gaps and the padding, in the pixels the classes give', () => {
    expect(gridMinWidth(['minmax(220px,1fr)', '112px', '96px'])).toBe('476px');
    expect(gridMinWidth(['minmax(0,1fr)'])).toBe('24px');
    expect(gridMinWidth(['96px', '96px'], 12, 40)).toBe('244px');
  });
});
