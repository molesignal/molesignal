import { act, cleanup, renderHook } from '@testing-library/react';
import type * as React from 'react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { StreamField } from '@/api/streams';
import { type QueryTabScope, QueryTabScopeContext } from '@/shell/queryTabs/scope';

import { logLinkKey } from './conditions/model';
import {
  defaultLogQuery,
  logQueryPlaceholder,
  logQuerySummary,
  requestedLogFieldQueryFromParams,
  requestedLogLinkFromParams,
  useLogQueryState,
  writeLogQueryParams,
} from './queryState';

function at(entry: string) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <MemoryRouter initialEntries={[entry]}>{children}</MemoryRouter>;
  };
}

/** A query tab that comes back with what it kept: a reload of the page. */
function tabAt(search: string, memory: Record<string, unknown>) {
  const scope: QueryTabScope = {
    id: 'tab-1',
    active: true,
    params: new URLSearchParams(search),
    setParams: vi.fn(),
    memory,
    remember: vi.fn(),
    report: vi.fn(),
  };

  return function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <MemoryRouter initialEntries={['/logs']}>
        <QueryTabScopeContext.Provider value={scope}>{children}</QueryTabScopeContext.Provider>
      </MemoryRouter>
    );
  };
}

const field = (name: string, data_type: StreamField['dataType'] = 'utf8'): StreamField => ({
  name,
  dataType: data_type,
  nullable: true,
  indexed: false,
});

/** A stream as OTLP writes logs: the service and the host by their semantic-convention names. */
const OTLP_FIELDS = [field('_timestamp', 'timestamp'), field('service.name'), field('trace_id'), field('body')];
const unknownFields = () => undefined;

afterEach(() => {
  cleanup();
});

describe('requestedLogFieldQueryFromParams', () => {
  it('takes the field query a link carries as it is', () => {
    expect(requestedLogFieldQueryFromParams(new URLSearchParams("q= level = 'error' "))).toBe(
      "level = 'error'",
    );
  });

  it('builds none from what a link asks by meaning: field names depend on the stream', () => {
    expect(requestedLogFieldQueryFromParams(new URLSearchParams('service=checkout&host=node-3'))).toBe('');
  });
});

describe('requestedLogLinkFromParams', () => {
  it('reads what a cross-signal link asks of the logs, under each name it may say it', () => {
    expect(
      requestedLogLinkFromParams(
        new URLSearchParams("traceId=abc&serviceName=checkout&statusCode=500&host=it's&path=/cart&environment=prod"),
      ),
    ).toEqual({
      trace_id: 'abc',
      service: 'checkout',
      status: '500',
      host: "it's",
      route: '/cart',
      environment: 'prod',
    });
  });

  it('asks nothing by meaning of a link that carries its own field query, or none', () => {
    expect(requestedLogLinkFromParams(new URLSearchParams("q=level%20%3D%20'error'&service=checkout"))).toBeNull();
    expect(requestedLogLinkFromParams(new URLSearchParams('stream=app&from=now-1h'))).toBeNull();
  });
});

