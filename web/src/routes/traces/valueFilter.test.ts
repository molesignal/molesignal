import { describe, expect, it } from 'vitest';

import { deriveTraceFields, parseTraceStatement, traceSqlTemplate, type TraceFieldDef } from './fieldQueryModel';
import {
  appendTraceSqlValueFilter,
  canFilterTraceValue,
  hasTraceValueFilter,
  insertTraceValueClause,
  removeTraceValueFilter,
} from './valueFilter';

const fields = deriveTraceFields([
  { name: 'trace_id', data_type: 'utf8', nullable: false, indexed: true },
  { name: 'span_id', data_type: 'utf8', nullable: false, indexed: true },
  { name: 'service.name', data_type: 'utf8', nullable: true, indexed: true },
  { name: 'name', data_type: 'utf8', nullable: true, indexed: true },
  { name: 'duration_ns', data_type: 'int64', nullable: false, indexed: false },
  { name: 'http.status_code', data_type: 'int64', nullable: true, indexed: false },
  { name: 'ratio', data_type: 'float64', nullable: true, indexed: false },
  { name: 'conflict', data_type: 'bool', nullable: true, indexed: false },
  { name: 'seen_at', data_type: 'timestamp', nullable: true, indexed: false },
  { name: 'attributes', data_type: 'json', nullable: true, indexed: false },
  { name: 'http.request.header.x-request-id', data_type: 'utf8', nullable: true, indexed: false },
]);
const field = (name: string): TraceFieldDef => fields.find((candidate) => candidate.name === name)!;

