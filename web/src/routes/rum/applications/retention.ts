import type { RetentionCondition, RetentionFilter } from '@/api/rum';

/** Most filters an application has, as the server keeps them. */
export const MAX_RETENTION_FILTERS = 10;
/** Most conditions of a filter, and most values of a condition. */
export const MAX_RETENTION_CONDITIONS = 5;
export const MAX_RETENTION_VALUES = 20;

/** Longest a filter name and a value are, in characters. */
export const MAX_FILTER_NAME = 64;
export const MAX_FILTER_VALUE = 128;

/** What a typed list of values is made of: the words between commas (also the Chinese ones), semicolons and lines. */
const SEPARATORS = /[,，、;；\n]+/;

/** The values a text lists: trimmed, each once whatever its case, none empty. */
export function parseValues(text: string): string[] {
  const seen = new Set<string>();

  return text
    .split(SEPARATORS)
    .map((value) => value.trim())
    .filter((value) => {
      const key = value.toLowerCase();

      if (value === '' || seen.has(key)) return false;

      seen.add(key);

      return true;
    });
}

export function sameValues(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

/** A condition on the environment, with nothing yet to compare it with. */
export function newCondition(): RetentionCondition {
  return { field: 'env', op: 'is', values: [] };
}

/** A filter that keeps every session it describes, named so that no other filter has its name. */
export function newFilter(taken: readonly string[], named: (number: number) => string): RetentionFilter {
  const used = new Set(taken.map((name) => name.trim().toLowerCase()));
  let number = taken.length + 1;

  while (used.has(named(number).toLowerCase())) number += 1;

  return { id: '', name: named(number), rate: 100, conditions: [newCondition()] };
}

function validCondition(condition: RetentionCondition): boolean {
  return (
    condition.values.length >= 1 &&
    condition.values.length <= MAX_RETENTION_VALUES &&
    condition.values.every((value) => value.trim() !== '' && value.length <= MAX_FILTER_VALUE)
  );
}

/** Whether a filter is one the server takes, whatever the others are. */
export function validFilter(filter: RetentionFilter): boolean {
  const name = filter.name.trim();

  return (
    name !== '' &&
    name.length <= MAX_FILTER_NAME &&
    Number.isInteger(filter.rate) &&
    filter.rate >= 0 &&
    filter.rate <= 100 &&
    filter.conditions.length <= MAX_RETENTION_CONDITIONS &&
    filter.conditions.every(validCondition)
  );
}

/** Whether the names of filters are told apart: a name is used once, whatever its case. */
export function distinctNames(filters: readonly RetentionFilter[]): boolean {
  const names = filters.map((filter) => filter.name.trim().toLowerCase());

  return new Set(names).size === names.length;
}

/** Whether a list of filters is one the server takes. */
export function validFilters(filters: readonly RetentionFilter[]): boolean {
  return filters.length <= MAX_RETENTION_FILTERS && filters.every(validFilter) && distinctNames(filters);
}

/** Whether two lists are the same, in the same order. */
export function sameFilters(left: readonly RetentionFilter[], right: readonly RetentionFilter[]): boolean {
  return (
    left.length === right.length &&
    left.every((filter, index) => {
      const other = right[index];

      return (
        other !== undefined &&
        filter.id === other.id &&
        filter.name === other.name &&
        filter.rate === other.rate &&
        filter.conditions.length === other.conditions.length &&
        filter.conditions.every((condition, at) => {
          const match = other.conditions[at];

          return (
            match !== undefined &&
            condition.field === match.field &&
            condition.op === match.op &&
            sameValues(condition.values, match.values)
          );
        })
      );
    })
  );
}
