import type {
  DashboardElement,
  DashboardGroup,
  DashboardPanel,
  DashboardVariable,
} from './schema';

export type DashboardVariableValues = Record<string, unknown>;

export interface RuntimeDashboardElement {
  key: string;
  element: DashboardElement;
  variables: DashboardVariableValues;
}

const VARIABLE_PATTERN =
  /\$\{([A-Za-z_][A-Za-z0-9_]*)(?::([A-Za-z]+))?\}|\$([A-Za-z_][A-Za-z0-9_]*)/g;

export function interpolateVariables(
  input: string,
  values: DashboardVariableValues,
): string {
  return input.replace(
    VARIABLE_PATTERN,
    (match, bracedName: string | undefined, format: string | undefined, plainName: string | undefined) => {
      const name = bracedName ?? plainName;
      if (!name || !(name in values)) return match;
      return formatVariableValue(values[name], format);
    },
  );
}

export function interpolateRecord(
  value: unknown,
  variables: DashboardVariableValues,
): unknown {
  if (typeof value === 'string') return interpolateVariables(value, variables);
  if (Array.isArray(value)) {
    return value.map((entry) => interpolateRecord(entry, variables));
  }
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [
        key,
        interpolateRecord(entry, variables),
      ]),
    );
  }
  return value;
}

export function initialVariableValues(
  variables: readonly DashboardVariable[],
): DashboardVariableValues {
  return Object.fromEntries(
    variables.map((variable) => [
      variable.name,
      variable.currentValue ??
        variable.defaultValue ??
        variable.options?.find((option) => option.selected)?.value ??
        variable.options?.[0]?.value ??
        '',
    ]),
  );
}

export function variableValuesAsStrings(
  values: DashboardVariableValues,
): Record<string, string> {
  return Object.fromEntries(
    Object.entries(values).map(([key, value]) => [
      key,
      formatVariableValue(value),
    ]),
  );
}

/**
 * Repeated elements are runtime-only. The source element remains the only
 * persisted object; each copy receives a scoped variable value and stable key.
 */
export function expandRepeatedElements(
  elements: readonly DashboardElement[],
  values: DashboardVariableValues,
  columns: number,
): RuntimeDashboardElement[] {
  return elements.flatMap((element) => {
    if (
      (element.kind !== 'panel' && element.kind !== 'group') ||
      !element.repeat
    ) {
      return [{ key: element.id, element, variables: values }];
    }

    const repeatValues = arrayValue(values[element.repeat.variable]);
    if (repeatValues.length === 0) {
      return [{ key: element.id, element, variables: values }];
    }
    return repeatValues.map((value, index) => {
      const scopedVariables = {
        ...values,
        [element.repeat!.variable]: value,
      };
      const repeated = cloneRepeatedElement(element, value, index, columns);
      return {
        key: repeated.id,
        element: repeated,
        variables: scopedVariables,
      };
    });
  });
}

export function variableOptions(
  variable: DashboardVariable,
): Array<{ label: string; value: unknown }> {
  const options = variable.options ?? [];
  if (variable.includeAll) {
    return [
      {
        label: 'All',
        value: options.map((option) => option.value),
      },
      ...options,
    ];
  }
  return options;
}

function cloneRepeatedElement(
  source: DashboardPanel | DashboardGroup,
  value: unknown,
  index: number,
  columns: number,
): DashboardPanel | DashboardGroup {
  const repeat = source.repeat;
  const position = { ...source.gridPos };
  if (repeat?.direction === 'vertical') {
    position.y += index * position.h;
  } else {
    const perRow = Math.max(
      1,
      Math.min(
        repeat?.maxPerRow ?? Math.floor(columns / Math.max(1, position.w)),
        Math.floor(columns / Math.max(1, position.w)),
      ),
    );
    position.x = (position.x + (index % perRow) * position.w) % columns;
    position.y += Math.floor(index / perRow) * position.h;
  }
  const suffix = encodeURIComponent(formatVariableValue(value, 'raw'));
  return {
    ...source,
    id: `${source.id}::repeat::${suffix}`,
    title: interpolateVariables(source.title, {
      [repeat?.variable ?? 'value']: value,
    }),
    gridPos: position,
    repeat: undefined,
  };
}

function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : value === undefined ? [] : [value];
}

function formatVariableValue(value: unknown, format = 'raw'): string {
  const values = arrayValue(value);
  if (format === 'json') return JSON.stringify(value);
  if (format === 'csv') return values.map(stringValue).join(',');
  if (format === 'pipe') return values.map(stringValue).join('|');
  if (format === 'regex') {
    return values.map((entry) => escapeRegex(stringValue(entry))).join('|');
  }
  if (format === 'sqlstring') {
    return values
      .map((entry) => `'${stringValue(entry).replaceAll("'", "''")}'`)
      .join(',');
  }
  return values.map(stringValue).join(',');
}

function stringValue(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return JSON.stringify(value);
}

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
