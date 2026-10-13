import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import { setActiveTimePreference } from '@/lib/time';

import type { Period } from './periods';
import { CompareStatement } from './Statement';

const at = (iso: string) => new Date(iso).getTime() * 1000;
const comparison: Period = { from: at('2026-10-12T16:18:00Z'), to: at('2026-10-12T16:36:00Z') };
const baseline: Period = { from: at('2026-10-12T16:00:00Z'), to: at('2026-10-12T16:18:00Z') };

function renderStatement(props: Partial<React.ComponentProps<typeof CompareStatement>> = {}) {
  render(
    <CompareStatement
      comparison={comparison}
      baseline={baseline}
      comparisonControl={<button type="button">comparison control</button>}
      baselineControl={<button type="button">baseline control</button>}
      {...props}
    />,
  );
  const statement = screen.getByRole('region', { name: 'The two periods compared' });

  return {
    statement,
    row: (name: string) => within(statement).getByRole('row', { name: new RegExp(`^${name}`) }),
  };
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  setActiveTimePreference('UTC', 'iso_24h');
});

afterEach(cleanup);

describe('CompareStatement', () => {
  it('says both periods with the control that sets each, before anything is known of them', () => {
    const { row } = renderStatement();

    expect(within(row('Comparison')).getByRole('button', { name: 'comparison control' })).toBeTruthy();
    expect(row('Comparison').textContent).toContain('2026-10-12 16:18 – 2026-10-12 16:36');
    expect(within(row('Baseline')).getByRole('button', { name: 'baseline control' })).toBeTruthy();
    expect(row('Baseline').textContent).toContain('2026-10-12 16:00 – 2026-10-12 16:18');
    expect(screen.queryByText('Overall change')).toBeNull();
  });

  it('says what each period holds and how the second differs from the first', () => {
    const { statement, row } = renderStatement({
      counts: { comparison: 12, baseline: 12 },
      totals: { baseline: 9_940_000_000, comparison: 21_910_000_000, delta: 11_970_000_000, deltaPct: 120.4 },
      units: 'nanoseconds',
    });

    expect(row('Comparison').textContent).toContain('12 profiles');
    expect(row('Comparison').textContent).toContain('21.91 s');
    expect(row('Baseline').textContent).toContain('9.94 s');
    const overall = statement.querySelector('[data-compare-overall]')!;
    expect(overall.textContent).toBe('Overall change+11.97 s · +120%');
    // More than before is the tone of a regression.
    expect(overall.innerHTML).toContain('--red-ink');
    expect(statement.querySelector('[data-compare-unequal]')).toBeNull();
  });

  it('has no change to give while one of the periods holds nothing', () => {
    const { statement, row } = renderStatement({
      counts: { comparison: 1, baseline: 0 },
      totals: { baseline: 0, comparison: 8, delta: 8, deltaPct: null },
      units: 'nanoseconds',
    });

    expect(row('Comparison').textContent).toContain('1 profile');
    expect(row('Baseline').textContent).toContain('0 profiles');
    // A period that holds nothing has no total to show.
    expect(row('Baseline').textContent).not.toContain('ns');
    expect(statement.querySelector('[data-compare-overall]')).toBeNull();
  });

  it('warns that unequal numbers of profiles are part of the change', () => {
    const { statement } = renderStatement({
      counts: { comparison: 24, baseline: 12 },
      totals: { baseline: 10, comparison: 30, delta: 20, deltaPct: 200 },
      units: 'nanoseconds',
    });

    expect(statement.querySelector('[data-compare-unequal]')!.textContent).toContain('24 against 12');
  });

  it('says so when the two periods share time', () => {
    const { statement } = renderStatement({
      baseline: { from: comparison.from - 60_000_000, to: comparison.from + 60_000_000 },
    });

    expect(statement.querySelector('[data-compare-overlap]')).not.toBeNull();
    cleanup();
    expect(renderStatement().statement.querySelector('[data-compare-overlap]')).toBeNull();
  });
});
