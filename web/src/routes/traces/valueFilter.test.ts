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
  { name: 'trace_id', dataType: 'utf8', nullable: false, indexed: true },
  { name: 'span_id', dataType: 'utf8', nullable: false, indexed: true },
  { name: 'service.name', dataType: 'utf8', nullable: true, indexed: true },
  { name: 'name', dataType: 'utf8', nullable: true, indexed: true },
  { name: 'duration_ns', dataType: 'int64', nullable: false, indexed: false },
  { name: 'http.status_code', dataType: 'int64', nullable: true, indexed: false },
  { name: 'ratio', dataType: 'float64', nullable: true, indexed: false },
  { name: 'conflict', dataType: 'bool', nullable: true, indexed: false },
  { name: 'seen_at', dataType: 'timestamp', nullable: true, indexed: false },
  { name: 'attributes', dataType: 'json', nullable: true, indexed: false },
  { name: 'http.request.header.x-request-id', dataType: 'utf8', nullable: true, indexed: false },
]);
const field = (name: string): TraceFieldDef => fields.find((candidate) => candidate.name === name)!;

describe('whether a value can be filtered by', () => {
  it('asks a field query for a value, or for the traces without it, on every field', () => {
    expect(canFilterTraceValue(field('service.name'), 'checkout', '=', 'fields')).toBe(true);
    // A trace passes "not checkout" when none of its spans is of checkout.
    expect(canFilterTraceValue(field('service.name'), 'checkout', '!=', 'fields')).toBe(true);
    expect(canFilterTraceValue(field('http.status_code'), '500', '!=', 'fields')).toBe(true);
    expect(canFilterTraceValue(field('conflict'), 'true', '!=', 'fields')).toBe(true);
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
    const statement = 'service.name eq "checkout"';

    expect(insertTraceValueClause(statement, field('service.name'), 'checkout', '=')).toBe(statement);
  });

  it('replaces the same value asked the other way round', () => {
    expect(insertTraceValueClause("trace_id != 'abc'", field('trace_id'), 'abc', '=')).toBe("trace_id = 'abc'");
    expect(insertTraceValueClause('span_count = 3', field('span_count'), '3', '!=')).toBe('span_count != 3');
  });

  it('leaves a value of a span out in place of the same value asked for, and beside another', () => {
    expect(insertTraceValueClause("service.name = 'checkout'", field('service.name'), 'checkout', '!='))
      .toBe("service.name != 'checkout'");
    // A trace can call `web` and not `payments`.
    expect(insertTraceValueClause("service.name = 'web'", field('service.name'), 'payments', '!='))
      .toBe("service.name = 'web' AND service.name != 'payments'");
    expect(parseTraceStatement("service.name != 'payments'", fields).filters)
      .toEqual([{ field: 'service.name', op: '!=', value: 'payments' }]);
  });

  it('joins a second value asked for to the first, as boxes ticked in a list', () => {
    // Any of them: a trace with a span of either service, either of two traces.
    expect(insertTraceValueClause("service.name = 'web'", field('service.name'), 'checkout', '='))
      .toBe("service.name in ('web', 'checkout')");
    expect(insertTraceValueClause("trace_id = 'abc'", field('trace_id'), 'def', '=')).toBe("trace_id in ('abc', 'def')");
    expect(insertTraceValueClause("error AND service.name in ('web', 'checkout')", field('service.name'), 'payments', '='))
      .toBe("error AND service.name in ('web', 'checkout', 'payments')");
    expect(insertTraceValueClause('http.status_code = 500', field('http.status_code'), '503', '=')).toBe('http.status_code in (500, 503)');
    // The empty comparison the field's "+" left goes when a value joins the one asked for.
    expect(insertTraceValueClause("service.name = 'web' AND service.name = ''", field('service.name'), 'checkout', '='))
      .toBe("service.name in ('web', 'checkout')");
  });

  it('finds a value in a list, and takes it out leaving the others asked for', () => {
    const list = "service.name in ('web', 'checkout', 'payments')";

    expect(hasTraceValueFilter('fields', list, field('service.name'), 'checkout', '=', 'default')).toBe(true);
    expect(hasTraceValueFilter('fields', list, field('service.name'), 'cart', '=', 'default')).toBe(false);
    expect(hasTraceValueFilter('fields', list, field('service.name'), 'checkout', '!=', 'default')).toBe(false);
    expect(insertTraceValueClause(list, field('service.name'), 'checkout', '=')).toBe(list);
    expect(removeTraceValueFilter('fields', list, field('service.name'), 'checkout', '=', 'default'))
      .toBe("service.name in ('web', 'payments')");
    expect(removeTraceValueFilter('fields', "error AND service.name in ('web', 'checkout')", field('service.name'), 'web', '=', 'default'))
      .toBe("error AND service.name = 'checkout'");
  });

  it('keeps a list beside a value left out, and takes the value out of it when it was asked for', () => {
    const list = "service.name in ('web', 'checkout')";

    expect(insertTraceValueClause(list, field('service.name'), 'payments', '!='))
      .toBe("service.name in ('web', 'checkout') AND service.name != 'payments'");
    expect(insertTraceValueClause(list, field('service.name'), 'checkout', '!='))
      .toBe("service.name = 'web' AND service.name != 'checkout'");
  });

  it('moves a bound instead of stacking another on the same side', () => {
    expect(insertTraceValueClause('duration_ns >= 5 AND duration_ns <= 90', field('duration_ns'), '20', '>='))
      .toBe('duration_ns >= 20 AND duration_ns <= 90');
    expect(insertTraceValueClause('duration_ns > 5', field('duration_ns'), '90', '<=')).toBe('duration_ns > 5 AND duration_ns <= 90');
  });

  it('is found and taken out again, leaving the rest as written', () => {
    const statement = "error AND service.name = 'checkout' AND span_count >= 3";

    expect(hasTraceValueFilter('fields', statement, field('service.name'), 'checkout', '=', 'default')).toBe(true);
    expect(hasTraceValueFilter('fields', statement, field('service.name'), 'web', '=', 'default')).toBe(false);
    expect(removeTraceValueFilter('fields', statement, field('service.name'), 'checkout', '=', 'default'))
      .toBe('error AND span_count >= 3');
    expect(removeTraceValueFilter('fields', "service.name = 'checkout'", field('service.name'), 'checkout', '=', 'default')).toBe('');
  });

  it('narrows a statement of alternatives as a whole, and takes the value out again', () => {
    const either = "service.name = 'checkout' OR service.name = 'payments'";
    const narrowed = insertTraceValueClause(either, field('http.status_code'), '500', '=');

    expect(narrowed).toBe("(service.name = 'checkout' OR service.name = 'payments') AND http.status_code = 500");
    expect(hasTraceValueFilter('fields', narrowed, field('http.status_code'), '500', '=', 'default')).toBe(true);
    expect(removeTraceValueFilter('fields', narrowed, field('http.status_code'), '500', '=', 'default')).toBe(either);
    // A value of a field the alternatives speak of joins no list of theirs.
    expect(insertTraceValueClause(either, field('service.name'), 'web', '=')).toBe(
      "(service.name = 'checkout' OR service.name = 'payments') AND service.name = 'web'",
    );
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
      `WHERE ("service.name" IS DISTINCT FROM 'web')\nORDER BY`,
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

  it('keeps a value left out in brackets, so a condition added after it holds beside it', () => {
    const service = field('service.name');
    const status = field('http.status_code');
    const left = appendTraceSqlValueFilter(spans, service, 'web', '!=', 'default');
    const both = appendTraceSqlValueFilter(left, status, '500', '=', 'default');

    // Without the brackets the engine would read `IS DISTINCT FROM ('web' AND "http.status_code" = 500)`.
    expect(both).toBe(
      `SELECT * FROM "default"\nWHERE ("service.name" IS DISTINCT FROM 'web')\n  AND "http.status_code" = 500\nORDER BY start_time_unix_nano DESC\nLIMIT 200`,
    );
    expect(appendTraceSqlValueFilter(left, service, 'web', '!=', 'default')).toBe(left);
    expect(hasTraceValueFilter('sql', both, service, 'web', '!=', 'default')).toBe(true);
    expect(removeTraceValueFilter('sql', both, service, 'web', '!=', 'default')).toBe(
      appendTraceSqlValueFilter(spans, status, '500', '=', 'default'),
    );
    expect(removeTraceValueFilter('sql', both, status, '500', '=', 'default')).toBe(left);
  });

  it('narrows every alternative of the statement, its WHERE or its HAVING put in brackets', () => {
    const status = field('http.status_code');
    const either = `SELECT * FROM "default"\nWHERE "service.name" = 'a' OR "service.name" = 'b'\nORDER BY start_time_unix_nano DESC\nLIMIT 200`;
    const narrowed = appendTraceSqlValueFilter(either, status, '500', '=', 'default');

    expect(narrowed).toBe(
      `SELECT * FROM "default"\nWHERE ("service.name" = 'a' OR "service.name" = 'b')\n  AND "http.status_code" = 500\nORDER BY start_time_unix_nano DESC\nLIMIT 200`,
    );
    // Taking the value out again leaves the alternatives in their brackets.
    expect(removeTraceValueFilter('sql', narrowed, status, '500', '=', 'default')).toBe(
      `SELECT * FROM "default"\nWHERE ("service.name" = 'a' OR "service.name" = 'b')\nORDER BY start_time_unix_nano DESC\nLIMIT 200`,
    );

    const grouped = `SELECT trace_id FROM "default"\nGROUP BY trace_id\nHAVING COUNT(*) > 10 OR SUM(CASE WHEN status_code = 'ERROR' THEN 1 ELSE 0 END) > 0\nLIMIT 200`;
    expect(appendTraceSqlValueFilter(grouped, field('service.name'), 'web', '=', 'default')).toBe(
      `SELECT trace_id FROM "default"\nGROUP BY trace_id\nHAVING (COUNT(*) > 10 OR SUM(CASE WHEN status_code = 'ERROR' THEN 1 ELSE 0 END) > 0)\n  AND MAX(CASE WHEN "service.name" = 'web' THEN 1 ELSE 0 END) = 1\nLIMIT 200`,
    );
  });
});
