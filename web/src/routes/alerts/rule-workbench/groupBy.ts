import type { QueryResult } from '@/types/query';

/** At most this many columns split a rule, as the backend allows. */
export const MAX_GROUP_BY_COLUMNS = 8;

/** The columns typed into "Split by": commas of either width separate them; repeats count once. */
export function parseGroupBy(text: string): string[] {
  const columns: string[] = [];
  for (const part of text.split(/[,，]/)) {
    const column = part.trim();
    if (column && !columns.includes(column)) columns.push(column);
  }
  return columns;
}

export function formatGroupBy(columns: readonly string[] | undefined): string {
  return (columns ?? []).join(', ');
}

function columnIndex(result: QueryResult, column: string): number {
  const wanted = column.toLowerCase();
  return result.columns.findIndex((name) => name.toLowerCase() === wanted);
}

/** The split columns a result with rows does not have; names match without regard to case, as the evaluator matches them. */
export function missingGroupColumns(result: QueryResult, columns: readonly string[]): string[] {
  if (result.rows.length === 0) return [];
  return columns.filter((column) => columnIndex(result, column) < 0);
}

/** How many groups the rows of a result fall into. */
export function countGroups(result: QueryResult, columns: readonly string[]): number {
  const indexes = columns.map((column) => columnIndex(result, column));
  if (indexes.length === 0 || indexes.some((index) => index < 0)) return 0;
  return new Set(result.rows.map((row) => JSON.stringify(indexes.map((index) => row[index] ?? null)))).size;
}
