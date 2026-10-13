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
  traceSqlTemplate,
  type TraceFieldDef,
} from './fieldQueryModel';

describe('trace field query model', () => {
  const fields = deriveTraceFields([
    { name: 'trace_id', data_type: 'utf8', nullable: false, indexed: true },
    { name: 'service.name', data_type: 'utf8', nullable: true, indexed: true },
    { name: 'http.status_code', data_type: 'int64', nullable: true, indexed: false },
    { name: 'conflict', data_type: 'bool', nullable: true, indexed: false },
    { name: 'attributes', data_type: 'json', nullable: true, indexed: false },
    { name: 'molesignal.trace.span_count', data_type: 'int64', nullable: true, indexed: false },
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
      { name: 'trace_id', data_type: 'utf8', nullable: false, indexed: true },
      { name: 'span_id', data_type: 'utf8', nullable: false, indexed: true },
      { name: 'name', data_type: 'utf8', nullable: false, indexed: true },
      { name: 'duration_ns', data_type: 'int64', nullable: false, indexed: false },
      { name: 'status_code', data_type: 'utf8', nullable: true, indexed: true },
      { name: 'service.name', data_type: 'utf8', nullable: true, indexed: true },
      { name: 'host.name', data_type: 'utf8', nullable: true, indexed: false },
      { name: 'telemetry.sdk.name', data_type: 'utf8', nullable: true, indexed: false },
      { name: 'scope_name', data_type: 'utf8', nullable: true, indexed: false },
      { name: 'http.request.method', data_type: 'utf8', nullable: true, indexed: false },
      { name: 'attributes', data_type: 'json', nullable: true, indexed: false },
      { name: 'conflict', data_type: 'bool', nullable: true, indexed: false },
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
      stream_type: 'traces' as const,
      type: 'traces' as const,
      schema: {
        fields: fieldNames.map((fieldName) => ({
          name: fieldName,
          data_type: 'utf8' as const,
          nullable: true,
          indexed: false,
        })),
      },
      retention: null,
      effective_retention: { days: 30 },
      settings: {
        description: null,
        index_rules: [],
        retention_filter: null,
        keep_conditions: [],
        max_query_range_hours: null,
        flatten_level: null,
        use_stream_stats_for_partitioning: false,
        store_original_data: false,
        enable_distinct_values: true,
        queryable: true,
      },
      created_at_micros: 1,
      updated_at_micros: 1,
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

    expect(selected?.name).toBe('otel');
    expect(preferred?.name).toBe('checkout-traces');
  });
});
