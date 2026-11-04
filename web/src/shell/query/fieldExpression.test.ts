import { describe, expect, it } from 'vitest';

import {
  andFieldClause,
  type FieldExpression,
  fieldAlternatives,
  fieldConjuncts,
  hasTopLevelOr,
  joinFieldConjuncts,
  parseFieldExpression,
  quotedText,
} from './fieldExpression';

/** The expression as nested lists of its parts' text, to compare at a glance. */
function shape(expression: FieldExpression | null): unknown {
  if (expression === null) return null;
  if (expression.kind === 'part') return expression.text;

  return { [expression.kind]: expression.items.map(shape) };
}

describe('parseFieldExpression', () => {
  it('reads AND before OR', () => {
    expect(shape(parseFieldExpression('a = 1 OR b = 2 AND c = 3'))).toEqual({
      or: ['a = 1', { and: ['b = 2', 'c = 3'] }],
    });
    expect(shape(parseFieldExpression('a = 1 AND b = 2 OR c = 3'))).toEqual({
      or: [{ and: ['a = 1', 'b = 2'] }, 'c = 3'],
    });
  });

  it('groups what brackets hold', () => {
    const read = parseFieldExpression('level = "error" AND (service.name = "checkout" OR service.name = "payments")');

    expect(shape(read)).toEqual({
      and: ['level = "error"', { or: ['service.name = "checkout"', 'service.name = "payments"'] }],
    });
    // A group keeps its brackets in its text, as it was written.
    expect(read?.kind === 'and' && read.items[1]?.text).toBe('(service.name = "checkout" OR service.name = "payments")');
  });

  it('reads the keywords in any case, as words of their own', () => {
    expect(shape(parseFieldExpression('timeout or "connection reset"'))).toEqual({
      or: ['timeout', '"connection reset"'],
    });
    expect(shape(parseFieldExpression('(a = 1)OR(b = 2) and c = 3'))).toEqual({
      or: ['a = 1', { and: ['b = 2', 'c = 3'] }],
    });
    // Not a keyword: part of a word, or of a value.
    expect(shape(parseFieldExpression('vendor = oracle AND orders > 2'))).toEqual({
      and: ['vendor = oracle', 'orders > 2'],
    });
  });

  it('leaves quotes and the brackets of a value whole', () => {
    expect(shape(parseFieldExpression("MATCH(message, 'a or b') AND x in ('or', 'and')"))).toEqual({
      and: ["MATCH(message, 'a or b')", "x in ('or', 'and')"],
    });
    expect(shape(parseFieldExpression('message = "it\\" OR x"'))).toBe('message = "it\\" OR x"');
  });

  it('takes nested groups of one kind apart', () => {
    expect(shape(parseFieldExpression('a OR (b OR (c))'))).toEqual({ or: ['a', 'b', 'c'] });
    expect(shape(parseFieldExpression('(a AND b) AND c'))).toEqual({ and: ['a', 'b', 'c'] });
    expect(shape(parseFieldExpression('((a OR b))'))).toEqual({ or: ['a', 'b'] });
  });

  it('keeps what is not a group as a part', () => {
    expect(shape(parseFieldExpression('a = 1 AND (b = 2 OR c = 3'))).toEqual({
      and: ['a = 1', '(b = 2 OR c = 3'],
    });
    expect(shape(parseFieldExpression('(a) b'))).toBe('(a) b');
    expect(shape(parseFieldExpression('()'))).toBe('()');
    expect(parseFieldExpression('  ')).toBeNull();
    // An unfinished statement reads as what it holds so far.
    expect(shape(parseFieldExpression('a = 1 AND '))).toBe('a = 1');
    expect(shape(parseFieldExpression('a = 1 OR'))).toBe('a = 1');
  });
});

describe('conjuncts', () => {
  it('are the parts AND joins outside brackets, as written', () => {
    expect(fieldConjuncts('a = 1 AND (b = 2 OR c = 3) AND d')).toEqual(['a = 1', '(b = 2 OR c = 3)', 'd']);
    expect(fieldConjuncts('a = 1 OR b = 2 AND c = 3')).toEqual(['a = 1 OR b = 2 AND c = 3']);
    expect(fieldConjuncts('a = 1 AND ')).toEqual(['a = 1']);
    expect(fieldConjuncts('')).toEqual([]);
  });

  it('join back with brackets where an OR needs them', () => {
    expect(joinFieldConjuncts(['a OR b', 'c = 1'])).toBe('(a OR b) AND c = 1');
    expect(joinFieldConjuncts(['(a OR b)', 'c = 1'])).toBe('(a OR b) AND c = 1');
    expect(joinFieldConjuncts(['(a OR b)'])).toBe('a OR b');
    expect(joinFieldConjuncts(['a', '', 'b'])).toBe('a AND b');
  });

  it('take a clause that narrows the whole statement', () => {
    expect(andFieldClause('', 'x = 1')).toBe('x = 1');
    expect(andFieldClause('a = 1', 'x = 1')).toBe('a = 1 AND x = 1');
    expect(andFieldClause('a = 1 OR b = 2', 'x = 1')).toBe('(a = 1 OR b = 2) AND x = 1');
    expect(andFieldClause('(a = 1 OR b = 2)', 'x = 1')).toBe('(a = 1 OR b = 2) AND x = 1');
    // A condition still being written takes it where it stands.
    expect(andFieldClause('a = 1 OR b = 2 AND ', 'x = 1')).toBe('a = 1 OR b = 2 AND x = 1');
  });

  it('tell an OR outside brackets', () => {
    expect(hasTopLevelOr('a OR b')).toBe(true);
    expect(hasTopLevelOr('(a OR b) AND c')).toBe(false);
    expect(hasTopLevelOr("x = 'a or b'")).toBe(false);
  });
});

describe('fieldAlternatives', () => {
  it('are the rows of the builder: an OR of parts that all hold', () => {
    const rows = (statement: string) =>
      fieldAlternatives(parseFieldExpression(statement)).map((row) => row.map((part) => part.text));

    expect(rows('a AND b OR c')).toEqual([['a', 'b'], ['c']]);
    expect(rows('a AND (b OR c)')).toEqual([['a', '(b OR c)']]);
    expect(rows('(a AND b) OR (c AND d)')).toEqual([['a', 'b'], ['c', 'd']]);
    expect(rows('')).toEqual([]);
  });
});

describe('quotedText', () => {
  it('reads a part that is one quoted string', () => {
    expect(quotedText('"connection reset"')).toBe('connection reset');
    expect(quotedText("'it\\'s or not'")).toBe("it's or not");
    expect(quotedText('"a" b "c"')).toBeNull();
    expect(quotedText('timeout')).toBeNull();
    expect(quotedText('"open')).toBeNull();
  });
});
