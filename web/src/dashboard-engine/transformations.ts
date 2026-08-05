import {
  cloneDataFrame,
  dataFrameToObjects,
  dataFrameToRows,
  inferFieldType,
  normalizeFrameLength,
  rowsToDataFrame,
} from './dataframe';
import type {
  DataField,
  DataFrame,
  TransformationConfig,
} from './schema';

export function applyTransformations(
  input: readonly DataFrame[],
  transformations: readonly TransformationConfig[],
): DataFrame[] {
  return transformations
    .filter((transformation) => !transformation.disabled)
    .reduce<DataFrame[]>(
      (frames, transformation) =>
        applyTransformation(frames, transformation),
      input.map(cloneDataFrame),
    );
}

export function applyTransformation(
  frames: readonly DataFrame[],
  transformation: TransformationConfig,
): DataFrame[] {
  const options = transformation.options;
  if (transformation.type === 'filter_fields') {
    return frames.map((frame) => filterFields(frame, options));
  }
  if (transformation.type === 'rename_fields') {
    return frames.map((frame) => renameFields(frame, options));
  }
  if (transformation.type === 'organize_fields') {
    return frames.map((frame) => organizeFields(frame, options));
  }
  if (transformation.type === 'calculate_field') {
    return frames.map((frame) => calculateField(frame, options));
  }
  if (transformation.type === 'reduce') {
    return frames.map((frame) => reduceFrame(frame, options));
  }
  if (transformation.type === 'group_by') {
    return frames.map((frame) => groupFrame(frame, options));
  }
  if (transformation.type === 'sort_by') {
    return frames.map((frame) => sortFrame(frame, options));
  }
  if (transformation.type === 'limit') {
    return frames.map((frame) => limitFrame(frame, options));
  }
  if (transformation.type === 'join') {
    return joinFrames(frames, options);
  }
  if (transformation.type === 'merge') return mergeFrames(frames);
  if (transformation.type === 'labels_to_fields') {
    return frames.map(labelsToFields);
  }
  if (transformation.type === 'rows_to_fields') {
    return frames.map((frame) => rowsToFields(frame, options));
  }
  if (transformation.type === 'time_series_to_table') {
    return framesToStatsTable(frames, options);
  }
  return frames.map(cloneDataFrame);
}

function filterFields(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const include = stringArray(options.include);
  const exclude = stringArray(options.exclude);
  const includeRegex = safeRegex(stringValue(options.includeRegex));
  const excludeRegex = safeRegex(stringValue(options.excludeRegex));
  return {
    ...frame,
    fields: frame.fields.filter((field) => {
      if (include.length > 0 && !include.includes(field.name)) return false;
      if (includeRegex && !includeRegex.test(field.name)) return false;
      if (exclude.includes(field.name)) return false;
      if (excludeRegex?.test(field.name)) return false;
      return true;
    }),
  };
}

function renameFields(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const names = record(options.names ?? options.rename);
  const pattern = safeRegex(stringValue(options.pattern));
  const replacement = stringValue(options.replacement);
  return {
    ...frame,
    fields: frame.fields.map((field) => ({
      ...field,
      name:
        stringValue(names[field.name]) ||
        (pattern ? field.name.replace(pattern, replacement) : field.name),
    })),
  };
}

function organizeFields(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const order = stringArray(options.order);
  const hidden = new Set(stringArray(options.exclude ?? options.hidden));
  const rename = record(options.rename);
  const fields = frame.fields
    .filter((field) => !hidden.has(field.name))
    .map((field) => ({
      ...field,
      name: stringValue(rename[field.name]) || field.name,
    }));
  if (order.length === 0) return { ...frame, fields };
  const rank = new Map(order.map((name, index) => [name, index]));
  return {
    ...frame,
    fields: [...fields].sort(
      (left, right) =>
        (rank.get(left.name) ?? Number.MAX_SAFE_INTEGER) -
        (rank.get(right.name) ?? Number.MAX_SAFE_INTEGER),
    ),
  };
}

