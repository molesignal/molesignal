import type uPlot from 'uplot';
import { describe, expect, it, vi } from 'vitest';

import { buildYAxisSize } from '@/viz/timeseries/axisSizing';

function plotWithMeasuredCharacterWidth(width: number): uPlot {
  const context = {
    font: '',
    measureText: vi.fn((value: string) => ({ width: value.length * width })),
    restore: vi.fn(),
    save: vi.fn(),
  } as unknown as CanvasRenderingContext2D;
  return { ctx: context } as unknown as uPlot;
}

describe('buildYAxisSize', () => {
  it('keeps the compact minimum for short tick values', () => {
    const size = buildYAxisSize('600 11px sans-serif', true);

    expect(size(plotWithMeasuredCharacterWidth(4), ['0', '1'], 1, 1)).toBe(58);
  });

  it('reserves enough room for scientific notation without entering the title lane', () => {
    const size = buildYAxisSize('600 11px sans-serif', true);

    expect(size(plotWithMeasuredCharacterWidth(6), ['5.00e-4 req/s'], 1, 1)).toBe(
      90,
    );
  });
});
