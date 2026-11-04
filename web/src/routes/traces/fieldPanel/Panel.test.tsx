import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as meApi from '@/api/me';
import * as queryApi from '@/api/query';
import i18n from '@/i18n';
import type { QueryResult } from '@/types/query';

import type { TraceFieldDef, TraceQueryMode } from '../fieldQueryModel';
import type { TraceFieldRecord } from './model';
import { TraceFieldPanel } from './Panel';

vi.mock('@/api/query', () => ({ readQuery: vi.fn() }));
vi.mock('@/api/me', async (original) => {
  const actual = await original<typeof meApi>();
  return { ...actual, preferences: async () => actual.DEFAULT_USER_PREFERENCES };
});

const readQuery = vi.mocked(queryApi.readQuery);

const FIELDS: TraceFieldDef[] = [
  { name: 'trace_id', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'span_id', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'duration_ns', dataType: 'int64', physical: true, scope: 'trace_aggregate' },
  { name: 'service.name', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'attributes', dataType: 'json', physical: true, scope: 'span' },
  { name: 'span_count', dataType: 'int64', physical: false, scope: 'trace_aggregate' },
];
const field = (name: string) => FIELDS.find((candidate) => candidate.name === name)!;

const trace = (id: string, durationMs: number, spans = 3): TraceFieldRecord => ({
  id,
  op: 'GET /',
  service: 'web',
  startNs: 2_000_000_000_000,
  durationMs,
  spans,
  errors: 0,
});
const TRACES = [trace('t-1', 10), trace('t-2', 20), trace('t-3', 30)];

const result = (columns: string[], rows: unknown[][]): QueryResult => ({ columns, rows, scannedRows: 0, tookMs: 0 });
const SERVICES = {
  top: result(['value', 'traces', 'spans'], [['web', 3, 7], ['router', 1, 2]]),
  summary: result(['distinct_values', 'traces', 'samples'], [[2, 3, 9]]),
};

/** Answers the two statements a field of a span is read with. */
function answer({ top, summary }: { top: QueryResult; summary: QueryResult }) {
  readQuery.mockImplementation(async (request) => (request.statement.includes('distinct_values') ? summary : top));
}

interface Options {
  traces?: TraceFieldRecord[];
  fields?: TraceFieldDef[];
  fieldFilter?: string;
  stream?: string;
  filterMode?: TraceQueryMode;
  onToggleField?: (field: string) => void;
  visibleFields?: string[];
  isValueFiltered?: (field: TraceFieldDef, value: string, op: string) => boolean;
}

function renderPanel({
  traces = TRACES,
  fields = FIELDS,
  fieldFilter = '',
  stream = 'default',
  filterMode = 'fields',
  onToggleField,
  visibleFields = ['http.route'],
  isValueFiltered = () => false,
}: Options = {}) {
  const onFilterValue = vi.fn();
  const onInsertField = vi.fn();

  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TraceFieldPanel
        traces={traces}
        fields={fields}
        source={{ orgId: 'org-1', stream, range: { start: 1, end: 9 } }}
        filterMode={filterMode}
        fieldFilter={fieldFilter}
        collapsed={false}
        onFieldFilterChange={() => undefined}
        onCollapsedChange={() => undefined}
        onInsertField={onInsertField}
        isValueFiltered={isValueFiltered}
        onFilterValue={onFilterValue}
        {...(onToggleField ? { visibleFields, onToggleField } : {})}
      />
    </QueryClientProvider>,
  );

  return { onFilterValue, onInsertField };
}

const row = (name: string) => document.querySelector<HTMLElement>(`[data-trace-field="${name}"]`)!;
const open = (name: string) => fireEvent.click(row(name).querySelector('button')!);
const values = (name: string) => row(name).querySelector<HTMLElement>('[data-trace-field-values]')!;
const listed = (name: string) =>
  [...values(name).querySelectorAll<HTMLElement>('[data-field-value]')].map((item) => item.dataset.fieldValue);
const actions = (name: string, value: string) =>
  within(values(name).querySelector<HTMLElement>(`[data-field-value="${value}"]`)!)
    .getAllByRole('button')
    .map((button) => button.getAttribute('aria-label'));

beforeEach(async () => {
  await i18n.changeLanguage('en-US');
  readQuery.mockReset();
});
afterEach(cleanup);

