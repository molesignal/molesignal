import { describe, expect, it } from 'vitest';

import {
  type BuilderClause,
  type BuilderPart,
  clauseLabel,
  detectStatementLanguage,
  makeClause,
  operatorsForKind,
  readBuilderStatement,
  withAlternative,
  withClause,
  withoutClause,
  withoutUnsupported,
  writeBuilderStatement,
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
      groups: [
        { clauses: [{ field: 'a', op: '=', value: '1' }], text: 'timeout', unsupported: ['x ~ y', 'z ?'] },
        { clauses: [{ field: 'c', op: '=', value: '3' }], text: '', unsupported: [] },
      ],
    };
    const added = withClause(statement, 0, null, { field: 'b', op: '!=', value: '2' });
    expect(added.groups[0]!.clauses.map(clauseLabel)).toEqual(['a = 1', 'b != 2']);
    expect(added.groups[0]!.text).toBe('timeout');
    expect(added.groups[1]).toBe(statement.groups[1]);

    const replaced = withClause(added, 0, 0, { field: 'a', op: '>', value: '3' });
    expect(replaced.groups[0]!.clauses.map(clauseLabel)).toEqual(['a > 3', 'b != 2']);

    expect(withoutClause(replaced, 0, 0).groups[0]!.clauses.map(clauseLabel)).toEqual(['b != 2']);
    expect(withoutUnsupported(statement, 0, 0).groups[0]!.unsupported).toEqual(['z ?']);

    const another = withAlternative(statement, { field: 'd', op: '=', value: '4' });
    expect(another.groups.map((group) => group.clauses.map(clauseLabel))).toEqual([['a = 1'], ['c = 3'], ['d = 4']]);
  });

  it('starts the first group of an empty statement', () => {
    expect(withClause({ groups: [] }, 0, null, { field: 'a', op: '=', value: '1' }).groups).toEqual([
      { clauses: [{ field: 'a', op: '=', value: '1' }], text: '', unsupported: [] },
    ]);
  });
});

describe('builder statements', () => {
  // A comparison `field op value`, text in quotes or a word to find, `??` something it cannot read.
  const readPart = (text: string): BuilderPart => {
    const comparison = /^(\w+) (=|!=) (\w+)$/.exec(text);
    if (comparison) return { kind: 'clause', clause: { field: comparison[1]!, op: comparison[2] as '=', value: comparison[3]! } };
    return text.startsWith('??') ? { kind: 'unsupported', text } : { kind: 'text', text };
  };
  const writeClause = (clause: BuilderClause) => `${clause.field} ${clause.op} ${clause.value}`;
  const read = (statement: string, alternatives = true) =>
    readBuilderStatement(statement, readPart, { alternatives, joinText: ' AND ' });

  it('reads each alternative as a group, AND before OR', () => {
    expect(read('a = 1 AND timeout OR b = 2').groups).toEqual([
      { clauses: [{ field: 'a', op: '=', value: '1' }], text: 'timeout', unsupported: [] },
      { clauses: [{ field: 'b', op: '=', value: '2' }], text: '', unsupported: [] },
    ]);
    expect(read('').groups).toEqual([]);
  });

  it('keeps alternatives inside a group as written', () => {
    const { groups } = read('a = 1 AND (b = 2 OR c = 3)');

    expect(groups).toHaveLength(1);
    expect(groups[0]!.unsupported).toEqual(['(b = 2 OR c = 3)']);
    expect(writeBuilderStatement({ groups }, writeClause)).toBe('a = 1 AND (b = 2 OR c = 3)');
  });

  it('writes groups back joined by OR, brackets where a part has an OR of its own', () => {
    const groups = read('a = 1 OR b = 2 AND x').groups;
    expect(writeBuilderStatement({ groups }, writeClause)).toBe('a = 1 OR b = 2 AND x');

    // Text typed into a group with OR in it stays of that group.
    const typed = { groups: [{ ...groups[0]!, text: 'p OR q' }, groups[1]!] };
    expect(writeBuilderStatement(typed, writeClause)).toBe('a = 1 AND (p OR q) OR b = 2 AND x');

    // A group left empty is no alternative.
    const emptied = { groups: [{ clauses: [], text: '', unsupported: [] }, groups[1]!] };
    expect(writeBuilderStatement(emptied, writeClause)).toBe('b = 2 AND x');
  });

  it('keeps a statement of alternatives whole where the dialect takes none', () => {
    expect(read('a = 1 OR b = 2', false).groups).toEqual([
      { clauses: [], text: '', unsupported: ['a = 1 OR b = 2'] },
    ]);

    // A filter added beside it narrows the whole statement.
    const added = withClause(read('a = 1 OR b = 2', false), 0, null, { field: 'c', op: '=', value: '3' });
    expect(writeBuilderStatement(added, writeClause)).toBe('c = 3 AND (a = 1 OR b = 2)');
  });
});