function calculateField(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const alias = stringValue(options.alias ?? options.name) || 'calculated';
  const left = frame.fields.find(
    (field) => field.name === stringValue(options.left),
  );
  const right = frame.fields.find(
    (field) => field.name === stringValue(options.right),
  );
  const operation = stringValue(options.operation) || 'sum';
  const expression = stringValue(options.expression);
  const values = Array.from({ length: frame.length }, (_, index) => {
    if (expression) {
      return evaluateArithmeticExpression(expression, frame, index);
    }
    return applyBinaryOperation(
      numeric(left?.values[index]),
      numeric(right?.values[index] ?? options.value),
      operation,
    );
  });
  return normalizeFrameLength({
    ...frame,
    fields: [
      ...frame.fields,
      {
        id: `${frame.refId}-calculated-${alias}`,
        name: alias,
        type: 'number',
        values,
      },
    ],
  });
}

function reduceFrame(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const reducers = stringArray(options.reducers);
  const selectedReducers =
    reducers.length > 0
      ? reducers
      : [stringValue(options.reducer) || 'last_not_null'];
  const fields: DataField[] = [];
  for (const field of frame.fields) {
    if (field.type !== 'number') continue;
    for (const reducer of selectedReducers) {
      fields.push({
        id: `${field.id}-${reducer}`,
        name:
          selectedReducers.length === 1
            ? field.name
            : `${field.name} (${reducer})`,
        type: 'number',
        values: [reduceValues(field.values, reducer)],
        labels: field.labels,
      });
    }
  }
  return {
    ...frame,
    name: frame.name ? `${frame.name} summary` : 'Summary',
    length: fields.length > 0 ? 1 : 0,
    fields,
  };
}

function groupFrame(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const groupNames = stringArray(options.fields ?? options.groupBy);
  if (groupNames.length === 0) return cloneDataFrame(frame);
  const reducer = stringValue(options.reducer) || 'sum';
  const rows = dataFrameToObjects(frame);
  const groups = new Map<string, Array<Record<string, unknown>>>();
  for (const row of rows) {
    const key = JSON.stringify(groupNames.map((name) => row[name]));
    const group = groups.get(key) ?? [];
    group.push(row);
    groups.set(key, group);
  }
  const numericFields = frame.fields.filter((field) => field.type === 'number');
  const columns = [...groupNames, ...numericFields.map((field) => field.name)];
  const groupedRows = [...groups.values()].map((group) => [
    ...groupNames.map((name) => group[0]?.[name]),
    ...numericFields.map((field) =>
      reduceValues(
        group.map((row) => row[field.name]),
        reducer,
      ),
    ),
  ]);
  return rowsToDataFrame(columns, groupedRows, {
    refId: frame.refId,
    name: frame.name,
    sourceType: frame.meta?.sourceType,
  });
}

function sortFrame(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const name = stringValue(options.field);
  const fieldIndex = frame.fields.findIndex((field) => field.name === name);
  if (fieldIndex < 0) return cloneDataFrame(frame);
  const direction = options.direction === 'desc' ? -1 : 1;
  const rows = dataFrameToRows(frame).sort((left, right) => {
    const a = left[fieldIndex];
    const b = right[fieldIndex];
    if (a === b) return 0;
    if (a === null || a === undefined) return 1;
    if (b === null || b === undefined) return -1;
    return (
      direction *
      (typeof a === 'number' && typeof b === 'number'
        ? a - b
        : String(a).localeCompare(String(b)))
    );
  });
  return rowsToDataFrame(
    frame.fields.map((field) => field.name),
    rows,
    {
      refId: frame.refId,
      name: frame.name,
      sourceType: frame.meta?.sourceType,
    },
  );
}

function limitFrame(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const count = Math.max(0, integer(options.count, 10));
  const offset = Math.max(0, integer(options.offset, 0));
  return {
    ...frame,
    length: Math.min(count, Math.max(0, frame.length - offset)),
    fields: frame.fields.map((field) => ({
      ...field,
      values: field.values.slice(offset, offset + count),
    })),
  };
}

