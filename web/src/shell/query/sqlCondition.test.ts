import { describe, expect, it } from 'vitest';

import { addSqlCondition } from './sqlCondition';

const CLAUSE = `"service" = 'api'`;

describe('addSqlCondition', () => {
  it('starts a WHERE before the clauses that follow, or at the end', () => {
    expect(addSqlCondition('SELECT * FROM "app"\nORDER BY _timestamp DESC\nLIMIT 200', 'WHERE', CLAUSE)).toBe(
      `SELECT * FROM "app"\nWHERE "service" = 'api'\nORDER BY _timestamp DESC\nLIMIT 200`,
    );
    expect(addSqlCondition('SELECT * FROM "app"', 'WHERE', CLAUSE)).toBe(`SELECT * FROM "app"\nWHERE "service" = 'api'`);
  });

  it('adds to the conditions there are with AND', () => {
    expect(addSqlCondition(`SELECT * FROM "app"\nWHERE level = 'error'\nLIMIT 200`, 'WHERE', CLAUSE)).toBe(
      `SELECT * FROM "app"\nWHERE level = 'error'\n  AND "service" = 'api'\nLIMIT 200`,
    );
  });

  it('puts alternatives in brackets first, so the condition narrows all of them', () => {
    const either = `SELECT * FROM "app"\nWHERE level = 'error' OR status >= 500\nORDER BY _timestamp DESC`;
    const narrowed = addSqlCondition(either, 'WHERE', CLAUSE);

    expect(narrowed).toBe(
      `SELECT * FROM "app"\nWHERE (level = 'error' OR status >= 500)\n  AND "service" = 'api'\nORDER BY _timestamp DESC`,
    );
    // The next condition finds them in brackets already.
    expect(addSqlCondition(narrowed, 'WHERE', `"host" = 'a'`)).toBe(
      `SELECT * FROM "app"\nWHERE (level = 'error' OR status >= 500)\n  AND "service" = 'api'\n  AND "host" = 'a'\nORDER BY _timestamp DESC`,
    );
    // Keywords read in any case.
    expect(addSqlCondition('select * from t where a = 1 or b = 2 order by x', 'WHERE', CLAUSE)).toBe(
      `select * from t where (a = 1 or b = 2)\n  AND "service" = 'api' order by x`,
    );
  });

  it('leaves an OR alone where it is no alternative of the statement', () => {
    for (const conditions of [
      '(a = 1 OR b = 2) AND c = 3',
      `message = 'timeout or reset'`,
      `msg = 'it''s or not'`,
      `"or" = 1`,
      'a = 1 /* or b = 2 */',
      'orders > 1 AND vendor = 1',
    ]) {
      expect(addSqlCondition(`SELECT * FROM t WHERE ${conditions}`, 'WHERE', CLAUSE)).toBe(
        `SELECT * FROM t WHERE ${conditions}\n  AND ${CLAUSE}`,
      );
    }
  });

  it('closes the brackets on a line of their own after a comment', () => {
    expect(addSqlCondition('SELECT * FROM t\nWHERE a = 1 OR b = 2 -- the usual two\nLIMIT 5', 'WHERE', CLAUSE)).toBe(
      `SELECT * FROM t\nWHERE (a = 1 OR b = 2 -- the usual two\n)\n  AND ${CLAUSE}\nLIMIT 5`,
    );
  });

  it('reads the WHERE of the statement, not of a subquery or a CTE', () => {
    expect(
      addSqlCondition('WITH recent AS (SELECT * FROM t WHERE a = 1 OR b = 2)\nSELECT * FROM recent\nORDER BY ts', 'WHERE', CLAUSE),
    ).toBe(`WITH recent AS (SELECT * FROM t WHERE a = 1 OR b = 2)\nSELECT * FROM recent\nWHERE ${CLAUSE}\nORDER BY ts`);
    expect(addSqlCondition('SELECT * FROM (SELECT * FROM t ORDER BY ts LIMIT 5) AS s', 'WHERE', CLAUSE)).toBe(
      `SELECT * FROM (SELECT * FROM t ORDER BY ts LIMIT 5) AS s\nWHERE ${CLAUSE}`,
    );
    expect(addSqlCondition("SELECT * FROM t WHERE a IN (SELECT a FROM u WHERE b = 1 OR c = 2)", 'WHERE', CLAUSE)).toBe(
      `SELECT * FROM t WHERE a IN (SELECT a FROM u WHERE b = 1 OR c = 2)\n  AND ${CLAUSE}`,
    );
  });

  it('adds to WHERE before the grouping, and to HAVING after it', () => {
    const grouped = 'SELECT level, COUNT(*) FROM t\nWHERE a = 1 OR b = 2\nGROUP BY level\nHAVING COUNT(*) > 1 OR MAX(x) = 1\nORDER BY 2 DESC';

    expect(addSqlCondition(grouped, 'WHERE', CLAUSE)).toBe(
      `SELECT level, COUNT(*) FROM t\nWHERE (a = 1 OR b = 2)\n  AND ${CLAUSE}\nGROUP BY level\nHAVING COUNT(*) > 1 OR MAX(x) = 1\nORDER BY 2 DESC`,
    );
    expect(addSqlCondition(grouped, 'HAVING', 'MIN(y) = 0')).toBe(
      'SELECT level, COUNT(*) FROM t\nWHERE a = 1 OR b = 2\nGROUP BY level\nHAVING (COUNT(*) > 1 OR MAX(x) = 1)\n  AND MIN(y) = 0\nORDER BY 2 DESC',
    );
    expect(addSqlCondition('SELECT level FROM t\nGROUP BY level\nLIMIT 3', 'HAVING', 'COUNT(*) > 1')).toBe(
      'SELECT level FROM t\nGROUP BY level\nHAVING COUNT(*) > 1\nLIMIT 3',
    );
  });

  it('completes a WHERE left without a condition', () => {
    expect(addSqlCondition('SELECT * FROM t WHERE', 'WHERE', CLAUSE)).toBe(`SELECT * FROM t WHERE ${CLAUSE}`);
  });
});
