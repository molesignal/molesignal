import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_USER_PREFERENCES } from '@/api/me';

import type { LogEntry } from '../viewModel';
import { LogPatternsView, type PatternSampleState } from './View';

function entry(second: number, message: string | null, level = 'INFO', service = 'payments'): LogEntry {
  const micros = Date.parse(`2026-05-23T09:59:${String(second).padStart(2, '0')}.250Z`) * 1_000;

  return {
    ts: `2026-05-23 09:59:${String(second).padStart(2, '0')}`,
    micros,
    level: level as LogEntry['level'],
    raw: { _timestamp: micros, level, 'service.name': service, ...(message === null ? {} : { message }) },
  };
}

const ROWS: LogEntry[] = [
  entry(58, 'payment authorized for order 8f31c2 in 182ms'),
  entry(57, 'upstream timeout after 3 retries', 'ERROR', 'gateway'),
  entry(56, 'payment authorized for order 77aa01 in 95ms'),
  entry(55, 'payment authorized for order 1b2c3d in 240ms', 'WARN', 'checkout-api'),
  entry(54, null),
];

afterEach(() => cleanup());

function renderPatterns(props: Partial<React.ComponentProps<typeof LogPatternsView>> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  client.setQueryData(['me', 'preferences'], { ...DEFAULT_USER_PREFERENCES, timezone: 'UTC' });
  const onOpenLog = vi.fn();
  const onFilter = vi.fn();

  const view = render(
    <QueryClientProvider client={client}>
      <LogPatternsView rows={ROWS} onOpenLog={onOpenLog} onFilter={onFilter} {...props} />
    </QueryClientProvider>,
  );

  return { ...view, onOpenLog, onFilter };
}

const patternRows = (root: HTMLElement) => Array.from(root.querySelectorAll<HTMLElement>('[data-log-pattern]'));

