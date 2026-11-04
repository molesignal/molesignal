import { fieldAlternatives, hasTopLevelOr, parseFieldExpression } from '@/shell/query/fieldExpression';

/**
 * The visual form of a field query.
 *
 * A field query joins comparisons and text to search for with AND and OR, AND
 * first. Its alternatives are rows of filters: a row holds where all of its
 * filters do, the statement where one row does. So the builder and the Code
 * editor are two views of one statement. Each signal says how its own
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

/** One alternative of a statement: the filters and the text that all hold in it. */
export interface BuilderGroup {
  clauses: BuilderClause[];
  /** Text to search for, as typed. */
  text: string;
  /** Parts the builder cannot show as a filter; they are kept as written. */
  unsupported: string[];
}

export interface BuilderStatement {
  /** The alternatives, joined by OR. A statement without OR is one group; an empty one has none. */
  groups: BuilderGroup[];
}

export interface BuilderDialect {
  parse: (statement: string) => BuilderStatement;
  serialize: (statement: BuilderStatement) => string;
  /** The comparisons a field takes; `undefined` is a field the schema does not list. */
  operators: (field: BuilderField | undefined) => readonly BuilderOperator[];
  /** The statement can hold alternatives: the builder offers another group of filters. */
  alternatives?: boolean;
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

export const EMPTY_GROUP: BuilderGroup = { clauses: [], text: '', unsupported: [] };

/** What one part of a statement is to the builder. */
export type BuilderPart =
  | { kind: 'clause'; clause: BuilderClause }
  | { kind: 'text'; text: string }
  | { kind: 'unsupported'; text: string };

/**
 * A statement read as groups: each alternative a group, each of its parts as
 * `readPart` reads it. A part that is itself a group of alternatives is kept
 * as written. A dialect without alternatives keeps a statement with an OR
 * outside brackets as one part.
 */
export function readBuilderStatement(
  statement: string,
  readPart: (text: string) => BuilderPart,
  { alternatives, joinText }: { alternatives: boolean; joinText: string },
): BuilderStatement {
  const rows = fieldAlternatives(parseFieldExpression(statement));
  if (!alternatives && rows.length > 1) return { groups: [{ ...EMPTY_GROUP, unsupported: [statement.trim()] }] };

  return {
    groups: rows.map((row) => {
      const group: BuilderGroup = { clauses: [], text: '', unsupported: [] };
      const texts: string[] = [];

      for (const part of row) {
        const read: BuilderPart = part.kind === 'part' ? readPart(part.text) : { kind: 'unsupported', text: part.text };
        if (read.kind === 'clause') group.clauses.push(read.clause);
        else if (read.kind === 'text') texts.push(read.text);
        else group.unsupported.push(read.text);
      }

      return { ...group, text: texts.join(joinText) };
    }),
  };
}

/**
 * Groups as a statement: the parts of a group joined by AND, the groups by
 * OR. A part with an OR of its own is put in brackets beside the others.
 */
export function writeBuilderStatement(
  { groups }: BuilderStatement,
  writeClause: (clause: BuilderClause) => string,
): string {
  return groups
    .map((group) => {
      const parts = [...group.clauses.map(writeClause), ...group.unsupported, group.text.trim()].filter(Boolean);

      return parts.length > 1 ? parts.map((part) => (hasTopLevelOr(part) ? `(${part})` : part)).join(' AND ') : (parts[0] ?? '');
    })
    .filter(Boolean)
    .join(' OR ');
}

function withGroup(statement: BuilderStatement, at: number, change: (group: BuilderGroup) => BuilderGroup): BuilderStatement {
  const groups = statement.groups.length > 0 ? statement.groups : [EMPTY_GROUP];
  return { groups: groups.map((group, position) => (position === at ? change(group) : group)) };
}

/** The statement with a filter added to group `at`, or put in place of its filter `index`. */
export function withClause(
  statement: BuilderStatement,
  at: number,
  index: number | null,
  clause: BuilderClause,
): BuilderStatement {
  return withGroup(statement, at, (group) => ({
    ...group,
    clauses:
      index === null
        ? [...group.clauses, clause]
        : group.clauses.map((current, position) => (position === index ? clause : current)),
  }));
}

export function withoutClause(statement: BuilderStatement, at: number, index: number): BuilderStatement {
  return withGroup(statement, at, (group) => ({
    ...group,
    clauses: group.clauses.filter((_, position) => position !== index),
  }));
}

export function withoutUnsupported(statement: BuilderStatement, at: number, index: number): BuilderStatement {
  return withGroup(statement, at, (group) => ({
    ...group,
    unsupported: group.unsupported.filter((_, position) => position !== index),
  }));
}

/** The statement with one more alternative, which holds where `clause` does. */
export function withAlternative(statement: BuilderStatement, clause: BuilderClause): BuilderStatement {
  return { groups: [...statement.groups, { ...EMPTY_GROUP, clauses: [clause] }] };
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
