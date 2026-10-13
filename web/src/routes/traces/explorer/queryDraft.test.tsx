import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { useTraceQueryDraft } from './queryDraft';

afterEach(() => {
  cleanup();
});

describe('useTraceQueryDraft', () => {
  it('starts in Code on the query the address carries', () => {
    const { result } = renderHook(() =>
      useTraceQueryDraft({ mode: 'fields', fields: "trace_id = 'abc'", sql: '' }),
    );

    expect(result.current.editorMode).toBe('code');
    expect(result.current.queryMode).toBe('fields');
    expect(result.current.queryDraft).toBe("trace_id = 'abc'");
  });

  it('reads what is typed in Code as a field query or as SQL', () => {
    const { result } = renderHook(() => useTraceQueryDraft({ mode: 'fields', fields: '', sql: '' }));

    act(() => result.current.changeCodeDraft("trace_id = 'abc'"));
    expect(result.current.queryMode).toBe('fields');
    expect(result.current.queryDraft).toBe("trace_id = 'abc'");

    act(() => result.current.changeCodeDraft('select * from traces'));
    expect(result.current.queryMode).toBe('sql');
    expect(result.current.sqlDraft).toBe('select * from traces');
    // The editor holds one statement: nothing of it is left over as a field query.
    expect(result.current.queryDraft).toBe('');

    act(() => result.current.changeCodeDraft(''));
    expect(result.current.queryMode).toBe('fields');
    expect(result.current.sqlDraft).toBe('');
  });

  it('writes the field query from the builder and makes it the statement in use', () => {
    const { result } = renderHook(() =>
      useTraceQueryDraft({ mode: 'sql', fields: '', sql: 'SELECT 1' }),
    );

    act(() => result.current.changeBuilderDraft("service.name = 'checkout'"));

    expect(result.current.queryMode).toBe('fields');
    expect(result.current.queryDraft).toBe("service.name = 'checkout'");
  });
});