describe('useLogQueryState', () => {
  it('starts in Code on what the link asks for', () => {
    const { result } = renderHook(() => useLogQueryState(unknownFields), {
      wrapper: at("/logs?stream=app&q=level%20%3D%20'error'"),
    });

    expect(result.current.editorMode).toBe('code');
    expect(result.current.queryMode).toBe('fields');
    expect(result.current.stream).toBe('app');
    expect(result.current.fieldQuery).toBe("level = 'error'");
  });

  it('opens a link that carries SQL as SQL', () => {
    const { result } = renderHook(() => useLogQueryState(unknownFields), {
      wrapper: at('/logs?stream=app&sql=SELECT%201'),
    });

    expect(result.current.queryMode).toBe('sql');
    expect(result.current.query).toBe('SELECT 1');
  });

  it('reads what is typed in Code as a field query or as SQL', () => {
    const { result } = renderHook(() => useLogQueryState(unknownFields), { wrapper: at('/logs') });

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
    const { result } = renderHook(() => useLogQueryState(unknownFields), { wrapper: at('/logs') });

    for (const typed of ['S', 'SE', 'SEL', 'SELE', 'SELEC', 'SELECT', 'SELECT 1']) {
      act(() => result.current.setCodeStatement(typed));
    }

    expect(result.current.queryMode).toBe('sql');
    expect(result.current.fieldQuery).toBe('');
  });

  it('writes what a link asks by meaning in the stream\'s own field names, once it knows them', () => {
    const { result, rerender } = renderHook(({ fieldsOf }) => useLogQueryState(fieldsOf), {
      wrapper: at('/logs?stream=app&service=checkout&host=node-3'),
      initialProps: { fieldsOf: unknownFields as (stream: string) => StreamField[] | undefined },
    });

    // Until the stream's fields are known there is no field to write the service on.
    expect(result.current.linkPending).toBe(true);
    expect(result.current.fieldQuery).toBe('');

    rerender({ fieldsOf: () => OTLP_FIELDS });

    expect(result.current.linkPending).toBe(false);
    expect(result.current.queryMode).toBe('fields');
    expect(result.current.fieldQuery).toBe("service.name = 'checkout'");
    // The stream keeps no host: the condition is left out, and said to be.
    expect(result.current.linkGaps).toEqual([{ meaning: 'host', value: 'node-3' }]);
  });

  it('does not write a link again over what its tab has made of the query since', () => {
    const link = requestedLogLinkFromParams(new URLSearchParams('service=checkout'))!;
    const { result } = renderHook(() => useLogQueryState(() => OTLP_FIELDS), {
      wrapper: tabAt('stream=app&service=checkout', {
        'logs.stream': 'app',
        'logs.fieldQuery': "service.name = 'checkout' AND level = 'ERROR'",
        'logs.writtenLink': logLinkKey(link),
      }),
    });

    expect(result.current.linkPending).toBe(false);
    expect(result.current.fieldQuery).toBe("service.name = 'checkout' AND level = 'ERROR'");
  });

  it('writes the field query from the builder and makes it the statement in use', () => {
    const { result } = renderHook(() => useLogQueryState(unknownFields), { wrapper: at('/logs') });
    act(() => result.current.setCodeStatement('SELECT 1'));

    act(() => result.current.setBuilderStatement("level = 'error'"));

    expect(result.current.queryMode).toBe('fields');
    expect(result.current.fieldQuery).toBe("level = 'error'");
  });
});

describe('writeLogQueryParams', () => {
  it('says the query that ran in the address, in place of what a link asked for by meaning', () => {
    const next = writeLogQueryParams(
      new URLSearchParams('stream=old&service=checkout&traceId=abc&view=stats&time=now-1h..now'),
      { stream: 'app', mode: 'fields', statement: " service.name = 'checkout' " },
    );

    expect(Object.fromEntries(next)).toEqual({
      stream: 'app',
      view: 'stats',
      time: 'now-1h..now',
      q: "service.name = 'checkout'",
    });
  });

  it('carries SQL in its own param, and no statement as none', () => {
    const sql = writeLogQueryParams(new URLSearchParams('q=level%3Derror'), {
      stream: 'app',
      mode: 'sql',
      statement: 'SELECT 1',
    });
    const none = writeLogQueryParams(new URLSearchParams('sql=SELECT+1'), { stream: 'app', mode: 'fields', statement: ' ' });

    expect(sql.toString()).toBe('stream=app&sql=SELECT+1');
    expect(none.toString()).toBe('stream=app');
  });

  it("writes the query that runs to its tab's address, and takes that address as its own", () => {
    const setParams = vi.fn();
    const scope: QueryTabScope = {
      id: 'tab-1',
      active: true,
      params: new URLSearchParams('stream=app&service=checkout'),
      setParams,
      memory: {},
      remember: vi.fn(),
      report: vi.fn(),
    };
    const { result } = renderHook(() => useLogQueryState(unknownFields), {
      wrapper: ({ children }: { children: React.ReactNode }) => (
        <MemoryRouter initialEntries={['/logs']}>
          <QueryTabScopeContext.Provider value={scope}>{children}</QueryTabScopeContext.Provider>
        </MemoryRouter>
      ),
    });

    act(() => result.current.publishQuery({ stream: 'app', mode: 'fields', statement: "service.name = 'checkout'" }));

    const [update, options] = setParams.mock.calls[0]! as [(current: URLSearchParams) => URLSearchParams, unknown];
    expect(options).toEqual({ replace: true });
    expect(Object.fromEntries(update(scope.params))).toEqual({ stream: 'app', q: "service.name = 'checkout'" });
    expect(result.current.addressTaken.current).toEqual({ stream: 'app', fieldQuery: "service.name = 'checkout'", sql: '' });
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
    expect(logQueryPlaceholder('')).toBe('trace_id = "..." / service.name contains "checkout"');
    expect(logQueryPlaceholder('app')).toContain('\nSELECT * FROM "app" WHERE');
  });
});
