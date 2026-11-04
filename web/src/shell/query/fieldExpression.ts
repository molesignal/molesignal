/**
 * A field query read as the alternatives and conditions it joins.
 *
 * `AND` binds tighter than `OR` and brackets group, both words in any case:
 * `a OR b AND c` is `a OR (b AND c)`. A word inside quotes, or inside the
 * brackets of a value (`MATCH(message, 'a or b')`, `x in ('a', 'b')`), is no
 * keyword. What a part between the keywords says, a comparison, a search
 * function or text to find, is for each page to read.
 */
export type FieldExpression =
  | {
      kind: 'and' | 'or';
      items: FieldExpression[];
      /** The group as written, its brackets included when it had them. */
      text: string;
    }
  | { kind: 'part'; text: string };

type Keyword = 'AND' | 'OR';

/**
 * The pieces of `text` between the keyword where it stands outside quotes and
 * brackets, each trimmed; empty pieces are left out. A closing bracket with
 * none open is part of the text.
 */
function splitAt(text: string, keyword: Keyword): string[] {
  const pieces: string[] = [];
  let start = 0;
  let depth = 0;
  let quote: "'" | '"' | null = null;
  let escaped = false;

  for (let index = 0; index < text.length; index += 1) {
    const character = text[index]!;

    if (escaped) {
      escaped = false;
      continue;
    }
    if (character === '\\') {
      escaped = true;
      continue;
    }
    if (quote) {
      if (character === quote) quote = null;
      continue;
    }
    if (character === "'" || character === '"') {
      quote = character;
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
    if (depth > 0 || !startsKeyword(text, index, keyword)) continue;

    pieces.push(text.slice(start, index));
    index += keyword.length - 1;
    start = index + 1;
  }

  pieces.push(text.slice(start));
  return pieces.map((piece) => piece.trim()).filter(Boolean);
}

/** Whether `keyword` stands at `index` as a word of its own: spaces or brackets around it. */
function startsKeyword(text: string, index: number, keyword: Keyword): boolean {
  if (text.slice(index, index + keyword.length).toUpperCase() !== keyword) return false;

  const before = index === 0 ? ' ' : text[index - 1]!;
  const after = index + keyword.length === text.length ? ' ' : text[index + keyword.length]!;

  return /[\s)]/.test(before) && /[\s(]/.test(after);
}

/** The inside of a part that is one group in brackets, `(a OR b)`; `null` for any other part. */
function bracketed(part: string): string | null {
  if (!part.startsWith('(') || !part.endsWith(')')) return null;

  let depth = 0;
  let quote: "'" | '"' | null = null;
  let escaped = false;

  for (let index = 0; index < part.length; index += 1) {
    const character = part[index]!;

    if (escaped) {
      escaped = false;
      continue;
    }
    if (character === '\\') {
      escaped = true;
      continue;
    }
    if (quote) {
      if (character === quote) quote = null;
      continue;
    }
    if (character === "'" || character === '"') {
      quote = character;
      continue;
    }
    if (character === '(') depth += 1;
    if (character !== ')') continue;

    depth -= 1;
    // The first bracket closes before the end: `(a) OR (b)` is two groups.
    if (depth === 0) return index === part.length - 1 ? part.slice(1, -1) : null;
  }

  return null;
}

/** A group of `kind` over `items`, a group of the same kind among them taken apart. */
function group(kind: 'and' | 'or', items: FieldExpression[], text: string): FieldExpression {
  if (items.length === 1) return items[0]!;

  return {
    kind,
    items: items.flatMap((item) => (item.kind === kind ? item.items : [item])),
    text,
  };
}

function readPart(part: string): FieldExpression {
  const inside = bracketed(part);
  if (inside === null) return { kind: 'part', text: part };

  const read = readExpression(inside);
  if (read === null) return { kind: 'part', text: part };

  return read.kind === 'part' ? read : { ...read, text: part };
}

function readExpression(text: string): FieldExpression | null {
  const alternatives = splitAt(text, 'OR').map((alternative) => {
    const parts = splitAt(alternative, 'AND').map(readPart);
    return parts.length === 0 ? null : group('and', parts, alternative);
  });
  const read = alternatives.filter((item): item is FieldExpression => item !== null);

  return read.length === 0 ? null : group('or', read, text.trim());
}

/** The statement read as AND and OR of its parts; `null` for one that says nothing. */
export function parseFieldExpression(statement: string): FieldExpression | null {
  return readExpression(statement);
}

/** Whether the text joins alternatives with an `OR` that no bracket holds. */
export function hasTopLevelOr(text: string): boolean {
  return splitAt(text, 'OR').length > 1;
}

/**
 * The parts of the statement that all have to hold, as written: what an `AND`
 * outside brackets joins. A statement with an `OR` outside brackets is one part.
 */
export function fieldConjuncts(statement: string): string[] {
  const trimmed = statement.trim();
  if (!trimmed) return [];
  if (hasTopLevelOr(trimmed)) return [trimmed];

  return splitAt(trimmed, 'AND');
}

/**
 * Parts that all have to hold, joined with `AND`: a part with an `OR` of its
 * own in brackets, so it stays one part. One part left that is a group in
 * brackets loses them.
 */
export function joinFieldConjuncts(parts: readonly string[]): string {
  const kept = parts.map((part) => part.trim()).filter(Boolean);
  if (kept.length === 1) return bracketed(kept[0]!)?.trim() ?? kept[0]!;

  return kept.map((part) => (hasTopLevelOr(part) ? `(${part})` : part)).join(' AND ');
}

/**
 * The statement with `clause` as one more condition that has to hold. The
 * statement is kept as written; one with an `OR` outside brackets is put in
 * them, so the clause narrows all of it. An `AND` left at the end, a condition
 * still being written, takes the clause as it is.
 */
export function andFieldClause(statement: string, clause: string): string {
  const trimmed = statement.trim();
  if (!trimmed) return clause;
  if (/\bAND\s*$/i.test(trimmed)) return `${trimmed} ${clause}`;

  return hasTopLevelOr(trimmed) ? `(${trimmed}) AND ${clause}` : `${trimmed} AND ${clause}`;
}

/**
 * The alternatives of an expression, each the parts that all hold in it: the
 * shape of the builder's rows. A part that is itself a group of alternatives
 * inside a row stays one part, as written.
 */
export function fieldAlternatives(expression: FieldExpression | null): FieldExpression[][] {
  if (expression === null) return [];
  const alternatives = expression.kind === 'or' ? expression.items : [expression];

  return alternatives.map((alternative) => (alternative.kind === 'and' ? alternative.items : [alternative]));
}

const ESCAPES: Record<string, string> = { n: '\n', r: '\r', t: '\t' };

/**
 * What a part that is one quoted string says, `"connection reset"` as
 * `connection reset`: text found as a phrase, keywords and all. `null` for a
 * part that is not one quoted string.
 */
export function quotedText(part: string): string | null {
  const quote = part[0];
  if ((quote !== "'" && quote !== '"') || part.length < 2) return null;

  let value = '';

  for (let index = 1; index < part.length; index += 1) {
    const character = part[index]!;

    if (character === '\\' && index < part.length - 1) {
      const next = part[index + 1]!;
      value += ESCAPES[next] ?? next;
      index += 1;
      continue;
    }
    if (character === quote) return index === part.length - 1 ? value : null;
    value += character;
  }

  return null;
}
