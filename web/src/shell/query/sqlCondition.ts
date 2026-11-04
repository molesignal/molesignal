/**
 * A condition added to an SQL statement a reader wrote: into the WHERE, or
 * the HAVING, of the statement itself, before the clauses that follow it.
 * Only what stands outside quotes, comments and brackets counts, so the WHERE
 * of a subquery or a CTE is not the statement's. A condition there that joins
 * alternatives with OR is put in brackets first, so the one added narrows all
 * of it rather than its last alternative.
 */

type ConditionKeyword = 'WHERE' | 'HAVING';

/** The clauses that come after each one, which an added condition goes before. */
const FOLLOWING: Record<ConditionKeyword, readonly string[]> = {
  WHERE: ['GROUP BY', 'HAVING', 'ORDER BY', 'LIMIT'],
  HAVING: ['ORDER BY', 'LIMIT'],
};

const patterns = new Map<string, RegExp>();

/** A keyword as a word of its own, its words apart by any space, in any case. */
function keywordPattern(keyword: string): RegExp {
  let pattern = patterns.get(keyword);
  if (!pattern) {
    pattern = new RegExp(`${keyword.split(' ').join('\\s+')}\\b`, 'iy');
    patterns.set(keyword, pattern);
  }
  return pattern;
}

/** Where a quoted string or identifier that opens at `start` closes: a doubled quote is part of it. */
function closingQuote(sql: string, start: number): number {
  const quote = sql[start]!;
  let index = start + 1;
  while (index < sql.length) {
    if (sql[index] === quote) {
      if (sql[index + 1] !== quote) return index;
      index += 2;
      continue;
    }
    index += 1;
  }
  return sql.length;
}

interface Scan {
  /** Each keyword found outside quotes, comments and brackets, where it begins. */
  found: Array<{ keyword: string; at: number }>;
  /** The text ends inside a `--` comment: whatever follows on its line is commented out. */
  inLineComment: boolean;
}

function scan(sql: string, keywords: readonly string[]): Scan {
  const found: Scan['found'] = [];
  let depth = 0;

  for (let index = 0; index < sql.length; index += 1) {
    const character = sql[index]!;

    if (character === "'" || character === '"') {
      index = closingQuote(sql, index);
      continue;
    }
    if (character === '-' && sql[index + 1] === '-') {
      const end = sql.indexOf('\n', index);
      if (end < 0) return { found, inLineComment: true };
      index = end;
      continue;
    }
    if (character === '/' && sql[index + 1] === '*') {
      const end = sql.indexOf('*/', index + 2);
      index = end < 0 ? sql.length : end + 1;
      continue;
    }
    if (character === '(') {
      depth += 1;
      continue;
    }
    if (character === ')') {
      depth = Math.max(0, depth - 1);
      continue;
    }
    if (depth > 0 || (index > 0 && /[\w$]/.test(sql[index - 1]!))) continue;

    for (const keyword of keywords) {
      const pattern = keywordPattern(keyword);
      pattern.lastIndex = index;
      if (pattern.test(sql)) found.push({ keyword, at: index });
    }
  }

  return { found, inLineComment: false };
}

/**
 * `statement` with `clause` as one more condition of its WHERE, or HAVING:
 * the keyword written before the clauses that follow when there is none,
 * `AND clause` after the conditions there are. The statement is otherwise
 * kept as written.
 */
export function addSqlCondition(statement: string, keyword: ConditionKeyword, clause: string): string {
  const next = scan(statement, FOLLOWING[keyword]).found[0];
  let boundary = next ? next.at : statement.length;
  // The clause that follows keeps the space before it, the line break included.
  while (next && boundary > 0 && /\s/.test(statement[boundary - 1]!)) boundary -= 1;

  const before = statement.slice(0, boundary).trimEnd();
  const after = statement.slice(boundary);
  const own = scan(before, [keyword]).found.at(-1);
  if (!own) return `${before}\n${keyword} ${clause}${after}`;

  const head = before.slice(0, own.at + keyword.length);
  const conditions = before.slice(own.at + keyword.length).trim();
  if (!conditions) return `${head} ${clause}${after}`;

  const conditionScan = scan(conditions, ['OR']);
  if (conditionScan.found.length === 0) return `${before}\n  AND ${clause}${after}`;

  // A closing bracket after a `--` comment would be commented out with it.
  const closing = conditionScan.inLineComment ? '\n)' : ')';
  return `${head} (${conditions}${closing}\n  AND ${clause}${after}`;
}
