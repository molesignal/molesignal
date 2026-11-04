import { addSqlCondition } from '@/shell/query/sqlCondition';

import type { PinnedLogFilter } from './conditions/model';
import { escapeSqlIdentifier, escapeSqlString, type LogFieldDef } from './fieldQueryModel';
import { defaultLogQuery } from './queryState';
import { displayLogValue } from './viewModel';

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function hasSqlFieldFilter(statement: string, field: string): boolean {
  const quotedField = `"${escapeSqlIdentifier(field)}"`;
  const escapedQuoted = escapeRegExp(quotedField);
  const escapedBare = escapeRegExp(field);
  const pattern = new RegExp(`(?:^|\\s|\\()(${escapedQuoted}|${escapedBare})\\s*(?:>=|<=|=|!=|<>|>|<|LIKE\\b|IN\\s*\\(|IS\\s+(?:NOT\\s+)?NULL)`, 'i');
  return pattern.test(statement);
}

export function appendSqlFieldFilter(statement: string, stream: string, field: LogFieldDef): string {
  const base = statement.trim() || defaultLogQuery(stream);
  if (hasSqlFieldFilter(base, field.name)) return statement;
  const identifier = `"${escapeSqlIdentifier(field.name)}"`;
  const clause = field.dataType === 'json'
    ? `CAST(${identifier} AS VARCHAR) LIKE '%"key"%'`
    : field.dataType === 'bool'
      ? `${identifier} = TRUE`
      : field.dataType === 'timestamp'
        ? `CAST(${identifier} AS BIGINT) >= 0`
        : ['int64', 'float64'].includes(field.dataType)
        ? `${identifier} >= 0`
        : `${identifier} = ''`;
  return addSqlCondition(base, 'WHERE', clause);
}

export function appendSqlFieldValueFilter(
  statement: string,
  stream: string,
  field: string,
  value: unknown,
  mode: 'include' | 'exclude',
): string {
  const base = statement.trim() || defaultLogQuery(stream);
  const sqlValue = typeof value === 'number' && Number.isFinite(value)
    ? String(value)
    : typeof value === 'boolean'
      ? String(value).toUpperCase()
      : `'${escapeSqlString(displayLogValue(value))}'`;
  const clause = `"${escapeSqlIdentifier(field)}" ${mode === 'exclude' ? '!=' : '='} ${sqlValue}`;
  return addSqlCondition(base, 'WHERE', clause);
}

/** The SQL form of asking for the records whose field has `text` somewhere in it. */
export function appendSqlFieldContainsFilter(statement: string, stream: string, field: string, text: string): string {
  const base = statement.trim() || defaultLogQuery(stream);
  const clause = `"${escapeSqlIdentifier(field)}" LIKE '%${escapeSqlString(text)}%'`;
  if (base.includes(clause)) return base;

  return addSqlCondition(base, 'WHERE', clause);
}

/**
 * The pinned filters, as they apply to the stream (`pinnedLogFilters`), added
 * to the SQL about to run as `"field" = 'value'` clauses before any ORDER BY /
 * LIMIT. A filter on a field the statement already constrains is left out, and
 * with nothing pinned the statement is as it was.
 */
export function appendPinnedFilters(statement: string, stream: string, pinned: readonly PinnedLogFilter[]): string {
  if (pinned.length === 0) return statement;
  const base = statement.trim() || defaultLogQuery(stream);
  const clauses = pinned
    .filter((filter) => !hasSqlFieldFilter(base, filter.field))
    .map((filter) => `"${escapeSqlIdentifier(filter.field)}" ${filter.operator} '${escapeSqlString(filter.value)}'`);
  if (clauses.length === 0) return base;
  return clauses.reduce((current, clause) => addSqlCondition(current, 'WHERE', clause), base);
}