describe('the trace field list', () => {
  it('reads what a trace has one of from the loaded rows, without a query', () => {
    renderPanel();
    open('trace_id');

    expect(listed('trace_id')).toEqual(['t-1', 't-2', 't-3']);
    expect(values('trace_id')).toHaveTextContent('3 values · in 3 of 3 traces');
    // Each id is in one trace: a "1" and a sliver of a bar on every row would say nothing.
    expect(values('trace_id').querySelector('[data-field-value="t-1"]')).toHaveTextContent(/^t-1$/);
    expect(values('trace_id').querySelector('[data-field-value-share]')).toBeNull();
    expect(readQuery).not.toHaveBeenCalled();
  });

  it('counts a trace once, however many rows a statement lists it on', async () => {
    answer(SERVICES);
    renderPanel({ traces: [...TRACES, trace('t-1', 10), trace('t-2', 20)] });
    open('trace_id');
    expect(values('trace_id')).toHaveTextContent('3 values · in 3 of 3 traces');

    open('service.name');
    await waitFor(() => expect(values('service.name')).toHaveTextContent('2 values · in 3 of 3 traces'));
    expect(readQuery.mock.calls[0]![0].statement).toContain("trace_id IN ('t-1', 't-2', 't-3') AND");
  });

  it('does not open a field of a span when there is no stream to read it from', () => {
    renderPanel({ stream: '' });

    expect(row('service.name').querySelector('button')).toBeDisabled();
    // What a trace has one of is on the rows, stream or not.
    expect(row('span_count').querySelector('button')).toBeEnabled();
  });

  it('reads a field of a span from the spans of the loaded traces', async () => {
    answer(SERVICES);
    renderPanel();
    open('service.name');

    expect(within(values('service.name')).getByRole('status', { name: 'Loading the values of service.name' })).toBeVisible();
    await waitFor(() => expect(listed('service.name')).toEqual(['web', 'router']));
    expect(values('service.name')).toHaveTextContent('2 values · in 3 of 3 traces');
    // A value says how many of the loaded traces have it, in a number and in the bar under it.
    const web = values('service.name').querySelector<HTMLElement>('[data-field-value="web"]')!;
    expect(web).toHaveTextContent(/^web3$/);
    expect(web.querySelector<HTMLElement>('[data-field-value-share]')!.style.width).toBe('100%');

    const request = readQuery.mock.calls[0]![0];
    expect(request.statement).toContain(`WHERE trace_id IN ('t-1', 't-2', 't-3') AND "service.name" IS NOT NULL`);
    expect(request.stream).toEqual({ name: 'default', streamType: 'traces' });
    // The traces' own stretch of time, not the whole range that was asked for.
    expect(request.timeRange).toEqual({ start: 2_000_000_000 - 1_000_000, end: 2_000_030_000 + 1_000_000 });
  });

  it('says how many values there are when it lists only the most common', async () => {
    answer({ top: SERVICES.top, summary: result(['distinct_values', 'traces', 'samples'], [[36, 3, 36]]) });
    renderPanel();
    open('span_id');

    await waitFor(() => expect(values('span_id')).toHaveTextContent('Top 2 of 36 values · in 3 of 3 traces'));
  });

  it('offers a value of a span both ways, in a field query as in SQL', async () => {
    answer(SERVICES);
    renderPanel();
    open('service.name');

    // Left out, the value keeps the traces none of whose spans has it.
    await waitFor(() =>
      expect(actions('service.name', 'web')).toEqual(['Filter for service.name = web', 'Filter out service.name = web']),
    );
    cleanup();

    renderPanel({ filterMode: 'sql' });
    open('service.name');
    await waitFor(() =>
      expect(actions('service.name', 'web')).toEqual(['Filter for service.name = web', 'Filter out service.name = web']),
    );
  });

  it('lets a field a trace has one value for be left out in a field query too', () => {
    renderPanel();
    open('trace_id');

    expect(actions('trace_id', 't-1')).toEqual(['Filter for trace_id = t-1', 'Filter out trace_id = t-1']);
  });

  it('hands a clicked value to the query', async () => {
    answer(SERVICES);
    const { onFilterValue } = renderPanel();
    open('service.name');

    fireEvent.click(await screen.findByRole('button', { name: 'Filter for service.name = router' }));
    expect(onFilterValue).toHaveBeenCalledWith(field('service.name'), 'router', '=');

    fireEvent.click(screen.getByRole('button', { name: 'Filter out service.name = router' }));
    expect(onFilterValue).toHaveBeenLastCalledWith(field('service.name'), 'router', '!=');
  });

  it('shows a filter that is in the query as on', async () => {
    answer(SERVICES);
    renderPanel({ isValueFiltered: (_field, value) => value === 'web' });
    open('service.name');

    expect(await screen.findByRole('button', { name: 'Filter for service.name = web' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'Filter for service.name = router' })).toHaveAttribute('aria-pressed', 'false');
  });

  it('reads a measure as what it runs from and to, with bounds to filter by', () => {
    const many = Array.from({ length: 12 }, (_, index) => trace(`t-${index}`, (index + 1) * 10));
    const { onFilterValue } = renderPanel({ traces: many });
    open('duration_ns');

    expect(values('duration_ns')).toHaveTextContent('12 values · in 12 of 12 traces');
    expect([...values('duration_ns').querySelectorAll('[data-field-value]')].map((item) => item.textContent)).toEqual([
      'Min10 ms≥≤',
      'Avg65 ms≥≤',
      'Max120 ms≥≤',
    ]);

    fireEvent.click(screen.getByRole('button', { name: 'Filter for duration_ns of at least 65 ms' }));
    expect(onFilterValue).toHaveBeenCalledWith(field('duration_ns'), '65000000', '>=');
  });

  it('says when the values could not be read, and reads them again', async () => {
    readQuery.mockRejectedValue(new Error('boom'));
    renderPanel();
    open('service.name');

    expect(await within(values('service.name')).findByRole('alert')).toHaveTextContent('The values could not be loaded.');
    answer(SERVICES);
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(listed('service.name')).toEqual(['web', 'router']));
  });

  it('says when the loaded traces have no value, and asks nothing when none are loaded', async () => {
    answer({ top: result(['value', 'traces', 'spans'], []), summary: result(['distinct_values', 'traces', 'samples'], [[0, 0, 0]]) });
    renderPanel();
    open('service.name');
    await waitFor(() => expect(values('service.name')).toHaveTextContent('The loaded traces have no value for this field.'));
    cleanup();
    readQuery.mockClear();

    renderPanel({ traces: [] });
    open('service.name');
    expect(values('service.name')).toHaveTextContent('No traces are loaded.');
    expect(readQuery).not.toHaveBeenCalled();
  });

  it('does not open a document field, and says why it cannot be put in a field query', () => {
    const { onInsertField } = renderPanel();
    const toggle = row('attributes').querySelector('button')!;

    expect(toggle).toBeDisabled();
    expect(within(row('attributes')).getByRole('button', { name: 'attributes requires a JSON path query in SQL mode' })).toBeDisabled();

    fireEvent.click(within(row('span_id')).getByRole('button', { name: 'Add span_id to query' }));
    expect(onInsertField).toHaveBeenCalledWith(field('span_id'));
  });

  it('offers the eye where the table can show a field of the root span beside the traces', () => {
    const ROOT = [...FIELDS, { name: 'http.route', dataType: 'utf8', physical: true, scope: 'span' } as TraceFieldDef];
    const eye = /^(Show .* as a column|Remove the .* column)$/;
    renderPanel({ fields: ROOT });
    expect(screen.queryByRole('button', { name: eye })).toBeNull();
    cleanup();

    const onToggleField = vi.fn();
    renderPanel({ fields: [...ROOT, { name: 'deployment.environment', dataType: 'utf8', physical: true, scope: 'span' }], onToggleField });
    expect(within(row('http.route')).getByRole('button', { name: 'Remove the http.route column' })).toHaveAttribute('aria-pressed', 'true');
    // The trace's own columns, and a document, are not fields of the root to add.
    for (const name of ['trace_id', 'span_id', 'duration_ns', 'service.name', 'span_count', 'attributes']) {
      expect(within(row(name)).queryByRole('button', { name: eye })).toBeNull();
    }

    fireEvent.click(within(row('deployment.environment')).getByRole('button', { name: "Show deployment.environment of each trace's root span as a column" }));
    expect(onToggleField).toHaveBeenCalledWith('deployment.environment');
  });

  it('puts what a trace is first narrowed by first, the HTTP status under the name the stream writes it with', () => {
    const named = (name: string, dataType: TraceFieldDef['dataType'] = 'utf8'): TraceFieldDef => ({ name, dataType, physical: true, scope: 'span' });
    renderPanel({ fields: [...FIELDS, named('name'), named('status_code'), named('http.status_code', 'int64')] });

    const common = document.querySelector<HTMLElement>('[data-side-panel-section]')!;
    expect([...common.querySelectorAll<HTMLElement>('[data-trace-field]')].map((item) => item.dataset.traceField))
      .toEqual(['service.name', 'name', 'status_code', 'http.status_code', 'duration_ns']);
    cleanup();

    // The name the conventions use now wins over the older one.
    renderPanel({ fields: [...FIELDS, named('http.status_code', 'int64'), named('http.response.status_code', 'int64')] });
    const pinned = document.querySelector<HTMLElement>('[data-side-panel-section]')!;
    expect(pinned.querySelector('[data-trace-field="http.response.status_code"]')).not.toBeNull();
    expect(pinned.querySelector('[data-trace-field="http.status_code"]')).toBeNull();
  });

  describe('attributes that share the first part of their names', () => {
    const span = (name: string, dataType: TraceFieldDef['dataType'] = 'utf8'): TraceFieldDef => ({
      name,
      dataType,
      physical: true,
      scope: 'span',
    });
    const ATTRIBUTES = [
      ...FIELDS,
      span('http.request.method'),
      span('http.route'),
      span('http.request.body.size', 'int64'),
      span('url.path'),
    ];
    const group = (name: string) => document.querySelector<HTMLElement>(`[data-field-group="${name}"]`);

    it('are one entry that says how many they are, and opens on them', () => {
      renderPanel({ fields: ATTRIBUTES });
      const http = group('http')!;
      const toggle = within(http).getByRole('button', { name: 'http: 3 fields' });

      // Closed until asked: three names less to read down.
      expect(toggle.getAttribute('aria-expanded')).toBe('false');
      expect(row('http.route')).toBeNull();
      // A first part only one field has says nothing: that field keeps its whole name.
      expect(group('url')).toBeNull();
      expect(row('url.path').textContent).toBe('Turl.path');
      // The fields a trace is told by are not looked through: they keep their own rows.
      expect(group('service')).toBeNull();

      fireEvent.click(toggle);
      expect(toggle.getAttribute('aria-expanded')).toBe('true');
      // By the rest of their names, the whole name a pointer away.
      const rows = [...http.querySelectorAll<HTMLElement>('[data-trace-field-row]')];
      expect(rows.map((item) => item.textContent)).toEqual(['#request.body.size', 'Trequest.method', 'Troute']);
      expect(within(row('http.route')).getByText('route').getAttribute('title')).toBe('http.route');
    });

    it('stay open around a field that is open on its values, and close it when they close', async () => {
      answer(SERVICES);
      renderPanel({ fields: ATTRIBUTES });
      const toggle = within(group('http')!).getByRole('button', { name: 'http: 3 fields' });

      fireEvent.click(toggle);
      open('http.route');
      await waitFor(() => expect(listed('http.route')).toEqual(['web', 'router']));

      fireEvent.click(toggle);
      expect(row('http.route')).toBeNull();
      fireEvent.click(toggle);
      expect(values('http.route')).toBeNull();
    });

    it('are all open on what a search has left, and a single match is a field of its own', () => {
      renderPanel({ fields: ATTRIBUTES, fieldFilter: 're' });

      // Both fields of the request match, the route does not: still a group, open without asking.
      expect(within(group('http')!).getByRole('button', { name: 'http: 2 fields' }).getAttribute('aria-expanded')).toBe('true');
      expect(row('http.request.method')).not.toBeNull();
      expect(row('http.route')).toBeNull();

      cleanup();
      renderPanel({ fields: ATTRIBUTES, fieldFilter: 'size' });
      expect(group('http')).toBeNull();
      expect(row('http.request.body.size').textContent).toBe('#http.request.body.size');
    });
  });

  describe('the parts of the list', () => {
    const sections = () =>
      [...document.querySelectorAll<HTMLElement>('[data-side-panel-section]')].map((section) => ({
        heading: section.querySelector('button')?.textContent,
        fields: [...section.querySelectorAll<HTMLElement>('[data-trace-field]')].map((item) => item.dataset.traceField),
      }));
    const INDEXED = FIELDS.map((item) =>
      item.name === 'trace_id' || item.name === 'service.name' ? { ...item, indexed: true } : item,
    );

    it('are the common fields, the fields kept in an index, and the rest, where no field can be put on display', () => {
      renderPanel({ fields: INDEXED });

      expect(sections()).toEqual([
        { heading: 'Common fields2', fields: ['service.name', 'duration_ns'] },
        { heading: 'Indexed fields1', fields: ['trace_id'] },
        { heading: 'Other fields3', fields: ['span_id', 'span_count', 'attributes'] },
      ]);
    });

    it('put the fields on display after the common ones, and in no other part', () => {
      // The harness puts `http.route` on display where the table can show a field beside a trace.
      const route: TraceFieldDef = { name: 'http.route', dataType: 'utf8', physical: true, scope: 'span' };
      renderPanel({ fields: [...INDEXED, route], onToggleField: () => undefined });

      expect(sections()).toEqual([
        { heading: 'Common fields2', fields: ['service.name', 'duration_ns'] },
        { heading: 'Displayed fields1', fields: ['http.route'] },
        { heading: 'Indexed fields1', fields: ['trace_id'] },
        { heading: 'Other fields3', fields: ['span_id', 'span_count', 'attributes'] },
      ]);
    });

    it('say so where the stream keeps no field in an index', () => {
      renderPanel();

      expect(sections().map((section) => section.heading)).toEqual(['Common fields2', 'Indexed fields0', 'Other fields4']);
      expect(screen.getByText('This stream keeps no field in an index.')).not.toBeNull();
    });

    it('have no heading for a part a search has left empty', () => {
      renderPanel({ fieldFilter: 'span' });

      // `span_id` and `span_count` match; neither is indexed.
      expect(sections().map((section) => section.heading)).toEqual(['Other fields2']);
      expect(screen.queryByText('This stream keeps no field in an index.')).toBeNull();
    });
  });
});

