import { describe, expect, it } from 'vitest';

import type { RumField, RumRow } from '@/api/rum';

import {
  DEFAULT_RUM_TAB,
  defaultTableColumns,
  ID_FIELD,
  isTroubled,
  RUM_TABS,
  rowId,
  rowTitle,
  rumTabOf,
  tableColumns,
} from './model';

const field = (name: string): RumField => ({
  name,
  dataType: 'utf8',
  unit: 'none',
  facetable: true,
  indexed: false,
});

const row = (values: Record<string, unknown>): RumRow => ({ timestamp: 1_000, ...values });

describe('the tabs of the explorer', () => {
  it('open on sessions and know only the types of the list', () => {
    expect(DEFAULT_RUM_TAB).toBe('sessions');
    expect(rumTabOf('views')).toBe('views');
    expect(rumTabOf('long_tasks')).toBe('long_tasks');
    expect(rumTabOf('vitals')).toBeNull();
    expect(rumTabOf('view')).toBeNull();
    expect(rumTabOf(null)).toBeNull();
  });

  it('name their rows by the column the server names them by', () => {
    expect(RUM_TABS.map((tab) => ID_FIELD[tab])).toEqual([
      'session.id',
      'view.id',
      'action.id',
      'error.id',
      'resource.id',
      'long_task.id',
      'log.id',
    ]);
  });
});

describe('the columns of a table', () => {
  const fields = [
    'usr.id',
    'session.duration',
    'session.id',
    'browser.name',
  ].map(field);

  it('open as the type lists them, keeping only those the schema has', () => {
    expect(tableColumns('sessions', fields, null).map((column) => column.name)).toEqual([
      'usr.id',
      'session.duration',
      'browser.name',
      'session.id',
    ]);
  });

  it('are the reader’s, in the order they chose them', () => {
    expect(
      tableColumns('sessions', fields, ['session.id', 'nope', 'usr.id']).map((column) => column.name),
    ).toEqual(['session.id', 'usr.id']);
    expect(tableColumns('sessions', fields, [])).toEqual([]);
  });

  it('come with a default list for every tab', () => {
    for (const tab of RUM_TABS) {
      expect(defaultTableColumns(tab).length).toBeGreaterThan(3);
      expect(defaultTableColumns(tab)).toContain(ID_FIELD[tab]);
    }
  });
});

describe('what a row is called', () => {
  it('is the most telling of its columns, else its id', () => {
    expect(rowTitle('sessions', row({ 'usr.email': 'ada@example.com', 'session.id': 's-1' }))).toBe(
      'ada@example.com',
    );
    expect(rowTitle('sessions', row({ 'session.id': 's-1' }))).toBe('s-1');
    expect(rowTitle('views', row({ 'view.name': '/checkout', 'view.url_path': '/co' }))).toBe('/checkout');
    expect(rowTitle('actions', row({ 'action.type': 'click', 'action.id': 'a-1' }))).toBe('click');
    expect(rowTitle('errors', row({ 'error.message': 'boom' }))).toBe('boom');
    expect(
      rowTitle('resources', row({ 'resource.method': 'GET', 'resource.url': 'https://a.example/x' })),
    ).toBe('GET https://a.example/x');
    expect(rowTitle('long_tasks', row({ 'long_task.id': 'l-1' }))).toBe('l-1');
  });

  it('is told by its id, which may be missing', () => {
    expect(rowId('errors', row({ 'error.id': 'e-1' }))).toBe('e-1');
    expect(rowId('errors', row({}))).toBe('');
  });
});

describe('the rows the histogram counts apart', () => {
  it('are the sessions and views with errors, the frustrated actions and the failed requests', () => {
    expect(isTroubled('sessions', row({ 'session.error.count': 2 }))).toBe(true);
    expect(isTroubled('sessions', row({ 'session.error.count': 0 }))).toBe(false);
    expect(isTroubled('views', row({ 'view.error.count': 1 }))).toBe(true);
    expect(isTroubled('actions', row({ 'action.frustration.count': 1 }))).toBe(true);
    expect(isTroubled('resources', row({ 'resource.status_code': 503 }))).toBe(true);
    expect(isTroubled('resources', row({ 'resource.status_code': 0 }))).toBe(true);
    expect(isTroubled('resources', row({ 'resource.status_code': 200 }))).toBe(false);
  });

  it('count a row that lacks the column as fine, and never count errors or long tasks apart', () => {
    expect(isTroubled('resources', row({}))).toBe(false);
    expect(isTroubled('views', row({}))).toBe(false);
    expect(isTroubled('errors', row({ 'error.id': 'e' }))).toBe(false);
    expect(isTroubled('long_tasks', row({}))).toBe(false);
  });
});