function joinFrames(
  frames: readonly DataFrame[],
  options: Record<string, unknown>,
): DataFrame[] {
  if (frames.length < 2) return frames.map(cloneDataFrame);
  const on = stringValue(options.field ?? options.on);
  if (!on) return mergeFrames(frames);
  const mode = options.mode === 'inner' ? 'inner' : 'outer';
  const rowsByKey = new Map<string, Record<string, unknown>>();
  const seenByFrame = new Map<string, Set<number>>();
  frames.forEach((frame, frameIndex) => {
    for (const row of dataFrameToObjects(frame)) {
      const key = stableKey(row[on]);
      const previous = rowsByKey.get(key) ?? { [on]: row[on] };
      const next = { ...previous };
      for (const [name, value] of Object.entries(row)) {
        if (name === on) continue;
        next[name in next ? `${name} (${frame.refId})` : name] = value;
      }
      rowsByKey.set(key, next);
      const seen = seenByFrame.get(key) ?? new Set<number>();
      seen.add(frameIndex);
      seenByFrame.set(key, seen);
    }
  });
  const objects = [...rowsByKey.entries()]
    .filter(
      ([key]) => mode === 'outer' || seenByFrame.get(key)?.size === frames.length,
    )
    .map(([, row]) => row);
  return [objectsToFrame(objects, 'joined')];
}

function mergeFrames(frames: readonly DataFrame[]): DataFrame[] {
  if (frames.length === 0) return [];
  const columns = unique(
    frames.flatMap((frame) => frame.fields.map((field) => field.name)),
  );
  const rows = frames.flatMap((frame) =>
    dataFrameToObjects(frame).map((row) =>
      columns.map((column) => row[column] ?? null),
    ),
  );
  return [
    rowsToDataFrame(columns, rows, {
      refId: frames.map((frame) => frame.refId).join('+'),
      name: 'Merged',
      sourceType: frames[0]?.meta?.sourceType,
    }),
  ];
}

function labelsToFields(frame: DataFrame): DataFrame {
  const labels = Object.fromEntries(
    frame.fields.flatMap((field) => Object.entries(field.labels ?? {})),
  );
  if (Object.keys(labels).length === 0) return cloneDataFrame(frame);
  return {
    ...frame,
    fields: [
      ...Object.entries(labels).map<DataField>(([name, value]) => ({
        id: `${frame.refId}-label-${name}`,
        name,
        type: 'string',
        values: Array.from({ length: frame.length }, () => value),
      })),
      ...frame.fields,
    ],
  };
}

