/**
 * JSON text laid out as `JSON.stringify(value, null, 2)` lays it out: a member or an element on a
 * line of its own, two spaces of indent a level, a space after the colon, an empty object or array
 * on one line.
 *
 * Only the space between the tokens changes. The text is not parsed into values and written again,
 * which would round a number longer than a double holds, move a key that reads as an index to the
 * front and keep only the last of a key written twice.
 */

const INDENT = '  ';

/** The text laid out, or `null` when it is not JSON. */
export function layOutJson(text: string): string | null {
  if (!isJson(text)) return null;

  let laidOut = '';
  let depth = 0;
  let index = 0;

  while (index < text.length) {
    const char = text[index]!;

    if (char === '"') {
      const end = stringEnd(text, index);
      laidOut += text.slice(index, end);
      index = end;
      continue;
    }

    index += 1;
    if (isSpace(char)) continue;

    if (char === '{' || char === '[') {
      const close = char === '{' ? '}' : ']';
      const next = skipSpace(text, index);

      if (text[next] === close) {
        laidOut += char + close;
        index = next + 1;
      } else {
        depth += 1;
        laidOut += char + lineBreak(depth);
      }
    } else if (char === '}' || char === ']') {
      depth -= 1;
      laidOut += lineBreak(depth) + char;
    } else if (char === ',') {
      laidOut += char + lineBreak(depth);
    } else if (char === ':') {
      laidOut += ': ';
    } else {
      laidOut += char;
    }
  }

  return laidOut;
}

/**
 * Where `offset` in `text` stands in `laidOut`, the same text laid out: after as many of its
 * characters, the space between the tokens not counted.
 */
export function offsetInLayout(text: string, offset: number, laidOut: string): number {
  const before = tokenReader();
  let count = 0;

  for (let index = 0; index < Math.min(offset, text.length); index += 1) {
    if (before(text[index]!)) count += 1;
  }
  if (count === 0) return 0;

  const after = tokenReader();

  for (let index = 0; index < laidOut.length; index += 1) {
    if (after(laidOut[index]!)) count -= 1;
    if (count === 0) return index + 1;
  }

  return laidOut.length;
}

function isJson(text: string): boolean {
  if (text.trim() === '') return false;

  try {
    JSON.parse(text);
    return true;
  } catch {
    return false;
  }
}

/** Where the string that opens at `start` ends: after its closing quote. */
function stringEnd(text: string, start: number): number {
  let index = start + 1;

  while (index < text.length) {
    const char = text[index];
    if (char === '"') return index + 1;
    index += char === '\\' ? 2 : 1;
  }

  return text.length;
}

/**
 * Takes the characters of a text in turn and says of each whether it is part of a token: every
 * character of a string is, an escaped quote among them, and the space between tokens is not.
 */
function tokenReader(): (char: string) => boolean {
  let inString = false;
  let escaped = false;

  return (char) => {
    if (inString) {
      if (escaped) escaped = false;
      else if (char === '\\') escaped = true;
      else if (char === '"') inString = false;
      return true;
    }

    if (char === '"') inString = true;
    return !isSpace(char);
  };
}

function skipSpace(text: string, index: number): number {
  let next = index;
  while (next < text.length && isSpace(text[next]!)) next += 1;
  return next;
}

function isSpace(char: string): boolean {
  return char === ' ' || char === '\n' || char === '\r' || char === '\t';
}

function lineBreak(depth: number): string {
  return `\n${INDENT.repeat(depth)}`;
}
