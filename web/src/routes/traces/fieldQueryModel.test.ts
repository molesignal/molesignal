import { describe, expect, it } from 'vitest';

import {
  appendTraceSqlFieldFilter,
  deriveTraceFields,
  groupTraceFields,
  insertTraceClause,
  isTraceFieldQueryable,
  isTraceLevelField,
  parseTraceStatement,
  selectTraceStream,
  splitTraceStatement,
  traceSqlTemplate,
  type TraceFieldDef,
} from './fieldQueryModel';

describe('trace field query model', () => {
  const fields = deriveTraceFields([
    { name: 'trace_id', dataType: 'utf8', nullable: false, indexed: true },
    { name: 'service.name', dataType: 'utf8', nullable: true, indexed: true },
    { name: 'http.status_code', dataType: 'int64', nullable: true, indexed: false },
    { name: 'conflict', dataType: 'bool', nullable: true, indexed: false },
    { name: 'attributes', dataType: 'json', nullable: true, indexed: false },
    { name: 'molesignal.trace.span_count', dataType: 'int64', nullable: true, indexed: false },
  ]);

  it('derives scalar physical fields and public aggregate aliases', () => {
    expect(fields.map((field) => field.name)).toEqual([
      'trace_id',
      'service.name',
      'http.status_code',
      'conflict',
      'attributes',
      'span_count',
      'error_count',
    ]);
    expect(fields.find((field) => field.name === 'span_count')?.physical).toBe(false);
  });

  it('separates OpenTelemetry core fields from resource, scope and Span attributes', () => {
    const grouped = groupTraceFields(deriveTraceFields([
      { name: 'trace_id', dataType: 'utf8', nullable: false, indexed: true },
      { name: 'span_id', dataType: 'utf8', nullable: false, indexed: true },
      { name: 'name', dataType: 'utf8', nullable: false, indexed: true },
      { name: 'duration_ns', dataType: 'int64', nullable: false, indexed: false },
      { name: 'status_code', dataType: 'utf8', nullable: true, indexed: true },
      { name: 'service.name', dataType: 'utf8', nullable: true, indexed: true },
      { name: 'host.name', dataType: 'utf8', nullable: true, indexed: false },
      { name: 'telemetry.sdk.name', dataType: 'utf8', nullable: true, indexed: false },
      { name: 'scope_name', dataType: 'utf8', nullable: true, indexed: false },
      { name: 'http.request.method', dataType: 'utf8', nullable: true, indexed: false },
      { name: 'attributes', dataType: 'json', nullable: true, indexed: false },
      { name: 'conflict', dataType: 'bool', nullable: true, indexed: false },
    ]));

    expect(grouped.core.map((field) => field.name)).toEqual([
      'trace_id',
      'span_id',
      'name',
      'duration_ns',
      'status_code',
      'span_count',
      'error_count',
    ]);
    expect(grouped.groups.map((group) => ({
      group: group.group,
      fields: group.fields.map((field) => field.name),
    }))).toEqual([
      {
        group: 'resource',
        fields: ['host.name', 'service.name', 'telemetry.sdk.name'],
      },
      {
        group: 'scope',
        fields: ['scope_name'],
      },
      {
        group: 'span',
        fields: ['attributes', 'conflict', 'http.request.method'],
      },
    ]);
  });

  it('parses typed numeric, boolean and any-Span field filters', () => {
    const parsed = parseTraceStatement(
      'span_count >= 3 AND http.status_code > 499 AND conflict = true',
      fields,
    );
    expect(parsed.rejected).toEqual([]);
    expect(parsed.filters).toEqual([
      { field: 'span_count', op: '>=', value: '3' },
      { field: 'http.status_code', op: '>', value: '499' },
      { field: 'conflict', op: '=', value: 'true' },
    ]);
  });

  it('reads a value of a span left out, however the operator is spelt', () => {
    const parsed = parseTraceStatement(
      'service.name ne "checkout" AND http.status_code != 500 AND conflict NE true',
      fields,
    );

    expect(parsed.rejected).toEqual([]);
    expect(parsed.filters).toEqual([
      { field: 'service.name', op: '!=', value: 'checkout' },
      { field: 'http.status_code', op: '!=', value: '500' },
      { field: 'conflict', op: '!=', value: 'true' },
    ]);
  });

  it('reads a list of values a field is asked for, quotes and commas in them kept', () => {
    const parsed = parseTraceStatement(
      "service.name in ('web', \"checkout\") AND http.status_code in (500, 503) AND service.name IN ('a, b', 'it\\'s')",
      fields,
    );

    expect(parsed.rejected).toEqual([]);
    expect(parsed.filters).toEqual([
      { field: 'service.name', op: 'in', value: '', values: ['web', 'checkout'] },
      { field: 'http.status_code', op: 'in', value: '', values: ['500', '503'] },
      { field: 'service.name', op: 'in', value: '', values: ['a, b', "it's"] },
    ]);
  });

  it('sets aside a list with nothing in it, too much in it, or on a document', () => {
    const many = Array.from({ length: 65 }, (_, index) => `'v${index}'`).join(', ');

    expect(parseTraceStatement('service.name in ()', fields).rejected).toEqual(['service.name in ()']);
    expect(parseTraceStatement(`service.name in (${many})`, fields).filters).toEqual([]);
    expect(parseTraceStatement("attributes in ('x')", fields).rejected).toEqual(["attributes in ('x')"]);
  });

  it('reads a field by the name the stream gives it, and no other', () => {
    const parsed = parseTraceStatement("service_name = 'checkout' AND operation contains 'cart' AND status = 'ERROR'", fields);

    expect(parsed.filters).toEqual([]);
    expect(parsed.rejected).toEqual(["service_name = 'checkout'", "operation contains 'cart'", "status = 'ERROR'"]);
  });

  it('keeps JSON fields out of field mode but available in SQL mode', () => {
    const attributes = fields.find((field) => field.name === 'attributes')!;
    expect(isTraceFieldQueryable(attributes, 'fields')).toBe(false);
    expect(isTraceFieldQueryable(attributes, 'sql')).toBe(true);
    expect(parseTraceStatement("attributes = '{}'", fields).rejected).toEqual(["attributes = '{}'"]);
  });

  it('inserts typed placeholders and HAVING clauses into aggregate SQL', () => {
    const status = fields.find((field) => field.name === 'http.status_code')!;
    const spanCount = fields.find((field) => field.name === 'span_count')!;
    expect(insertTraceClause('', status)).toBe('http.status_code >= 0');

    const template = traceSqlTemplate('default');
    expect(appendTraceSqlFieldFilter(template, status, 'default')).toContain(
      'HAVING MAX(CASE WHEN "http.status_code" >= 0 THEN 1 ELSE 0 END) = 1',
    );
    expect(appendTraceSqlFieldFilter(template, spanCount, 'default')).toContain(
      'HAVING COUNT(*) >= 0',
    );
  });

  it('reads back a quoted value as it was written', () => {
    // The writer escapes a quote and a backslash; a value with either used to never match itself.
    expect(parseTraceStatement("name = 'it\\'s' AND path contains 'C:\\\\tmp'", [
      ...fields,
      { name: 'name', dataType: 'utf8', physical: true, scope: 'span' },
      { name: 'path', dataType: 'utf8', physical: true, scope: 'span' },
    ]).filters).toEqual([
      { field: 'name', op: '=', value: "it's" },
      { field: 'path', op: 'contains', value: 'C:\\tmp' },
    ]);
  });

  it('sets a comparison with no value aside instead of searching for it as text', () => {
    const parsed = parseTraceStatement('service.name = AND checkout', fields);

    expect(parsed.rejected).toEqual(['service.name =']);
    expect(parsed.q).toBe('checkout');
  });

  it('only offers a field query the fields it can name', () => {
    const header: TraceFieldDef = { name: 'http.request.header.x-request-id', dataType: 'utf8', physical: true, scope: 'span' };

    expect(isTraceFieldQueryable(header, 'fields')).toBe(false);
    expect(isTraceFieldQueryable(header, 'sql')).toBe(true);
  });

  it('knows the four fields a trace has one value for', () => {
    expect(['trace_id', 'duration_ns', 'span_count', 'error_count'].every(isTraceLevelField)).toBe(true);
    expect(['span_id', 'service.name', 'status_code'].some(isTraceLevelField)).toBe(false);
  });

  it('asks a count of the stream when the statement lists spans', () => {
    const spanCount = fields.find((field) => field.name === 'span_count')!;

    // A statement with a row per span has no trace to count; HAVING there is not valid SQL.
    expect(appendTraceSqlFieldFilter('SELECT * FROM "default" LIMIT 10', spanCount, 'default')).toBe(
      'SELECT * FROM "default"\nWHERE trace_id IN (SELECT trace_id FROM "default" GROUP BY trace_id HAVING COUNT(*) >= 0) LIMIT 10',
    );
  });

  it('selects the same canonical trace stream contract as the backend', () => {
    const stream = (name: string, fieldNames: string[]) => ({
      id: name,
      label: name,
      name,
      streamType: 'traces' as const,
      type: 'traces' as const,
      schema: {
        fields: fieldNames.map((fieldName) => ({
          name: fieldName,
          dataType: 'utf8' as const,
          nullable: true,
          indexed: false,
        })),
      },
      retention: null,
      effectiveRetention: { days: 30 },
      settings: {
        description: null,
        indexRules: [],
        maxQueryRangeHours: null,
        queryable: true,
      },
      createdAt: 1,
      updatedAt: 1,
    });
    const canonicalFields = [
      'trace_id',
      'span_id',
      'service.name',
      'name',
      'start_time_unix_nano',
      'end_time_unix_nano',
      'status_code',
    ];

    const selected = selectTraceStream([
      stream('default', ['trace_id', 'span_id']),
      stream('otel', canonicalFields),
    ]);
    const preferred = selectTraceStream(
      [
        stream('default', canonicalFields),
        stream('checkout-traces', canonicalFields),
      ],
      'checkout-traces',
    );
    const summarized = selectTraceStream([
      stream('default', canonicalFields),
      stream('prod', [...canonicalFields, 'molesignal.trace.summary']),
    ]);

    expect(selected?.name).toBe('otel');
    expect(preferred?.name).toBe('checkout-traces');
    expect(summarized?.name).toBe('prod');
  });

  it('asks alternatives as a condition tree beside the filters that all hold', () => {
    const parsed = parseTraceStatement(
      "http.status_code = 500 AND (service.name = 'checkout' OR service.name = 'payments') AND timeout",
      fields,
    );

    expect(parsed.filters).toEqual([{ field: 'http.status_code', op: '=', value: '500' }]);
    expect(parsed.q).toBe('timeout');
    expect(parsed.where).toEqual({
      or: [
        { field: 'service.name', op: '=', value: 'checkout' },
        { field: 'service.name', op: '=', value: 'payments' },
      ],
    });
    expect(parsed.rejected).toEqual([]);
  });

  it('reads AND before OR, text of one group as one phrase', () => {
    const parsed = parseTraceStatement('timeout OR "connection reset" AND upstream OR span_count > 5', fields);

    expect(parsed.filters).toEqual([]);
    expect(parsed.where).toEqual({
      or: [{ text: 'timeout' }, { text: 'connection reset upstream' }, { field: 'span_count', op: '>', value: '5' }],
    });
  });

  it('leaves out the whole of alternatives a part of which it cannot read', () => {
    const parsed = parseTraceStatement("service.name = 'a' OR service_name = 'b'", fields);

    expect(parsed.where).toBeUndefined();
    expect(parsed.rejected).toEqual(["service.name = 'a' OR service_name = 'b'"]);
  });

  it('splits at the AND that joins the statement, not inside brackets or an OR', () => {
    expect(splitTraceStatement("a = 1 AND (b = 2 OR c = 3) AND x in ('p AND q')")).toEqual([
      'a = 1',
      '(b = 2 OR c = 3)',
      "x in ('p AND q')",
    ]);
    expect(splitTraceStatement('a = 1 OR b = 2 AND c = 3')).toEqual(['a = 1 OR b = 2 AND c = 3']);
  });

  it('starts a clause that narrows a statement of alternatives as a whole', () => {
    const service = fields.find((field) => field.name === 'service.name')!;

    expect(insertTraceClause("status_code = 'ERROR' OR timeout", service)).toBe(
      "(status_code = 'ERROR' OR timeout) AND service.name = ''",
    );
  });
});