describe('LogPatternsView', () => {
  it('shows each kind of message once, with how many logs say it and how they divide', () => {
    const { container } = renderPatterns();
    const [payments, timeout] = patternRows(container);

    expect(patternRows(container)).toHaveLength(2);
    expect(screen.getByText('2 patterns')).not.toBeNull();

    // Three of the four logs that say something: the number once, and again as a length.
    expect(payments?.dataset.logPattern).toBe('payment authorized for order <any> in <number>ms');
    expect(within(payments!).getByText('3')).not.toBeNull();
    const share = payments?.querySelector<HTMLElement>('[data-log-pattern-share]');
    expect(share?.style.width).toBe('75%');
    expect(share?.closest('[title]')?.getAttribute('title')).toBe('75%');
    expect(payments?.textContent).not.toContain('%');
    // The loudest level is the row's, and comes first; the rest follow with their counts.
    expect(payments?.dataset.logLevel).toBe('warn');
    expect(payments?.querySelector('[data-log-pattern-levels]')?.textContent).toBe('WARN1INFO2');
    expect(payments?.textContent).toContain('payments +1');

    // A pattern of one level says the level alone: how many is the pattern's own count.
    expect(timeout?.dataset.logPattern).toBe('upstream timeout after <number> retries');
    expect(timeout?.dataset.logLevel).toBe('error');
    expect(timeout?.querySelector('[data-log-pattern-levels]')?.textContent).toBe('ERROR');
    expect(timeout?.querySelector<HTMLElement>('[data-log-pattern-share]')?.style.width).toBe('25%');
  });

  it('names the three loudest levels of a pattern and counts the rest', () => {
    const levels = ['DEBUG', 'TRACE', 'INFO', 'WARN', 'ERROR'];
    const { container } = renderPatterns({
      rows: levels.map((level, index) => entry(50 + index, 'cache refreshed', level)),
    });
    const cell = patternRows(container)[0]?.querySelector<HTMLElement>('[data-log-pattern-levels]');

    expect(cell?.textContent).toBe('ERROR1WARN1INFO1+2');
    expect(cell?.title).toBe('ERROR 1 · WARN 1 · INFO 1 · DEBUG 1 · TRACE 1');
  });

  it('draws a place where every log says something of its own as an open slot', () => {
    const { container } = renderPatterns();
    const text = patternRows(container)[0]?.querySelector('[data-log-pattern-text]');

    expect(text?.textContent).toBe('payment authorized for order * in *ms');
    const slots = Array.from(text?.querySelectorAll<HTMLElement>('[data-log-pattern-slot]') ?? []);
    expect(slots.map((slot) => slot.dataset.logPatternSlot)).toEqual(['any', 'number']);
    // What stood there in one of the logs is a hover away.
    expect(slots[1]?.title).toBe('A number: 182ms');
  });

  it('opens a pattern on its logs, and a log from there', async () => {
    const user = userEvent.setup();
    const { container, onOpenLog } = renderPatterns();
    const payments = patternRows(container)[0]!;
    const toggle = within(payments).getByRole('button', { name: /3 events\. Show their logs$/ });

    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    await user.click(payments.querySelector('[data-log-pattern-text]')!);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');

    // Each log says its own level and source here, since the pattern has more than one of each.
    const examples = payments.querySelector<HTMLElement>('[data-log-pattern-examples]')!;
    expect(within(examples).getAllByRole('button').map((button) => button.textContent)).toEqual([
      '09:59:58.250INFOpayment authorized for order 8f31c2 in 182mspayments',
      '09:59:56.250INFOpayment authorized for order 77aa01 in 95mspayments',
      '09:59:55.250WARNpayment authorized for order 1b2c3d in 240mscheckout-api',
    ]);

    // The log is opened by its place among the loaded rows, not among the pattern's.
    await user.click(within(examples).getByText('payment authorized for order 1b2c3d in 240ms'));
    expect(onOpenLog).toHaveBeenCalledExactlyOnceWith(3);

    // One pattern is open at a time.
    await user.click(patternRows(container)[1]!.querySelector('[data-log-pattern-text]')!);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
  });

  it('leaves a log to say only when it was and what it said where the pattern says the rest', async () => {
    const user = userEvent.setup();
    const { container } = renderPatterns({ selectedIndex: 1 });
    const timeout = patternRows(container)[1]!;

    await user.click(timeout.querySelector('[data-log-pattern-text]')!);
    const [example] = within(timeout.querySelector<HTMLElement>('[data-log-pattern-examples]')!).getAllByRole('button');

    expect(example?.textContent).toBe('09:59:57.250upstream timeout after 3 retries');
    // The log open in the drawer is marked where it stands.
    expect(example?.getAttribute('aria-pressed')).toBe('true');
  });

  it('shows the first logs of a pattern, and more of them when asked', async () => {
    const user = userEvent.setup();
    const { container } = renderPatterns({
      rows: Array.from({ length: 60 }, (_, index) => entry(59 - (index % 60), `request ${index} completed`)),
    });
    const requests = patternRows(container)[0]!;

    await user.click(requests.querySelector('[data-log-pattern-text]')!);
    const examples = requests.querySelector<HTMLElement>('[data-log-pattern-examples]')!;
    expect(examples.querySelectorAll('[data-log-pattern-example]')).toHaveLength(6);

    await user.click(within(examples).getByRole('button', { name: 'Show 50 more' }));
    expect(examples.querySelectorAll('[data-log-pattern-example]')).toHaveLength(56);

    await user.click(within(examples).getByRole('button', { name: 'Show 4 more' }));
    expect(examples.querySelectorAll('[data-log-pattern-example]')).toHaveLength(60);
    expect(within(examples).queryByRole('button', { name: /^Show \d+ more$/ })).toBeNull();
  });

  it('asks the query for a pattern by the words all of its logs have, without opening it', async () => {
    const user = userEvent.setup();
    const { container, onFilter } = renderPatterns();
    const payments = patternRows(container)[0]!;

    await user.click(within(payments).getByRole('button', { name: /^Show only logs like payment authorized/ }));

    expect(onFilter).toHaveBeenCalledExactlyOnceWith('message', 'payment authorized for order');
    expect(payments.querySelector('[data-log-pattern-examples]')).toBeNull();
  });

  it('offers no filter where the query cannot say one', () => {
    const { container } = renderPatterns({ onFilter: undefined });

    expect(within(container).queryByRole('button', { name: /^Show only logs like/ })).toBeNull();
  });

  it('says how many logs say nothing and are in no pattern', () => {
    renderPatterns();

    expect(screen.getByText('1 event carries no message and is in no pattern.')).not.toBeNull();
  });

  describe('what the patterns were read from', () => {
    const sample = (state: Partial<PatternSampleState> = {}): PatternSampleState => ({
      held: true,
      reading: false,
      more: false,
      failed: false,
      step: 500,
      onReadMore: vi.fn(),
      onRetry: vi.fn(),
      ...state,
    });
    const basis = (root: HTMLElement) => root.querySelector<HTMLElement>('[data-log-patterns-basis]')!;

    it('is all there is, for a statement the reader wrote', () => {
      const { container } = renderPatterns();

      expect(basis(container).textContent).toBe('The events the query returned');
      expect(within(basis(container)).queryByRole('button')).toBeNull();
    });

    it('is the latest of what matches, back to a moment, with the way to read further', async () => {
      const user = userEvent.setup();
      const state = sample({ more: true });
      const { container } = renderPatterns({ sample: state });

      expect(basis(container).textContent).toBe('The latest events that match·back to 09:59:54.250Read 500 older');
      await user.click(within(basis(container)).getByRole('button', { name: 'Read 500 older' }));
      expect(state.onReadMore).toHaveBeenCalledTimes(1);
    });

    it('is everything that matches once there is nothing older', () => {
      const { container } = renderPatterns({ sample: sample() });

      expect(basis(container).textContent).toBe('Every event that matches');
      expect(within(basis(container)).queryByRole('button')).toBeNull();
    });

    it('says that it is being read, and offers nothing meanwhile', () => {
      const { container } = renderPatterns({ sample: sample({ reading: true, more: true }) });

      expect(within(basis(container)).getByRole('status').textContent).toBe('Reading…');
      expect(within(basis(container)).queryByRole('button')).toBeNull();
    });

    it('does not call the page the list had the latest logs, before the first answer', () => {
      const { container } = renderPatterns({ sample: sample({ held: false, reading: true }) });

      expect(basis(container).textContent).toBe('Reading…');
    });

    it('says that reading stopped, and asks again', async () => {
      const user = userEvent.setup();
      const state = sample({ failed: true, more: true });
      const { container } = renderPatterns({ sample: state });

      expect(within(basis(container)).getByRole('alert').textContent).toBe('Could not read further back.');
      await user.click(within(basis(container)).getByRole('button', { name: 'Retry' }));
      expect(state.onRetry).toHaveBeenCalledTimes(1);
    });
  });

  it('can be opened from the keyboard: the row has one control of its own', async () => {
    const user = userEvent.setup();
    const { container } = renderPatterns();
    const toggle = within(patternRows(container)[0]!).getByRole('button', { name: /Show their logs$/ });

    toggle.focus();
    await user.keyboard('{Enter}');

    expect(toggle.getAttribute('aria-expanded')).toBe('true');
  });
});
