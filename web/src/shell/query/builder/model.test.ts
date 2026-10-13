import { describe, expect, it } from 'vitest';

import {
  clauseLabel,
  detectStatementLanguage,
  makeClause,
  operatorsForKind,
  withClause,
  withoutClause,
  withoutUnsupported,
  type BuilderStatement,
} from './model';

describe('detectStatementLanguage', () => {
  it('reads a statement that opens with SELECT or WITH as SQL', () => {
    expect(detectStatementLanguage('SELECT * FROM "app"')).toBe('sql');
    expect(detectStatementLanguage('  select count(*) from app')).toBe('sql');
    expect(detectStatementLanguage('WITH recent AS (SELECT 1) SELECT * FROM recent')).toBe('sql');
  });

  it('looks past leading comments', () => {
    expect(detectStatementLanguage('-- slow requests\nSELECT * FROM "app"')).toBe('sql');
    expect(detectStatementLanguage('/* note */ select 1')).toBe('sql');
  });

  it('reads everything else as a field query', () => {
    expect(detectStatementLanguage("trace_id = 'abc'")).toBe('fields');
    expect(detectStatementLanguage('')).toBe('fields');
    expect(detectStatementLanguage('timeout')).toBe('fields');
    // A field or a word that merely starts like a keyword is not SQL.
    expect(detectStatementLanguage("selected = 'true'")).toBe('fields');
    expect(detectStatementLanguage('within 5 minutes')).toBe('fields');
  });
});

describe('builder clauses', () => {
  it('writes a number or a boolean bare and anything else in quotes', () => {
    expect(makeClause({ name: 'status', kind: 'number' }, '>=', ' 500 ')).toEqual({
      field: 'status',
      op: '>=',
      value: '500',
      quoted: false,
    });
    expect(makeClause({ name: 'ok', kind: 'boolean' }, '=', 'TRUE').quoted).toBe(false);
    expect(makeClause({ name: 'status', kind: 'number' }, '=', '5xx').quoted).toBe(true);
    expect(makeClause({ name: 'service', kind: 'text' }, '=', '200').quoted).toBe(true);
    expect(makeClause({ name: 'unlisted' }, '=', '200').quoted).toBe(true);
  });

  it('offers the comparisons the kind of field takes', () => {
    expect(operatorsForKind('text')).toEqual(['=', '!=', 'contains']);
    expect(operatorsForKind('number')).toContain('>=');
    expect(operatorsForKind('boolean')).toEqual(['=', '!=']);
  });

  it('adds, replaces and removes without touching the rest', () => {
    const statement: BuilderStatement = {
      clauses: [{ field: 'a', op: '=', value: '1' }],
      text: 'timeout',
      unsupported: ['x ~ y', 'z ?'],
    };
    const added = withClause(statement, null, { field: 'b', op: '!=', value: '2' });
    expect(added.clauses.map(clauseLabel)).toEqual(['a = 1', 'b != 2']);
    expect(added.text).toBe('timeout');

    const replaced = withClause(added, 0, { field: 'a', op: '>', value: '3' });
    expect(replaced.clauses.map(clauseLabel)).toEqual(['a > 3', 'b != 2']);

    expect(withoutClause(replaced, 0).clauses.map(clauseLabel)).toEqual(['b != 2']);
    expect(withoutUnsupported(statement, 0).unsupported).toEqual(['z ?']);
  });
});
