import { act, cleanup, renderHook } from '@testing-library/react';
import type * as React from 'react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it } from 'vitest';

import {
  defaultLogQuery,
  logQueryPlaceholder,
  logQuerySummary,
  requestedLogFieldQueryFromParams,
  useLogQueryState,
} from './queryState';

function at(entry: string) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <MemoryRouter initialEntries={[entry]}>{children}</MemoryRouter>;
  };
}

afterEach(() => {
  cleanup();
});

describe('requestedLogFieldQueryFromParams', () => {
  it('takes the field query a link carries as it is', () => {
    expect(requestedLogFieldQueryFromParams(new URLSearchParams("q= level = 'error' "))).toBe(
      "level = 'error'",
    );
  });

  it('builds one from the signal params of a cross-signal link', () => {
    expect(
      requestedLogFieldQueryFromParams(
        new URLSearchParams("trace_id=abc&service_name=checkout&status=500&host=it's"),
      ),
    ).toBe("trace_id='abc' AND service='checkout' AND host='it\\'s' AND status_code='500'");
  });
});

describe('useLogQueryState', () => {
  it('starts in Code on what the link asks for', () => {
    const { result } = renderHook(() => useLogQueryState(), {
      wrapper: at("/logs?stream=app&q=level%20%3D%20'error'"),
    });

    expect(result.current.editorMode).toBe('code');
    expect(result.current.queryMode).toBe('fields');
    expect(result.current.stream).toBe('app');
    expect(result.current.fieldQuery).toBe("level = 'error'");
  });

  it('opens a link that carries SQL as SQL', () => {
    const { result } = renderHook(() => useLogQueryState(), {
      wrapper: at('/logs?stream=app&sql=SELECT%201'),
    });

    expect(result.current.queryMode).toBe('sql');
    expect(result.current.query).toBe('SELECT 1');
  });

  it('reads what is typed in Code as a field query or as SQL', () => {
    const { result } = renderHook(() => useLogQueryState(), { wrapper: at('/logs') });

    act(() => result.current.setCodeStatement("trace_id = 'abc'"));
    expect(result.current.queryMode).toBe('fields');
    expect(result.current.fieldQuery).toBe("trace_id = 'abc'");

    act(() => result.current.setCodeStatement('SELECT * FROM "app"'));
    expect(result.current.queryMode).toBe('sql');
    expect(result.current.query).toBe('SELECT * FROM "app"');
    // The editor holds one statement: nothing of it is left over as a field query.
    expect(result.current.fieldQuery).toBe('');

    act(() => result.current.setCodeStatement('timeout'));
    expect(result.current.queryMode).toBe('fields');
    expect(result.current.query).toBe('');
  });

  it('does not leave the first letters of a SELECT behind as search text', () => {
    const { result } = renderHook(() => useLogQueryState(), { wrapper: at('/logs') });

    for (const typed of ['S', 'SE', 'SEL', 'SELE', 'SELEC', 'SELECT', 'SELECT 1']) {
      act(() => result.current.setCodeStatement(typed));
    }

    expect(result.current.queryMode).toBe('sql');
    expect(result.current.fieldQuery).toBe('');
  });

  it('writes the field query from the builder and makes it the statement in use', () => {
    const { result } = renderHook(() => useLogQueryState(), { wrapper: at('/logs') });
    act(() => result.current.setCodeStatement('SELECT 1'));

    act(() => result.current.setBuilderStatement("level = 'error'"));

    expect(result.current.queryMode).toBe('fields');
    expect(result.current.fieldQuery).toBe("level = 'error'");
  });
});

describe('what a Logs tab says about itself', () => {
  it('names the stream and the field query', () => {
    expect(
      logQuerySummary({ stream: 'app', queryMode: 'fields', query: '', fieldQuery: " level = 'error' " }),
    ).toBe("app · level = 'error'");
    expect(logQuerySummary({ stream: 'app', queryMode: 'fields', query: '', fieldQuery: '' })).toBe('app');
  });

  it('shows a SQL statement on one line, and only the stream for the statement it starts with', () => {
    expect(
      logQuerySummary({
        stream: 'app',
        queryMode: 'sql',
        query: 'SELECT level,\n  count(*)\nFROM "app"',
        fieldQuery: '',
      }),
    ).toBe('SELECT level, count(*) FROM "app"');
    expect(
      logQuerySummary({ stream: 'app', queryMode: 'sql', query: defaultLogQuery('app'), fieldQuery: '' }),
    ).toBe('app');
  });

  it('hints at both languages the editor takes', () => {
    expect(logQueryPlaceholder('')).toBe('trace_id = "..." / service_name contains "checkout"');
    expect(logQueryPlaceholder('app')).toContain('\nSELECT * FROM "app" WHERE');
  });
});
