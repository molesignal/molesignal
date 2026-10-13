import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { OverviewMetrics } from '@/api/rum/overview';
import i18n from '@/i18n';
import { reduceMotionInTests } from '@/shell/readout/reduceMotionInTests';

import { RumPulse } from './Pulse';

const METRICS: OverviewMetrics = {
  users: 12_840,
  sessions: 31_206,
  errorFreeRate: 0.9724,
  lcpP75: 2_340,
  inpP75: 236,
  clsP75: 0.064,
};

let restoreMotion = () => {};

function vital(key: string): HTMLElement {
  const cell = document.querySelector<HTMLElement>(`[data-rum-vital="${key}"]`);
  if (!cell) throw new Error(`no vital ${key}`);
  return cell;
}

function readout(label: string): HTMLElement {
  return screen.getByText(label).closest<HTMLElement>('[data-readout]')!;
}

beforeEach(async () => {
  restoreMotion = reduceMotionInTests();
  await i18n.changeLanguage('en-us');
});

afterEach(() => {
  cleanup();
  restoreMotion();
});

describe('RumPulse', () => {
  it('opens with the audience and what each figure means for it', () => {
    render(<RumPulse metrics={METRICS} />);

    expect(readout('Active users').textContent).toContain('12,840');
    expect(readout('Active users').textContent).toContain('2.4 sessions per user');
    expect(readout('Sessions').textContent).toContain('31,206');
    expect(readout('Error-free sessions').textContent).toContain('97.24');
    expect(readout('Error-free sessions').textContent).toContain('861 sessions with errors');
    // Between 95% and 99% is a warning, not a pass.
    expect(readout('Error-free sessions').querySelector('.text-yellow')).not.toBeNull();
  });

  it('grades each Core Web Vital and marks it on its own scale', () => {
    render(<RumPulse metrics={METRICS} />);

    expect(vital('lcp').getAttribute('data-grade')).toBe('good');
    expect(vital('inp').getAttribute('data-grade')).toBe('needs_improvement');
    expect(vital('cls').getAttribute('data-grade')).toBe('good');
    expect(vital('inp').textContent).toContain('236 ms');
    expect(vital('inp').textContent).toContain('Needs improvement');
    expect(vital('cls').textContent).toContain('0.064');

    // 236 ms on a scale that runs to 750 ms.
    const mark = vital('inp').querySelector<HTMLElement>('[data-rum-vital-mark]')!;
    expect(Number.parseFloat(mark.style.left)).toBeCloseTo((236 / 750) * 100, 4);
    expect(
      screen.getByRole('img', { name: 'INP at the 75th percentile: Needs improvement' }),
    ).not.toBeNull();
  });

  it('states the thresholds a vital is graded against', () => {
    render(<RumPulse metrics={METRICS} />);

    expect(vital('lcp').textContent).toContain('≤ 2.5 s good · > 4.0 s poor');
    expect(vital('cls').textContent).toContain('≤ 0.1 good · > 0.25 poor');
  });

  it('leaves a vital without a measurement unmarked', () => {
    render(<RumPulse metrics={{ ...METRICS, lcpP75: 0 }} />);

    expect(vital('lcp').getAttribute('data-grade')).toBe('unknown');
    expect(vital('lcp').textContent).toContain('—');
    expect(vital('lcp').querySelector('[data-rum-vital-mark]')).toBeNull();
  });

  it('draws sessions over the window once the trend has arrived', () => {
    const { rerender } = render(<RumPulse metrics={METRICS} />);
    expect(readout('Sessions').querySelector('[data-sparkline]')).toBeNull();

    rerender(
      <RumPulse
        metrics={METRICS}
        trend={[
          { start: 1, good: 8, needs: 1, poor: 1 },
          { start: 2, good: 12, needs: 2, poor: 0 },
        ]}
      />,
    );
    expect(readout('Sessions').querySelectorAll('[data-sparkline="bars"] rect')).toHaveLength(2);
  });

  it('puts the comparison with the period before in place of the context', () => {
    render(
      <RumPulse
        metrics={METRICS}
        comparison={{ users: '4.2% better', lcpP75: '8.0% worse' }}
      />,
    );

    expect(readout('Active users').textContent).toContain('4.2% better');
    expect(readout('Active users').textContent).not.toContain('sessions per user');
    expect(vital('lcp').textContent).toContain('8.0% worse');
    // A figure with no comparison keeps its own context.
    expect(vital('inp').textContent).toContain('≤ 200 ms good');
  });
});
