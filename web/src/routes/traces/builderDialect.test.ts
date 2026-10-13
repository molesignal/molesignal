import { describe, expect, it } from 'vitest';

import { makeClause, withClause } from '@/shell/query/builder/model';

import { traceBuilderDialect, traceBuilderFields } from './builderDialect';
import { parseTraceStatement, type TraceFieldDef } from './fieldQueryModel';

const FIELDS: TraceFieldDef[] = [
  { name: 'trace_id', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'service.name', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'duration_ns', dataType: 'int64', physical: true, scope: 'trace_aggregate' },
  { name: 'sampled', dataType: 'bool', physical: true, scope: 'span' },
  { name: 'attributes', dataType: 'json', physical: true, scope: 'span' },
];

describe('traceBuilderDialect', () => {
  const dialect = traceBuilderDialect(FIELDS);

  it('shows a field query as filters, search text and what the schema rejects', () => {
    const parsed = dialect.parse(
      "service_name = 'checkout' AND duration_ns > 500 AND timeout AND unknown_field = 1",
    );

    // The alias is read as the field the stream has.
    expect(parsed.clauses).toEqual([
      { field: 'service.name', op: '=', value: 'checkout' },
      { field: 'duration_ns', op: '>', value: '500' },
    ]);
    expect(parsed.text).toBe('timeout');
    expect(parsed.unsupported).toEqual(['unknown_field = 1']);
  });

  it('writes text in quotes and numbers bare, the way the statement is typed', () => {
    const written = dialect.serialize({
      clauses: [
        makeClause({ name: 'trace_id', kind: 'text' }, '=', 'abc123'),
        makeClause({ name: 'duration_ns', kind: 'number' }, '>=', '1000'),
        makeClause({ name: 'sampled', kind: 'boolean' }, '=', 'true'),
      ],
      text: 'timeout',
      unsupported: [],
    });

    expect(written).toBe("trace_id = 'abc123' AND duration_ns >= 1000 AND sampled = true AND timeout");
  });

  it('a filter added in the builder is the filter the typed statement sends', () => {
    const typed = "trace_id = 'abc123' AND duration_ns >= 1000";
    const built = dialect.serialize(
      withClause(
        dialect.parse("trace_id = 'abc123'"),
        null,
        makeClause({ name: 'duration_ns', kind: 'number' }, '>=', '1000'),
      ),
    );

    expect(parseTraceStatement(built, FIELDS)).toEqual(parseTraceStatement(typed, FIELDS));
  });

  it('offers comparisons by the kind of field and leaves JSON columns out', () => {
    // A field whose name is not one word cannot be written in a field query.
    expect(traceBuilderFields([
      ...FIELDS,
      { name: 'http.request.header.x-request-id', dataType: 'utf8', physical: true, scope: 'span' },
    ])).toEqual(traceBuilderFields(FIELDS));

    const fields = traceBuilderFields(FIELDS);

    expect(fields.map((field) => field.name)).not.toContain('attributes');
    expect(dialect.operators(fields.find((field) => field.name === 'trace_id'))).toEqual([
      '=',
      '!=',
      'contains',
    ]);
    expect(dialect.operators(fields.find((field) => field.name === 'duration_ns'))).toContain('>=');
    expect(dialect.operators(fields.find((field) => field.name === 'sampled'))).toEqual(['=', '!=']);
  });
});
