import '@/i18n';

import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { TooltipProvider } from '@/shell/ui/tooltip';

import type { LogFieldDef } from '../fieldQueryModel';
import { LogFieldList } from './List';

const field = (name: string, type: LogFieldDef['type'] = 'string', count = 3): LogFieldDef => ({
  name,
  type,
  dataType: type === 'number' ? 'int64' : type === 'object' ? 'json' : 'utf8',
  count: String(count),
  expandable: true,
});

const FIELDS = [
  field('duration_ms', 'number'),
  field('http.method'),
  field('http.route'),
  field('http.status_code', 'number'),
  field('k8s.pod.name'),
  field('error', 'object', 1),
];
const RECORDS = [
  { 'http.method': 'GET', 'http.route': '/cart', 'http.status_code': 200, duration_ms: 12, 'k8s.pod.name': 'a' },
  { 'http.method': 'GET', 'http.route': '/cart', 'http.status_code': 200, duration_ms: 15, 'k8s.pod.name': 'a' },
  { 'http.method': 'POST', 'http.route': '/pay', 'http.status_code': 504, duration_ms: 900, 'k8s.pod.name': 'b', error: { code: 504 } },
];

afterEach(() => cleanup());

function renderList(props: Partial<React.ComponentProps<typeof LogFieldList>> = {}) {
  const handlers = {
    onExpandedFieldChange: vi.fn(),
    onToggleVisibility: vi.fn(),
    onInsertFilter: vi.fn(),
    onInsertValueFilter: vi.fn(),
  };

  const view = render(
    <TooltipProvider>
      <LogFieldList
        fields={FIELDS}
        grouped
        visible={false}
        queryMode="fields"
        records={RECORDS}
        expandedField={null}
        {...handlers}
        {...props}
      />
    </TooltipProvider>,
  );

  return { ...view, ...handlers };
}

const row = (name: string) => document.querySelector<HTMLElement>(`[data-log-field-row="${name}"]`);
const group = (name: string) => document.querySelector<HTMLElement>(`[data-field-group="${name}"]`);

describe('LogFieldList', () => {
  it('lists the fields of one first part under it, closed until asked', () => {
    renderList();
    const http = group('http')!;
    const toggle = within(http).getByRole('button', { name: 'http: 3 fields' });

    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(row('http.method')).toBeNull();
    // What shares its first part with nothing keeps its whole name and its place.
    expect(group('k8s')).toBeNull();
    expect(row('k8s.pod.name')).not.toBeNull();
    expect(row('duration_ms')).not.toBeNull();

    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect([...http.querySelectorAll('[data-log-field-row]')].map((item) => item.getAttribute('data-log-field-row'))).toEqual([
      'http.method',
      'http.route',
      'http.status_code',
    ]);
    // By the rest of its name, the whole name a pointer away.
    expect(within(row('http.status_code')!).getByText('status_code').getAttribute('title')).toBe('http.status_code');
  });

  it('keeps the entries in the order the fields came in, a group where its first field stood', () => {
    const { container } = renderList();
    const entries = [...container.querySelectorAll<HTMLElement>(':scope > [data-log-field-row], :scope > [data-field-group]')];

    expect(entries.map((entry) => entry.dataset.logFieldRow ?? `group:${entry.dataset.fieldGroup}`)).toEqual([
      'duration_ms',
      'group:http',
      'k8s.pod.name',
      'error',
    ]);
  });

  it('asks a field under a group what any field is asked', () => {
    const { onExpandedFieldChange, onToggleVisibility, onInsertFilter } = renderList();

    fireEvent.click(within(group('http')!).getByRole('button', { name: 'http: 3 fields' }));
    const status = within(row('http.status_code')!);
    fireEvent.click(status.getAllByRole('button')[0]!);
    expect(onExpandedFieldChange).toHaveBeenLastCalledWith('http.status_code');
    fireEvent.click(status.getByRole('button', { name: 'Show http.status_code' }));
    expect(onToggleVisibility).toHaveBeenCalledWith('http.status_code');
    fireEvent.click(status.getByRole('button', { name: 'Add http.status_code to query' }));
    expect(onInsertFilter).toHaveBeenCalledWith('http.status_code');
  });

  it('stays open around a field that is open on its values, and closes it when the group closes', () => {
    const { onExpandedFieldChange } = renderList({ expandedField: 'http.method' });
    const toggle = within(group('http')!).getByRole('button', { name: 'http: 3 fields' });

    // Nobody opened the group: the open field does.
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(row('http.method')?.querySelector('[data-field-value="GET"]')).not.toBeNull();

    fireEvent.click(toggle);
    expect(onExpandedFieldChange).toHaveBeenCalledWith(null);
  });

  it('stands open on what a search has left', () => {
    renderList({ searching: true, fields: FIELDS.filter((item) => item.name.includes('http.')) });

    expect(within(group('http')!).getByRole('button', { name: 'http: 3 fields' }).getAttribute('aria-expanded')).toBe('true');
    expect(row('http.route')).not.toBeNull();
  });

  it('is a plain list where the order is the order of the columns', () => {
    renderList({ grouped: false, visible: true });

    expect(group('http')).toBeNull();
    expect(screen.getByText('http.method')).not.toBeNull();
    expect(within(row('http.method')!).getByRole('button', { name: 'Hide http.method' })).not.toBeNull();
  });

  it('says how many records carry a field only where not all of them do', () => {
    renderList();

    expect(row('error')?.querySelector('[data-log-field-count]')?.textContent).toBe('1');
    expect(row('duration_ms')?.querySelector('[data-log-field-count]')).toBeNull();
  });
});
