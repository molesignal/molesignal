import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_USER_PREFERENCES } from '@/api/me';

import type { LogEntry } from '../viewModel';
import { LogDetail } from './Drawer';

const MESSAGE = 'failed to reserve stock: sku=SKU-48213 requested=4 available=1';

function log(second: number, overrides: Partial<LogEntry['raw']> = {}): LogEntry {
  const micros = Date.parse(`2026-05-23T09:59:${String(second).padStart(2, '0')}.120Z`) * 1_000;

  return {
    ts: `2026-05-23 09:59:${String(second).padStart(2, '0')}`,
    micros,
    level: 'INFO',
    raw: {
      _timestamp: micros,
      level: 'INFO',
      'service.name': 'payments',
      message: `request ${second} completed`,
      ...overrides,
    },
  };
}

const OPEN: LogEntry = {
  ...log(54),
  level: 'ERROR',
  raw: {
    _timestamp: Date.parse('2026-05-23T09:59:54.120Z') * 1_000,
    level: 'ERROR',
    'service.name': 'checkout-api',
    message: MESSAGE,
    trace_id: '7c1f0e0004a94b4d2e8f3a5b6c7d8e9f',
    'http.method': 'GET',
    'http.route': '/api/v1/cart/:id',
    'http.status_code': 504,
    duration_ms: 224,
    'k8s.pod.name': 'checkout-api-7d9f8b6c5-p8m4z',
    'k8s.namespace.name': 'production',
    'user.id': 'u_1148',
    retry: false,
    error: { type: 'UpstreamTimeout', code: 504 },
  },
};
const AROUND = [log(56), log(55), OPEN, log(52), log(51)];

afterEach(() => cleanup());

function renderDetail(props: Partial<React.ComponentProps<typeof LogDetail>> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  // The reader's clock is UTC, whatever zone the tests run in.
  client.setQueryData(['me', 'preferences'], { ...DEFAULT_USER_PREFERENCES, timezone: 'UTC' });
  const handlers = {
    onInsertValueFilter: vi.fn(),
    onToggleVisibility: vi.fn(),
    onSelectContext: vi.fn(),
    onClose: vi.fn(),
    onPrev: vi.fn(),
    onNext: vi.fn(),
  };

  const view = render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <LogDetail
          log={OPEN}
          index={4}
          total={40}
          stream="application_logs"
          contextRows={AROUND}
          visibleFields={['level', 'service.name', 'message']}
          {...handlers}
          {...props}
        />
      </QueryClientProvider>
    </MemoryRouter>,
  );

  return { ...view, ...handlers };
}

const fieldNames = (root: HTMLElement) =>
  Array.from(root.querySelectorAll('[data-log-detail-field]')).map((row) => row.getAttribute('data-log-detail-field'));

