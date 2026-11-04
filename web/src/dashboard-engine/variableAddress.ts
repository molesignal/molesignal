import type { DashboardVariable } from './schema';
import type { DashboardVariableValues } from './variables';

const PREFIX = 'var-';

function list(value: unknown): unknown[] {
  if (Array.isArray(value)) return value;
  return value === undefined || value === null || value === '' ? [] : [value];
}

function text(value: unknown): string {
  return typeof value === 'object' && value !== null ? JSON.stringify(value) : String(value);
}

/**
 * The values the address of a dashboard gives its variables: `var-<name>=<value>`, the name once
 * for each value of a list. A name the dashboard does not declare is not a variable of it.
 */
export function variablesInAddress(
  params: URLSearchParams,
  declared: readonly DashboardVariable[],
): DashboardVariableValues {
  const values: DashboardVariableValues = {};

  for (const variable of declared) {
    const given = params.getAll(`${PREFIX}${variable.name}`);

    if (given.length === 0) continue;

    values[variable.name] = variable.multi ? given : given[0];
  }

  return values;
}

/** The address with the variables written in it; nothing else of the address is touched. */
export function addressWithVariables(
  params: URLSearchParams,
  declared: readonly DashboardVariable[],
  values: DashboardVariableValues,
): URLSearchParams {
  const next = new URLSearchParams(params);

  for (const variable of declared) {
    const key = `${PREFIX}${variable.name}`;

    next.delete(key);

    // A constant is the dashboard's own, not a choice.
    if (variable.type === 'constant') continue;

    for (const entry of list(values[variable.name])) next.append(key, text(entry));
  }

  return next;
}

/** Whether two values of a variable are the same, as text or as a list of it. */
export function sameVariableValue(left: unknown, right: unknown): boolean {
  const a = list(left).map(text);
  const b = list(right).map(text);

  return a.length === b.length && a.every((entry, index) => entry === b[index]);
}

/** Whether two sets of values hold the same value under every name. */
export function sameVariableValues(left: DashboardVariableValues, right: DashboardVariableValues): boolean {
  const names = new Set([...Object.keys(left), ...Object.keys(right)]);

  return [...names].every((name) => sameVariableValue(left[name], right[name]));
}
