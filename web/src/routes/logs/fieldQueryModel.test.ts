import { describe, expect, it } from 'vitest';

import {
  appendLogFieldClause,
  appendLogFieldValueClause,
  buildLogFieldQuerySql,
  deriveLogFields,
  formatLogFieldQueryValue,
  isLogFieldFilterable,
  logFieldClauseToSql,
  parseLogFieldStatement,
} from './fieldQueryModel';

describe('log field query model', () => {
  it('keeps AND inside a quoted value and splits only real clauses', () => {
    const parsed = parseLogFieldStatement(
      "message = 'checkout failed AND retry stopped' AND level = 'ERROR'",
    );

    expect(parsed.filters).toEqual([
      {
        field: 'message',
        op: '=',
        value: 'checkout failed AND retry stopped',
        quoted: true,
      },
      {
        field: 'level',
        op: '=',
        value: 'ERROR',
        quoted: true,
      },
    ]);
  });

  it('decodes escaped control characters in a quoted value', () => {
    const parsed = parseLogFieldStatement(
      String.raw`error = "first line\nsecond line\t'quoted'"`,
    );

    expect(parsed.filters[0]?.value).toBe("first line\nsecond line\t'quoted'");
  });

  it('supports a literal that contains a real line break', () => {
    const parsed = parseLogFieldStatement("error = 'first line\nsecond line'");

    expect(parsed.filters[0]?.value).toBe('first line\nsecond line');
    expect(parsed.freeText).toEqual([]);
  });

  it('escapes embedded apostrophes for SQL', () => {
    expect(logFieldClauseToSql({
      field: 'error',
      op: '=',
      value: "Did you mean 'app_logs._timestamp'?",
      quoted: true,
    })).toContain("Did you mean ''app_logs._timestamp''?");
  });

  it('adds a rendered-whitespace fallback for quoted text', () => {
    const sql = buildLogFieldQuerySql(
      '_molesignal',
      "error = 'first line second line'",
      200,
    );

    expect(sql).toContain('"error" = \'first line second line\'');
    expect(sql).toContain(
      "regexp_replace(CAST(\"error\" AS VARCHAR), '[[:space:]]+', ' ', 'g') = 'first line second line'",
    );
  });

  it('formats typed field values and appends include/exclude clauses', () => {
    expect(formatLogFieldQueryValue(500)).toBe('500');
    expect(formatLogFieldQueryValue(true)).toBe('true');
    expect(formatLogFieldQueryValue("can't\nretry")).toBe("'can\\'t\\nretry'");
    expect(appendLogFieldValueClause("level = 'ERROR'", 'service', 'checkout-api')).toBe(
      "level = 'ERROR' AND service = 'checkout-api'",
    );
    expect(appendLogFieldValueClause('', 'status_code', 500, 'exclude')).toBe(
      'status_code != 500',
    );
  });

  it('derives the complete typed schema before query results are available', () => {
    const fields = deriveLogFields([
      { name: 'status_code', data_type: 'int64', nullable: true, indexed: false },
      { name: 'sampled', data_type: 'bool', nullable: true, indexed: false },
      { name: 'payload', data_type: 'json', nullable: true, indexed: false },
    ]);

    expect(fields.map((field) => [field.name, field.dataType])).toEqual([
      ['status_code', 'int64'],
      ['sampled', 'bool'],
      ['payload', 'json'],
      ['_timestamp', 'timestamp'],
    ]);
    expect(fields.every((field) => field.count === '0')).toBe(true);
  });

  it('uses typed placeholders and parses numeric comparisons', () => {
    const [status] = deriveLogFields([
      { name: 'status_code', data_type: 'int64', nullable: true, indexed: false },
    ]);

    expect(appendLogFieldClause('', status!)).toBe('status_code >= 0');
    expect(parseLogFieldStatement('status_code >= 500').filters).toEqual([
      { field: 'status_code', op: '>=', value: '500', quoted: false },
    ]);
  });

  it('requires SQL mode for composite JSON fields', () => {
    const [payload] = deriveLogFields([
      { name: 'payload', data_type: 'json', nullable: true, indexed: false },
    ]);

    expect(isLogFieldFilterable(payload!, 'fields')).toBe(false);
    expect(isLogFieldFilterable(payload!, 'sql')).toBe(true);
  });
});
