import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import type { DiffFunction } from '@/viz/profiles/flamebearer';

import { ChangedFunctions } from './ChangedFunctions';

const ROWS: DiffFunction[] = [
  { name: 'database.query', baseline: 2_320_000_000, comparison: 8_900_000_000, delta: 6_580_000_000, deltaPct: 283.6 },
  { name: 'cache.evict', baseline: 0, comparison: 400_000_000, delta: 400_000_000, deltaPct: null },
  { name: 'json.legacy', baseline: 900_000_000, comparison: 0, delta: -900_000_000, deltaPct: -100 },
  { name: 'crypto.hash', baseline: 1_750_000_000, comparison: 1_500_000_000, delta: -250_000_000, deltaPct: -14.3 },
];

function renderChanged(props: Partial<React.ComponentProps<typeof ChangedFunctions>> = {}) {
  const onSelect = vi.fn();
  render(
    <ChangedFunctions rows={ROWS} units="nanoseconds" selected={null} onSelect={onSelect} {...props} />,
  );

  return { onSelect, row: (name: string) => screen.getByRole('button', { name: new RegExp(name) }) };
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

describe('ChangedFunctions', () => {
  it('says what each function cost before and after, and which way it went', () => {
    const { row } = renderChanged();

    expect(screen.getByText('4 functions changed')).toBeTruthy();
    expect(row('database.query').textContent).toBe('database.query2.32 s8.9 s+6.58 s+284%');
    expect(row('crypto.hash').textContent).toBe('crypto.hash1.75 s1.5 s-250 ms-14.3%');
    // More is the tone of a regression, less the tone of a gain.
    expect(row('database.query').innerHTML).toContain('--red-ink');
    expect(row('crypto.hash').innerHTML).toContain('--green-ink');
  });

  it('calls a function that was not there before new, and one that is no longer there gone', () => {
    const { row } = renderChanged();

    expect(row('cache.evict').textContent).toBe('cache.evict—400 ms+400 msnew');
    expect(row('json.legacy').textContent).toBe('json.legacy900 ms—-900 msgone');
  });

  it('hands the function picked to the page, by pointer or by key, and marks it', () => {
    const { onSelect, row } = renderChanged({ selected: 'crypto.hash' });

    fireEvent.click(row('database.query'));
    expect(onSelect).toHaveBeenLastCalledWith('database.query');
    fireEvent.keyDown(row('json.legacy'), { key: 'Enter' });
    expect(onSelect).toHaveBeenLastCalledWith('json.legacy');

    expect(row('crypto.hash').getAttribute('data-state')).toBe('selected');
    expect(row('crypto.hash').getAttribute('aria-pressed')).toBe('true');
    expect(row('database.query').getAttribute('data-state')).toBeNull();
  });

  it('shows the ten largest changes until the rest are asked for', () => {
    const many: DiffFunction[] = Array.from({ length: 14 }, (_, index) => ({
      name: `fn_${index}`,
      baseline: 100,
      comparison: 200 + index,
      delta: 100 + index,
      deltaPct: 100 + index,
    }));
    render(<ChangedFunctions rows={many} units="count" selected={null} onSelect={() => {}} />);
    const table = screen.getByRole('table');

    expect(within(table).getAllByRole('button')).toHaveLength(10);
    fireEvent.click(screen.getByRole('button', { name: 'Show all 14' }));
    expect(within(table).getAllByRole('button')).toHaveLength(14);
    fireEvent.click(screen.getByRole('button', { name: 'Show the top 10' }));
    expect(within(table).getAllByRole('button')).toHaveLength(10);
  });

  it('says so when nothing changed, with no table', () => {
    render(<ChangedFunctions rows={[]} units="count" selected={null} onSelect={() => {}} />);

    expect(screen.getByText("No function's own cost changed between the two periods.")).toBeTruthy();
    expect(screen.queryByRole('table')).toBeNull();
    expect(screen.getByText('0 functions changed')).toBeTruthy();
  });
});
