/**
 * The visual form of a field query.
 *
 * A field query is a flat list: comparisons joined by AND, plus text to search
 * for. That is exactly what a row of filters can show, so the builder and the
 * Code editor are two views of one statement. Each signal says how its own
 * statement reads and writes through a `BuilderDialect`.
 */
export type BuilderOperator =
  | '='
  | '!='
  | '>'
  | '>='
  | '<'
  | '<='
  | 'contains'
  | 'match'
  | 'match_text';

export type BuilderFieldKind = 'text' | 'number' | 'boolean';

export interface BuilderField {
  name: string;
  /** Decides which comparisons apply and whether a value is written in quotes. */
  kind: BuilderFieldKind;
}

export interface BuilderClause {
  field: string;
  op: BuilderOperator;
  value: string;
  /** The value is written in quotes: `'200'` is text, `200` is a number. */
  quoted?: boolean;
}

export interface BuilderStatement {
  clauses: BuilderClause[];
  /** Text to search for, as typed. */
  text: string;
  /** Parts of the statement the builder cannot show as a filter; they are kept as written. */
  unsupported: string[];
}

export interface BuilderDialect {
  parse: (statement: string) => BuilderStatement;
  serialize: (statement: BuilderStatement) => string;
  /** The comparisons a field takes; `undefined` is a field the schema does not list. */
  operators: (field: BuilderField | undefined) => readonly BuilderOperator[];
}

export const TEXT_OPERATORS: readonly BuilderOperator[] = ['=', '!=', 'contains'];
export const NUMBER_OPERATORS: readonly BuilderOperator[] = ['=', '!=', '>', '>=', '<', '<='];
export const BOOLEAN_OPERATORS: readonly BuilderOperator[] = ['=', '!='];

export function operatorsForKind(kind: BuilderFieldKind): readonly BuilderOperator[] {
  if (kind === 'number') return NUMBER_OPERATORS;
  if (kind === 'boolean') return BOOLEAN_OPERATORS;
  return TEXT_OPERATORS;
}

/** A filter as one line of text: what its chip says and how it is announced. */
export function clauseLabel(clause: BuilderClause): string {
  return `${clause.field} ${clause.op} ${clause.value}`;
}

/** Whether a value for this kind of field is written bare (a number, true / false) or in quotes. */
export function isBareValue(kind: BuilderFieldKind | undefined, value: string): boolean {
  if (kind === 'number') return /^-?\d+(?:\.\d+)?$/.test(value.trim());
  if (kind === 'boolean') return /^(true|false)$/i.test(value.trim());
  return false;
}

/** A filter with the value as the user typed it, quoted or not as its field asks. */
export function makeClause(
  field: BuilderField | { name: string; kind?: undefined },
  op: BuilderOperator,
  value: string,
): BuilderClause {
  const trimmed = value.trim();
  return { field: field.name, op, value: trimmed, quoted: !isBareValue(field.kind, trimmed) };
}

export function withClause(
  statement: BuilderStatement,
  index: number | null,
  clause: BuilderClause,
): BuilderStatement {
  const clauses =
    index === null
      ? [...statement.clauses, clause]
      : statement.clauses.map((current, position) => (position === index ? clause : current));
  return { ...statement, clauses };
}

export function withoutClause(statement: BuilderStatement, index: number): BuilderStatement {
  return { ...statement, clauses: statement.clauses.filter((_, position) => position !== index) };
}

export function withoutUnsupported(statement: BuilderStatement, index: number): BuilderStatement {
  return {
    ...statement,
    unsupported: statement.unsupported.filter((_, position) => position !== index),
  };
}

export type StatementLanguage = 'fields' | 'sql';

/**
 * Which language a statement typed into the Code editor is in. A statement that
 * opens with SELECT or WITH is SQL; anything else is a field query
 * (`trace_id = '…'`), so both can be written in the one editor.
 */
export function detectStatementLanguage(statement: string): StatementLanguage {
  // Leading comments say nothing about the language of what follows them.
  const code = statement.replace(/^(?:\s+|--[^\n]*(?:\n|$)|\/\*[\s\S]*?\*\/)+/, '');
  return /^(?:select|with)\b/i.test(code) ? 'sql' : 'fields';
}
