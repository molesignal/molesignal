import { act, renderHook } from '@testing-library/react';
import * as React from 'react';
import { describe, expect, it, vi } from 'vitest';

import type { QueryEditorMode } from '@/shell/query/ModeSwitch';

import { traceSqlTemplate, type TraceFieldDef, type TraceQueryMode } from '../fieldQueryModel';
import { useTraceFieldFilters } from './fieldFilters';

const SERVICE: TraceFieldDef = { name: 'service.name', dataType: 'utf8', physical: true, scope: 'span' };
const TRACE_ID: TraceFieldDef = { name: 'trace_id', dataType: 'utf8', physical: true, scope: 'span' };

interface Start {
  editorMode?: QueryEditorMode;
  queryMode?: TraceQueryMode;
  queryDraft?: string;
  sqlDraft?: string;
  stream?: string;
}

/** The hook over drafts that behave as the page's do. */
function setup({ editorMode = 'code', queryMode: startMode = 'fields', queryDraft: startQuery = '', sqlDraft: startSql = '', stream = 'default' }: Start = {}) {
  const onRun = vi.fn();
  const onDraftStarted = vi.fn();
  const setBuilderField = vi.fn();

  const hook = renderHook(() => {
    const [queryMode, setQueryMode] = React.useState<TraceQueryMode>(startMode);
    const [queryDraft, setQueryDraft] = React.useState(startQuery);
    const [sqlDraft, setSqlDraft] = React.useState(startSql);
    const filters = useTraceFieldFilters({
      editorMode,
      queryMode,
      queryDraft,
      sqlDraft,
      stream,
      setBuilderField,
      setQueryMode,
      setQueryDraft,
      setSqlDraft,
      onDraftStarted,
      onRun,
    });

    return { ...filters, queryMode, queryDraft, sqlDraft };
  });

  return { hook, onRun, onDraftStarted, setBuilderField };
}

describe('what the field list does to the trace query', () => {
  it('writes a value into the field query and runs it at once', () => {
    const { hook, onRun, onDraftStarted } = setup({ queryDraft: 'error' });

    act(() => hook.result.current.filterValue(SERVICE, 'web', '='));

    expect(hook.result.current.queryDraft).toBe("error AND service.name = 'web'");
    expect(onRun).toHaveBeenCalledWith('fields', "error AND service.name = 'web'");
    // The filter is whole: there is nothing to type, so the editor is left as it is.
    expect(onDraftStarted).not.toHaveBeenCalled();
    expect(hook.result.current.isValueFiltered(SERVICE, 'web', '=')).toBe(true);
  });

  it('takes the value out again with the same click', () => {
    const { hook, onRun } = setup({ queryDraft: "error AND service.name = 'web'" });

    act(() => hook.result.current.filterValue(SERVICE, 'web', '='));

    expect(hook.result.current.queryDraft).toBe('error');
    expect(onRun).toHaveBeenLastCalledWith('fields', 'error');
    expect(hook.result.current.isValueFiltered(SERVICE, 'web', '=')).toBe(false);
  });

  it('writes SQL while the Code editor holds SQL', () => {
    const template = traceSqlTemplate('default');
    const { hook, onRun } = setup({ queryMode: 'sql', sqlDraft: template });

    expect(hook.result.current.filterMode).toBe('sql');
    act(() => hook.result.current.filterValue(TRACE_ID, 'abc', '!='));

    expect(hook.result.current.sqlDraft).toContain(`WHERE "trace_id" <> 'abc'`);
    expect(onRun).toHaveBeenCalledWith('sql', hook.result.current.sqlDraft);
    expect(hook.result.current.queryDraft).toBe('');
  });

  it('writes the field query from the builder, whatever Code last held', () => {
    const { hook, onRun } = setup({ editorMode: 'builder', queryMode: 'sql', sqlDraft: 'SELECT 1' });

    expect(hook.result.current.filterMode).toBe('fields');
    act(() => hook.result.current.filterValue(SERVICE, 'web', '='));

    expect(hook.result.current.queryMode).toBe('fields');
    expect(hook.result.current.queryDraft).toBe("service.name = 'web'");
    expect(hook.result.current.sqlDraft).toBe('SELECT 1');
    expect(onRun).toHaveBeenCalledWith('fields', "service.name = 'web'");
  });

  it('has no SQL to write without a stream to read', () => {
    const { hook, onRun } = setup({ queryMode: 'sql', sqlDraft: '', stream: '' });

    act(() => hook.result.current.filterValue(SERVICE, 'web', '='));

    expect(hook.result.current.sqlDraft).toBe('');
    expect(onRun).not.toHaveBeenCalled();
  });

  it('starts a comparison for a field and leaves the value to be typed', () => {
    const { hook, onRun, onDraftStarted } = setup();

    act(() => hook.result.current.insertField(SERVICE));

    expect(hook.result.current.queryDraft).toBe("service.name = ''");
    expect(onDraftStarted).toHaveBeenCalledTimes(1);
    expect(onRun).not.toHaveBeenCalled();
  });

  it('starts a filter in the builder when that is what is open', () => {
    const { hook, setBuilderField, onDraftStarted } = setup({ editorMode: 'builder' });

    act(() => hook.result.current.insertField(SERVICE));

    expect(setBuilderField).toHaveBeenCalledWith('service.name');
    expect(hook.result.current.queryDraft).toBe('');
    expect(onDraftStarted).toHaveBeenCalledTimes(1);
  });
});
