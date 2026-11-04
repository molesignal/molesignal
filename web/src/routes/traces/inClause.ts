import type { TraceCondition, TraceFilter } from '@/api/web';

/** The most values one list holds: what the trace list takes in one filter. */
export const MAX_TRACE_IN_VALUES = 64;

/**
 * A field compared with a list of values, read from a part of a field query:
 * `service.name in ('checkout', 'payments')` keeps a trace one of whose spans
 * has any of them. `null` for any other part.
 */
export function traceInClauseOf(part: string): TraceFilter | null {
  const match = part.trim().match(/^([a-zA-Z_][\w.]*)\s+in\s*\(([\s\S]*)\)$/i);
  if (!match) return null;

  const values = splitTraceList(match[2]!)
    .map(unquoteTraceValue)
    .filter((value) => value !== '');

  return { field: match[1]!, op: 'in', value: '', values };
}

/** The part of a field query that compares a field with a list of values already written as literals. */
export function writeTraceInClause(field: string, literals: readonly string[]): string {
  return `${field} in (${literals.join(', ')})`;
}

/** A comparison as it reads, a list of values in its brackets. */
export function describeTraceFilter(filter: TraceFilter): string {
  return filter.op === 'in' ? `${filter.field} in (${(filter.values ?? []).join(', ')})` : `${filter.field} ${filter.op} ${filter.value}`;
}

/** A condition tree as it reads: each group in brackets, text in quotes. */
export function describeTraceCondition(condition: TraceCondition): string {
  if ('and' in condition) return `(${condition.and.map(describeTraceCondition).join(' AND ')})`;
  if ('or' in condition) return `(${condition.or.map(describeTraceCondition).join(' OR ')})`;
  if ('text' in condition) return JSON.stringify(condition.text);
  return describeTraceFilter(condition);
}

/** The items of a list, split at the commas outside quotes. */
function splitTraceList(input: string): string[] {
  const items: string[] = [];
  let start = 0;
  let quote: "'" | '"' | null = null;
  let escaped = false;

  for (let index = 0; index < input.length; index += 1) {
    const character = input[index]!;
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
    if (character !== ',') continue;

    items.push(input.slice(start, index));
    start = index + 1;
  }

  items.push(input.slice(start));
  return items.map((item) => item.trim());
}

/** A value as a field query writes it, bare or quoted, read back as it is. */
export function unquoteTraceValue(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length >= 2) {
    const first = trimmed[0];
    const last = trimmed[trimmed.length - 1];
    if (first === '"' && last === '"') {
      try { return JSON.parse(trimmed) as string; } catch { return trimmed.slice(1, -1); }
    }
    // The writer escapes a quote and a backslash inside single quotes; reading undoes exactly that.
    if (first === "'" && last === "'") return trimmed.slice(1, -1).replace(/\\(['\\])/g, '$1');
  }
  return trimmed;
}