function rowsToFields(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataFrame {
  const nameField =
    stringValue(options.nameField) || frame.fields[0]?.name || 'name';
  const valueField =
    stringValue(options.valueField) || frame.fields[1]?.name || 'value';
  const objects = dataFrameToObjects(frame);
  const fields = objects.map<DataField>((row, index) => {
    const name = stringValue(row[nameField]) || `Field ${index + 1}`;
    const value = row[valueField];
    return {
      id: `${frame.refId}-pivot-${index}-${name}`,
      name,
      type: inferFieldType(name, [value]),
      values: [value],
    };
  });
  return { ...frame, length: fields.length > 0 ? 1 : 0, fields };
}

function framesToStatsTable(
  frames: readonly DataFrame[],
  options: Record<string, unknown>,
): DataFrame[] {
  const reducers = stringArray(options.reducers);
  const selected = reducers.length > 0 ? reducers : ['last', 'min', 'max', 'mean'];
  const rows: unknown[][] = [];
  for (const frame of frames) {
    for (const field of frame.fields) {
      if (field.type !== 'number') continue;
      rows.push([
        frame.name ?? field.name,
        ...selected.map((reducer) => reduceValues(field.values, reducer)),
      ]);
    }
  }
  return [
    rowsToDataFrame(['series', ...selected], rows, {
      refId: 'transformed',
      name: 'Time series summary',
    }),
  ];
}

function objectsToFrame(
  objects: Array<Record<string, unknown>>,
  refId: string,
): DataFrame {
  const columns = unique(objects.flatMap((row) => Object.keys(row)));
  return rowsToDataFrame(
    columns,
    objects.map((row) => columns.map((column) => row[column] ?? null)),
    { refId },
  );
}

function reduceValues(values: readonly unknown[], reducer: string): number | null {
  const numbers = values
    .map(numeric)
    .filter((value): value is number => value !== null);
  if (reducer === 'count') return values.length;
  if (numbers.length === 0) return null;
  if (reducer === 'min') return Math.min(...numbers);
  if (reducer === 'max') return Math.max(...numbers);
  if (reducer === 'mean' || reducer === 'avg') {
    return numbers.reduce((sum, value) => sum + value, 0) / numbers.length;
  }
  if (reducer === 'sum') {
    return numbers.reduce((sum, value) => sum + value, 0);
  }
  if (reducer === 'first' || reducer === 'first_not_null') return numbers[0]!;
  return numbers.at(-1) ?? null;
}

function evaluateArithmeticExpression(
  expression: string,
  frame: DataFrame,
  rowIndex: number,
): number | null {
  const values = Object.fromEntries(
    frame.fields.map((field) => [
      field.name,
      numeric(field.values[rowIndex]) ?? 0,
    ]),
  );
  const tokens = tokenize(expression);
  if (!tokens) return null;
  const output: Array<number | string> = [];
  const operators: string[] = [];
  const precedence: Record<string, number> = { '+': 1, '-': 1, '*': 2, '/': 2 };
  for (const token of tokens) {
    if (/^\d/.test(token)) output.push(Number(token));
    else if (/^[A-Za-z_]/.test(token)) output.push(values[token] ?? 0);
    else if (token === '(') operators.push(token);
    else if (token === ')') {
      while (operators.length > 0 && operators.at(-1) !== '(') {
        output.push(operators.pop()!);
      }
      if (operators.at(-1) === '(') operators.pop();
    } else {
      while (
        operators.length > 0 &&
        (precedence[operators.at(-1)!] ?? 0) >= (precedence[token] ?? 0)
      ) {
        output.push(operators.pop()!);
      }
      operators.push(token);
    }
  }
  output.push(...operators.reverse());
  const stack: number[] = [];
  for (const token of output) {
    if (typeof token === 'number') stack.push(token);
    else {
      const right = stack.pop() ?? 0;
      const left = stack.pop() ?? 0;
      stack.push(applyBinaryOperation(left, right, token) ?? 0);
    }
  }
  const value = stack[0];
  return value !== undefined && Number.isFinite(value) ? value : null;
}

function tokenize(expression: string): string[] | null {
  const normalized = expression.replace(
    /\$\{([A-Za-z_][A-Za-z0-9_]*)\}/g,
    '$1',
  );
  if (/[^A-Za-z0-9_+\-*/().\s]/.test(normalized)) return null;
  return normalized.match(/[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?|[()+\-*/]/g);
}

function applyBinaryOperation(
  left: number | null,
  right: number | null,
  operation: string,
): number | null {
  if (left === null || right === null) return null;
  if (operation === 'subtract' || operation === '-') return left - right;
  if (operation === 'multiply' || operation === '*') return left * right;
  if (operation === 'divide' || operation === '/') {
    return right === 0 ? null : left / right;
  }
  return left + right;
}

function numeric(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : [];
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function integer(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.round(value)
    : fallback;
}

function safeRegex(value: string): RegExp | null {
  if (!value) return null;
  try {
    return new RegExp(value);
  } catch {
    return null;
  }
}

function stableKey(value: unknown): string {
  return `${typeof value}:${JSON.stringify(value)}`;
}

function unique(values: readonly string[]): string[] {
  return [...new Set(values)];
}