describe('LogDetail', () => {
  it('says what the log is once: its level and moment, what it says, and where it came from', () => {
    const { container } = renderDetail();

    // To the millisecond, which is what orders two logs of one second.
    expect(container.querySelector('[data-log-detail-moment]')?.textContent).toBe('2026-05-23 09:59:54.120');
    expect(screen.getAllByText('ERROR').length).toBeGreaterThan(0);
    // The message stands in full at the head, and is not repeated as a row under it.
    expect(screen.getAllByText(MESSAGE)).toHaveLength(1);
    expect(container.querySelector('[data-log-detail-message]')?.textContent).toBe(MESSAGE);
    expect(fieldNames(container)).not.toContain('message');
    expect(fieldNames(container)).not.toContain('_timestamp');
    expect(screen.getByText('application_logs')).not.toBeNull();
  });

  it('lists the rest of the record field by field, a signal as the reference it is everywhere', () => {
    const { container } = renderDetail();

    expect(fieldNames(container)).toEqual([
      'level',
      'service.name',
      'trace_id',
      'http.method',
      'http.route',
      'http.status_code',
      'duration_ms',
      'k8s.pod.name',
      'k8s.namespace.name',
      'user.id',
      'retry',
      'error.type',
      'error.code',
    ]);
    const trace = container.querySelector('[data-log-detail-field="trace_id"]') as HTMLElement;
    expect(trace.querySelector('[data-signal-type="trace_id"]')?.textContent).toBe(
      '7c1f0e0004a94b4d2e8f3a5b6c7d8e9f',
    );
    // A list of terms and their values, each pair in the list itself.
    for (const term of container.querySelectorAll('dt')) expect(term.parentElement?.parentElement?.tagName).toBe('DL');
  });

  it('asks a value into the query, out of it, or into the columns of the list', async () => {
    const user = userEvent.setup();
    const { container, onInsertValueFilter, onToggleVisibility } = renderDetail();
    const status = within(container.querySelector('[data-log-detail-field="http.status_code"]') as HTMLElement);

    await user.click(status.getByRole('button', { name: 'Add include filter' }));
    expect(onInsertValueFilter).toHaveBeenLastCalledWith('http.status_code', 504, 'include');
    await user.click(status.getByRole('button', { name: 'Add exclude filter' }));
    expect(onInsertValueFilter).toHaveBeenLastCalledWith('http.status_code', 504, 'exclude');
    await user.click(status.getByRole('button', { name: 'Show as column' }));
    expect(onToggleVisibility).toHaveBeenCalledWith('http.status_code');

    // A field that is a column already offers to take it away.
    const level = within(container.querySelector('[data-log-detail-field="level"]') as HTMLElement);
    expect(level.getByRole('button', { name: 'Hide column' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('finds a field in a long record by its name or its value', async () => {
    const user = userEvent.setup();
    const { container } = renderDetail();

    await user.type(screen.getByRole('textbox', { name: 'Filter fields' }), 'http');
    expect(fieldNames(container)).toEqual(['http.method', 'http.route', 'http.status_code']);
    expect(screen.getByText('3 / 13')).not.toBeNull();

    await user.clear(screen.getByRole('textbox', { name: 'Filter fields' }));
    await user.type(screen.getByRole('textbox', { name: 'Filter fields' }), 'no-such-field');
    expect(fieldNames(container)).toEqual([]);
    expect(screen.getByText('No field matches.')).not.toBeNull();
  });

  it('has nothing to search in a record that is all in view', () => {
    renderDetail({ log: log(52) });

    expect(screen.queryByRole('textbox', { name: 'Filter fields' })).toBeNull();
  });

  it('shows the record as it was stored, and the results around it', async () => {
    const user = userEvent.setup();
    const { container, onSelectContext } = renderDetail();
    const views = screen.getByRole('group', { name: 'Views of the log' });

    await user.click(within(views).getByRole('button', { name: /Raw JSON/ }));
    // The stored record keeps the fields the list leaves to the head.
    expect(screen.getByText('_timestamp:')).not.toBeNull();
    expect(screen.getByText('message:')).not.toBeNull();
    expect(container.querySelector('[data-log-detail-fields]')).toBeNull();

    await user.click(within(views).getByRole('button', { name: /Context/ }));
    const current = container.querySelector('[aria-current="true"]') as HTMLElement;
    expect(current.textContent).toContain('09:59:54.120');
    await user.click(screen.getByText('request 52 completed'));
    expect(onSelectContext).toHaveBeenCalledWith(AROUND[3]);
  });

  it('steps to the logs on either side from the head, where its place among them is said once', async () => {
    const user = userEvent.setup();
    const { onPrev, onNext } = renderDetail();

    expect(screen.getByLabelText('5 of 40').textContent).toBe('5 / 40');
    await user.click(screen.getByRole('button', { name: 'Previous' }));
    await user.click(screen.getByRole('button', { name: 'Next' }));
    expect(onPrev).toHaveBeenCalledTimes(1);
    expect(onNext).toHaveBeenCalledTimes(1);
  });

  it('has no log before the first or after the last', () => {
    renderDetail({ index: 0, total: 1 });

    expect((screen.getByRole('button', { name: 'Previous' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Next' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('takes the keyboard when it opens, and closes on Escape', async () => {
    const user = userEvent.setup();
    const { container, onClose } = renderDetail();

    expect(document.activeElement).toBe(container.querySelector('[data-log-detail]'));
    await user.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: 'Close log details' }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it('shows a record that has no message without making one up', () => {
    const { container } = renderDetail({
      log: { ...OPEN, raw: { _timestamp: OPEN.micros, model: 'gpt', provider: 'openai', completion_tokens: 12 } },
    });

    expect(container.querySelector('[data-log-detail-message]')).toBeNull();
    expect(fieldNames(container)).toEqual(['model', 'provider', 'completion_tokens']);
  });
});
