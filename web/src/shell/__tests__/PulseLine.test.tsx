import { cleanup, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { PulseLine } from '@/shell/PulseLine';

afterEach(cleanup);

describe('PulseLine', () => {
  it('is decorative: hidden from assistive technology and not focusable', () => {
    const { container } = render(<PulseLine />);
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('aria-hidden')).toBe('true');
    expect(svg?.getAttribute('focusable')).toBe('false');
  });

  it('empty: a flat line, a dotted tail and one beat that draws itself in', () => {
    const { container } = render(<PulseLine variant="empty" />);
    expect(container.querySelectorAll('path')).toHaveLength(3);
    const beat = container.querySelector('path.ms-pulse-draw');
    expect(beat).not.toBeNull();
    // A normalised path length lets one dash definition fit any size of mark.
    expect(beat?.getAttribute('pathLength')).toBe('1');
    expect(container.querySelector('path[stroke-dasharray]')).not.toBeNull();
  });

  it('travel: a faint line with a bright beat running along it', () => {
    const { container } = render(<PulseLine variant="travel" />);
    expect(container.querySelectorAll('path')).toHaveLength(2);
    expect(container.querySelector('path.ms-pulse-travel')).not.toBeNull();
    expect(container.querySelector('.ms-pulse-draw')).toBeNull();
  });

  it('mark: the whole line in the signal gradient', () => {
    const { container } = render(<PulseLine variant="mark" />);
    const path = container.querySelector('path');
    expect(path?.getAttribute('stroke')).toMatch(/^url\(#.+\)$/);
    expect(container.querySelectorAll('linearGradient stop')).toHaveLength(3);
  });

  it('gives every instance its own gradient id', () => {
    const { container } = render(
      <>
        <PulseLine />
        <PulseLine />
      </>,
    );
    const ids = Array.from(container.querySelectorAll('linearGradient')).map((g) => g.id);
    expect(new Set(ids).size).toBe(2);
  });
});
