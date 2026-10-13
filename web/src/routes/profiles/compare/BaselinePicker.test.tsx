import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { setActiveTimePreference } from '@/lib/time';

import { BaselinePicker } from './BaselinePicker';
import { type CompareBaseline, type Period, periodWindow } from './periods';

const HOUR = 3_600 * 1_000_000;
const DAY = 24 * HOUR;
const at = (iso: string) => new Date(iso).getTime() * 1000;
const hour: Period = { from: at('2026-10-12T15:00:00Z'), to: at('2026-10-12T16:00:00Z') };
const before: Period = { from: hour.from - HOUR, to: hour.from };

function renderPicker(
  value: CompareBaseline = { kind: 'previous' },
  comparison: Period = hour,
  resolved: Period = before,
) {
  const onChange = vi.fn();
  render(<BaselinePicker value={value} comparison={comparison} resolved={resolved} onChange={onChange} />);

  return onChange;
}

const field = () => screen.getByRole('button', { name: /^Baseline:/ });
const open = () => {
  fireEvent.click(field());

  return screen.getByRole('dialog', { name: 'Choose the baseline' });
};

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  setActiveTimePreference('UTC', 'iso_24h');
});

afterEach(cleanup);

describe('BaselinePicker', () => {
  it('names the baseline it stands for, and its times for a reader who cannot see them beside it', () => {
    renderPicker({ kind: 'day' }, hour, { from: hour.from - DAY, to: hour.to - DAY });
    expect(field().textContent).toBe('Same time a day earlier');
    expect(field().getAttribute('aria-label')).toBe(
      'Baseline: Same time a day earlier, 2026-10-11 15:00 – 2026-10-11 16:00',
    );

    cleanup();
    renderPicker({ kind: 'window', window: { from: 'now-6h', to: 'now', mode: 'relative' } });
    expect(field().textContent).toBe('Last 6 hours');

    cleanup();
    const own: Period = { from: at('2026-10-01T08:00:00Z'), to: at('2026-10-01T09:30:00Z') };
    renderPicker({ kind: 'window', window: periodWindow(own) }, hour, own);
    // Its times stand beside the control; the control says what kind of baseline it is.
    expect(field().textContent).toBe('A period of its own');
  });

  it('sets a baseline that follows the period', () => {
    const onChange = renderPicker();
    const panel = open();

    expect(within(panel).getByRole('button', { name: 'The period before' }).getAttribute('aria-pressed')).toBe('true');
    fireEvent.click(within(panel).getByRole('button', { name: 'Same time a week earlier' }));

    expect(onChange).toHaveBeenCalledWith({ kind: 'week' });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('does not offer a relation that would overlap the period', () => {
    renderPicker({ kind: 'previous' }, { from: hour.to - 3 * DAY, to: hour.to });
    open();

    const day = screen.getByRole('button', { name: 'Same time a day earlier' }) as HTMLButtonElement;
    expect(day.disabled).toBe(true);
    expect(day.title).toBe('This baseline would overlap the period it is compared with.');
    expect((screen.getByRole('button', { name: 'Same time a week earlier' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('offers the quick ranges the period itself is set with', () => {
    const onChange = renderPicker();
    const panel = open();

    // While the baseline follows the period, none of the quick ranges is the one in use.
    expect(within(panel).getByRole('button', { name: 'Last 7 days' }).getAttribute('aria-pressed')).toBe('false');
    fireEvent.click(within(panel).getByRole('button', { name: 'Last 7 days' }));

    expect(onChange).toHaveBeenCalledWith({
      kind: 'window',
      window: { from: 'now-7d', to: 'now', mode: 'relative' },
    });
    expect(screen.queryByRole('dialog')).toBeNull();

    cleanup();
    renderPicker({ kind: 'window', window: { from: 'now-7d', to: 'now', mode: 'relative' } });
    expect(within(open()).getByRole('button', { name: 'Last 7 days' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('sets a start and an end of its own, beginning from the baseline as it stands', () => {
    const onChange = renderPicker();
    const panel = open();

    // The two fields say the hour before the period: applied as they are, that hour is held.
    fireEvent.click(within(panel).getByRole('button', { name: 'Apply' }));

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({ kind: 'window', window: periodWindow(before) });
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