describe('whether a value can be filtered by', () => {
  it('asks a field query for a value, and only leaves one out where a trace has one value', () => {
    expect(canFilterTraceValue(field('service.name'), 'checkout', '=', 'fields')).toBe(true);
    // Any span of another service would satisfy "not checkout": that is not leaving it out.
    expect(canFilterTraceValue(field('service.name'), 'checkout', '!=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('trace_id'), 'abc', '!=', 'fields')).toBe(true);
    expect(canFilterTraceValue(field('span_count'), '3', '!=', 'fields')).toBe(true);
  });

  it('lets SQL leave a value out of any field', () => {
    expect(canFilterTraceValue(field('service.name'), 'checkout', '!=', 'sql')).toBe(true);
    expect(canFilterTraceValue(field('http.request.header.x-request-id'), 'r-1', '=', 'sql')).toBe(true);
  });

  it('refuses what the trace list would not take or would never match', () => {
    expect(canFilterTraceValue(field('name'), ' padded', '=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('name'), 'x'.repeat(513), '=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('name'), '日'.repeat(171), '=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('name'), '', '=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('attributes'), '{}', '=', 'fields')).toBe(false);
    // A field query names its fields bare; this one cannot be named.
    expect(canFilterTraceValue(field('http.request.header.x-request-id'), 'r-1', '=', 'fields')).toBe(false);
  });

  it('keeps a value to its field type', () => {
    expect(canFilterTraceValue(field('http.status_code'), '500', '=', 'fields')).toBe(true);
    expect(canFilterTraceValue(field('http.status_code'), '500.5', '=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('ratio'), '0.25', '>=', 'fields')).toBe(true);
    expect(canFilterTraceValue(field('ratio'), 'NaN', '=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('conflict'), 'true', '=', 'fields')).toBe(true);
    expect(canFilterTraceValue(field('conflict'), 'yes', '=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('seen_at'), '1791701313537673', '>=', 'fields')).toBe(true);
    expect(canFilterTraceValue(field('seen_at'), '2026-10-11T14:58:10', '>=', 'fields')).toBe(false);
    expect(canFilterTraceValue(field('name'), 'checkout', '>=', 'fields')).toBe(false);
  });
});

describe('a value in a field query', () => {
  it('is written as its type reads, and joined to what is there', () => {
    expect(insertTraceValueClause('', field('service.name'), 'checkout', '=')).toBe("service.name = 'checkout'");
    expect(insertTraceValueClause('error', field('http.status_code'), '500', '=')).toBe('error AND http.status_code = 500');
    expect(insertTraceValueClause('error AND ', field('conflict'), 'true', '=')).toBe('error AND conflict = true');
    expect(insertTraceValueClause('', field('duration_ns'), '14000000', '>=')).toBe('duration_ns >= 14000000');
  });

  it('survives the statement being read back, quotes and all', () => {
    const tricky = "it's a \\ path AND more";
    const statement = insertTraceValueClause('', field('name'), tricky, '=');

    expect(parseTraceStatement(statement, fields).filters).toEqual([{ field: 'name', op: '=', value: tricky }]);
  });

  it('takes the place of the empty comparison the field left', () => {
    expect(insertTraceValueClause("service.name = '' AND error", field('service.name'), 'checkout', '='))
      .toBe("service.name = 'checkout' AND error");
  });

  it('is not written twice, however the first was spelt', () => {
    const statement = 'service_name eq "checkout"';

    expect(insertTraceValueClause(statement, field('service.name'), 'checkout', '=')).toBe(statement);
  });

  it('replaces the same value asked the other way round', () => {
    expect(insertTraceValueClause("trace_id != 'abc'", field('trace_id'), 'abc', '=')).toBe("trace_id = 'abc'");
    expect(insertTraceValueClause('span_count = 3', field('span_count'), '3', '!=')).toBe('span_count != 3');
  });

  it('keeps a second value for a field of a span, and replaces it for a field of a trace', () => {
    // A trace can have spans of both services; it cannot have two ids.
    expect(insertTraceValueClause("service.name = 'web'", field('service.name'), 'checkout', '='))
      .toBe("service.name = 'web' AND service.name = 'checkout'");
    expect(insertTraceValueClause("trace_id = 'abc'", field('trace_id'), 'def', '=')).toBe("trace_id = 'def'");
  });

  it('moves a bound instead of stacking another on the same side', () => {
    expect(insertTraceValueClause('duration_ns >= 5 AND duration_ns <= 90', field('duration_ns'), '20', '>='))
      .toBe('duration_ns >= 20 AND duration_ns <= 90');
    expect(insertTraceValueClause('duration_ns > 5', field('duration_ns'), '90', '<=')).toBe('duration_ns > 5 AND duration_ns <= 90');
  });

  it('is found and taken out again, leaving the rest as written', () => {
    const statement = "error AND service_name = 'checkout' AND span_count >= 3";

    expect(hasTraceValueFilter('fields', statement, field('service.name'), 'checkout', '=', 'default')).toBe(true);
    expect(hasTraceValueFilter('fields', statement, field('service.name'), 'web', '=', 'default')).toBe(false);
    expect(removeTraceValueFilter('fields', statement, field('service.name'), 'checkout', '=', 'default'))
      .toBe('error AND span_count >= 3');
    expect(removeTraceValueFilter('fields', "service.name = 'checkout'", field('service.name'), 'checkout', '=', 'default')).toBe('');
  });
});

describe('a value in SQL', () => {
  const template = traceSqlTemplate('default');
  const spans = 'SELECT * FROM "default"\nORDER BY start_time_unix_nano DESC\nLIMIT 200';

  it('asks a statement with a row per trace whether some span has it, or none', () => {
    expect(appendTraceSqlValueFilter(template, field('service.name'), "o'brien", '=', 'default')).toContain(
      `HAVING MAX(CASE WHEN "service.name" = 'o''brien' THEN 1 ELSE 0 END) = 1\nORDER BY`,
    );
    expect(appendTraceSqlValueFilter(template, field('service.name'), 'web', '!=', 'default')).toContain(
      `HAVING MAX(CASE WHEN "service.name" = 'web' THEN 1 ELSE 0 END) = 0`,
    );
    expect(appendTraceSqlValueFilter('', field('conflict'), 'true', '=', 'default')).toContain(
      'HAVING MAX(CASE WHEN "conflict" = TRUE THEN 1 ELSE 0 END) = 1',
    );
  });

  it('compares what a trace has one of on the trace', () => {
    expect(appendTraceSqlValueFilter(template, field('span_count'), '3', '>=', 'default')).toContain('HAVING COUNT(*) >= 3');
    expect(appendTraceSqlValueFilter(template, field('duration_ns'), '500', '<=', 'default')).toContain(
      'HAVING (MAX(end_time_unix_nano) - MIN(start_time_unix_nano)) <= 500',
    );
    // The id is the same on every span, so it is compared before they are grouped.
    expect(appendTraceSqlValueFilter(template, field('trace_id'), 'abc', '=', 'default')).toContain(
      `FROM "default"\nWHERE "trace_id" = 'abc'\nGROUP BY trace_id`,
    );
  });

  it('asks a statement with a row per span about the span', () => {
    expect(appendTraceSqlValueFilter(spans, field('service.name'), 'web', '=', 'default')).toContain(
      `FROM "default"\nWHERE "service.name" = 'web'\nORDER BY`,
    );
    // A span without the field is not one that has the value.
    expect(appendTraceSqlValueFilter(spans, field('service.name'), 'web', '!=', 'default')).toContain(
      `WHERE "service.name" IS DISTINCT FROM 'web'`,
    );
    expect(appendTraceSqlValueFilter(spans, field('duration_ns'), '500', '>=', 'default')).toContain('WHERE "duration_ns" >= 500');
    expect(appendTraceSqlValueFilter(spans, field('seen_at'), '1791701313537673', '>=', 'default')).toContain(
      'WHERE CAST("seen_at" AS BIGINT) >= 1791701313537673',
    );
    // A span has no count of its own: the count is asked of its trace.
    expect(appendTraceSqlValueFilter(spans, field('span_count'), '3', '>=', 'default')).toContain(
      'WHERE trace_id IN (SELECT trace_id FROM "default" GROUP BY trace_id HAVING COUNT(*) >= 3)',
    );
  });

  it('is found whole, not as the start of a longer number', () => {
    const twenty = appendTraceSqlValueFilter(template, field('http.status_code'), '20', '=', 'default');

    expect(hasTraceValueFilter('sql', twenty, field('http.status_code'), '20', '=', 'default')).toBe(true);
    expect(hasTraceValueFilter('sql', twenty, field('http.status_code'), '2', '=', 'default')).toBe(false);
    expect(hasTraceValueFilter('sql', '', field('http.status_code'), '20', '=', 'default')).toBe(false);
    expect(appendTraceSqlValueFilter(twenty, field('http.status_code'), '20', '=', 'default')).toBe(twenty);
  });

  it('is taken out again whether it stands alone, first or last', () => {
    const service = field('service.name');
    const status = field('http.status_code');
    const one = appendTraceSqlValueFilter(template, service, 'web', '=', 'default');
    const two = appendTraceSqlValueFilter(one, status, '500', '=', 'default');

    expect(removeTraceValueFilter('sql', one, service, 'web', '=', 'default')).toBe(template);
    expect(removeTraceValueFilter('sql', two, status, '500', '=', 'default')).toBe(one);
    expect(removeTraceValueFilter('sql', two, service, 'web', '=', 'default')).toBe(
      appendTraceSqlValueFilter(template, status, '500', '=', 'default'),
    );
  });
});
