import { onField, vrlPath, vrlReadPaths, vrlRegex, vrlString } from './vrl';

/**
 * Processing a step can do without anyone writing code. Each one is a small
 * form that is turned into VRL, which is what the backend stores and runs; the
 * form is kept beside the script so the step opens as a form again.
 *
 * Nothing here can fail an event: a field that is missing or of another type
 * is left as it was. A filter or a sample drops events on purpose, with VRL's
 * `abort`: a run counts them as dropped, not as failed.
 */
export type ProcessorConfig =
  | { op: 'parse_json'; source: string; target: string; removeSource: boolean }
  | { op: 'parse_key_value'; source: string; target: string; removeSource: boolean }
  | { op: 'parse_regex'; source: string; pattern: string }
  | { op: 'rename'; from: string; to: string }
  | { op: 'remove'; fields: string[] }
  | { op: 'set'; field: string; valueType: SetValueType; value: string }
  | { op: 'mask'; field: string; mode: MaskMode }
  | { op: 'case'; field: string; to: 'lower' | 'upper' }
  | { op: 'convert'; field: string; to: ConvertTarget }
  | { op: 'filter'; action: FilterAction; field: string; test: FilterTest; value: string }
  | { op: 'sample'; percent: string; by: string };

export type ProcessorOp = ProcessorConfig['op'];
export type SetValueType = 'string' | 'number' | 'boolean' | 'field';
export type MaskMode = 'full' | 'last4' | 'hash';
export type ConvertTarget = 'integer' | 'float' | 'boolean' | 'string';
/** What a filter does with the events its test matches. */
export type FilterAction = 'keep' | 'drop';
export type FilterTest =
  | 'equals'
  | 'not_equals'
  | 'contains'
  | 'starts_with'
  | 'matches'
  | 'exists'
  | 'missing'
  | 'gt'
  | 'gte'
  | 'lt'
  | 'lte';

export const FILTER_TESTS: readonly FilterTest[] = [
  'equals',
  'not_equals',
  'contains',
  'starts_with',
  'matches',
  'exists',
  'missing',
  'gt',
  'gte',
  'lt',
  'lte',
];

/** The tests that look at the field only, not at a value. */
export function filterTestTakesValue(test: FilterTest): boolean {
  return test !== 'exists' && test !== 'missing';
}

const NUMERIC_TESTS: Partial<Record<FilterTest, string>> = { gt: '>', gte: '>=', lt: '<', lte: '<=' };

/**
 * The processors in the order they are offered: reading structure out of text,
 * shaping fields, then choosing which events go on at all.
 */
export const PROCESSOR_GROUPS: ReadonlyArray<{
  group: 'parse' | 'fields' | 'events';
  ops: readonly ProcessorOp[];
}> = [
  { group: 'parse', ops: ['parse_json', 'parse_key_value', 'parse_regex'] },
  { group: 'fields', ops: ['rename', 'remove', 'set', 'mask', 'case', 'convert'] },
  { group: 'events', ops: ['filter', 'sample'] },
];

export const PROCESSOR_OPS: readonly ProcessorOp[] = PROCESSOR_GROUPS.flatMap((group) => group.ops);

export function defaultProcessor(op: ProcessorOp): ProcessorConfig {
  switch (op) {
    case 'parse_json':
    case 'parse_key_value':
      return { op, source: 'message', target: '', removeSource: false };
    case 'parse_regex':
      return { op, source: 'message', pattern: '' };
    case 'rename':
      return { op, from: '', to: '' };
    case 'remove':
      return { op, fields: [] };
    case 'set':
      return { op, field: '', valueType: 'string', value: '' };
    case 'mask':
      return { op, field: '', mode: 'full' };
    case 'case':
      return { op, field: '', to: 'lower' };
    case 'convert':
      return { op, field: '', to: 'integer' };
    case 'filter':
      return { op, action: 'keep', field: '', test: 'equals', value: '' };
    case 'sample':
      return { op, percent: '10', by: '' };
  }
}

/** The name a new step of this kind starts with. */
export function processorStepName(op: ProcessorOp): string {
  return op.replace(/_/g, '-');
}

/** What a form still lacks before it says something, as a validation code; `null` when complete. */
export function processorProblem(config: ProcessorConfig): string | null {
  const blank = (value: string) => value.trim() === '';

  switch (config.op) {
    case 'parse_json':
    case 'parse_key_value':
      return blank(config.source) ? 'processor_field_missing' : null;
    case 'parse_regex':
      if (blank(config.source)) return 'processor_field_missing';
      return blank(config.pattern) ? 'processor_pattern_missing' : null;
    case 'rename':
      return blank(config.from) || blank(config.to) ? 'processor_field_missing' : null;
    case 'remove':
      return config.fields.every(blank) ? 'processor_field_missing' : null;
    case 'set':
      if (blank(config.field)) return 'processor_field_missing';
      if (config.valueType === 'number') {
        if (blank(config.value)) return 'processor_field_missing';
        return numberLiteral(config.value) ? null : 'processor_number_invalid';
      }
      if (config.valueType === 'field') return blank(config.value) ? 'processor_field_missing' : null;
      return null;
    case 'mask':
    case 'case':
    case 'convert':
      return blank(config.field) ? 'processor_field_missing' : null;
    case 'filter':
      if (blank(config.field)) return 'processor_field_missing';
      if (!filterTestTakesValue(config.test)) return null;
      if (config.test === 'matches') return blank(config.value) ? 'processor_pattern_missing' : null;
      if (blank(config.value)) return 'processor_field_missing';
      return NUMERIC_TESTS[config.test] && !numberLiteral(config.value) ? 'processor_number_invalid' : null;
    case 'sample':
      return percentOf(config.percent) === null ? 'processor_percent_invalid' : null;
  }
}

/** The VRL a processor stands for. */
export function compileProcessor(config: ProcessorConfig): string {
  return lines(config).join('\n');
}

function lines(config: ProcessorConfig): string[] {
  switch (config.op) {
    case 'parse_json':
      return parseInto(config, (path) => `parse_json(${path})`, 'any');
    case 'parse_key_value':
      return parseInto(config, (path) => `parse_key_value(${path})`, 'object');
    case 'parse_regex':
      return parseInto(
        { ...config, target: '', removeSource: false },
        (path) => `parse_regex(${path}, ${vrlRegex(config.pattern)})`,
        'object',
      );
    case 'rename':
      return onField(config.from, 'exists', (path) => [`${vrlPath(config.to)} = del(${path})`]);
    case 'remove':
      return config.fields
        .filter((field) => field.trim() !== '')
        .flatMap((field) => vrlReadPaths(field).map((path) => `del(${path})`));
    case 'set':
      return setLines(config);
    case 'mask':
      return maskLines(config);
    case 'case':
      return onField(config.field, 'is_string', (path) => [
        `${path} = ${config.to === 'lower' ? 'downcase' : 'upcase'}(string!(${path}))`,
      ]);
    case 'convert':
      // Guarded: VRL converts a missing or null value too (to 0, false, ""), which would invent the field.
      return onField(config.field, 'exists', (path) => [
        `converted, err = ${CONVERTERS[config.to]}(${path})`,
        `if err == null && ${path} != null {`,
        `  ${path} = converted`,
        '}',
      ]);
    case 'filter':
      return [...filterTest(config), `if ${config.action === 'keep' ? '!matched' : 'matched'} {`, '  abort', '}'];
    case 'sample':
      return sampleLines(config);
  }
}

/**
 * The field's value as `value`: under the first of its read paths that is
 * there, `null` when none is.
 */
function readValue(field: string): string[] {
  const paths = vrlReadPaths(field);
  const [only] = paths;

  if (paths.length === 1 && only !== undefined) return [`value = ${only}`];

  return ['value = null', ...onField(field, 'exists', (path) => [`value = ${path}`])];
}

/** Sets `matched` to whether the event passes the filter's test. */
function filterTest(config: Extract<ProcessorConfig, { op: 'filter' }>): string[] {
  const exists = vrlReadPaths(config.field)
    .map((path) => `exists(${path})`)
    .join(' || ');
  const text = vrlString(config.value);
  const numeric = NUMERIC_TESTS[config.test];

  switch (config.test) {
    case 'exists':
      return [`matched = ${exists}`];
    case 'missing':
      return [`matched = !(${exists})`];
    case 'equals':
      return [...readValue(config.field), `matched = ${equality(config.value)}`];
    case 'not_equals':
      return [...readValue(config.field), `matched = !(${equality(config.value)})`];
    case 'contains':
      return [...readValue(config.field), `matched = is_string(value) && contains(string!(value), ${text})`];
    case 'starts_with':
      return [...readValue(config.field), `matched = is_string(value) && starts_with(string!(value), ${text})`];
    case 'matches':
      return [
        ...readValue(config.field),
        `matched = is_string(value) && match(string!(value), ${vrlRegex(config.value)})`,
      ];
    default:
      // A number, or text that reads as one; anything else does not match.
      return [
        ...readValue(config.field),
        'matched = false',
        'if value != null {',
        '  number, err = to_float(value)',
        `  matched = err == null && number ${numeric ?? '=='} ${floatLiteral(config.value)}`,
        '}',
      ];
  }
}

/**
 * The field equals the value as typed: `200` matches the number and the text,
 * `true` the boolean and the text.
 */
function equality(value: string): string {
  const number = numberLiteral(value);
  const literal = number ?? (value === 'true' || value === 'false' ? value : null);

  return literal === null
    ? `value == ${vrlString(value)}`
    : `value == ${literal} || value == ${vrlString(value)}`;
}

/**
 * Keeps the share of events the sample says. By a field, the choice follows
 * its value: every event of one trace is kept or none is. An event without the
 * field is kept or not at random, as without one.
 */
function sampleLines(config: Extract<ProcessorConfig, { op: 'sample' }>): string[] {
  const percent = percentOf(config.percent) ?? 100;
  const random = `random_float(0.0, 100.0) < ${floatLiteral(String(percent))}`;

  if (config.by.trim() === '') return [`if !(${random}) {`, '  abort', '}'];

  return [
    ...readValue(config.by),
    'key, err = to_string(value)',
    'sampled = false',
    'if value == null || err != null {',
    `  sampled = ${random}`,
    '} else {',
    `  sampled = mod(abs(seahash(key)), 10000) < ${Math.round(percent * 100)}`,
    '}',
    'if !sampled {',
    '  abort',
    '}',
  ];
}

/** A share of events as a percentage above 0 and at most 100, or `null`. */
function percentOf(text: string): number | null {
  const literal = numberLiteral(text);
  const percent = literal === null ? Number.NaN : Number(literal);

  return percent > 0 && percent <= 100 ? percent : null;
}

/** A number as a VRL float: comparing a float with an integer is not allowed. */
function floatLiteral(text: string): string {
  const literal = numberLiteral(text) ?? '0';

  return literal.includes('.') ? literal : `${literal}.0`;
}

const CONVERTERS: Record<ConvertTarget, string> = {
  integer: 'to_int',
  float: 'to_float',
  boolean: 'to_bool',
  string: 'to_string',
};

/**
 * Parses a text field and puts what it held in the event: merged into it, or
 * under `target`. The source is removed before the merge, so a parsed key of
 * the same name survives.
 */
function parseInto(
  config: { source: string; target: string; removeSource: boolean },
  parse: (path: string) => string,
  yields: 'object' | 'any',
): string[] {
  return whereFound(config.source, (path) => {
    const drop = config.removeSource ? [`  del(${path})`] : [];

    if (config.target.trim() !== '') {
      return [
        `parsed, err = ${parse(path)}`,
        'if err == null {',
        ...drop,
        `  ${vrlPath(config.target)} = parsed`,
        '}',
      ];
    }

    return [
      `parsed, err = ${parse(path)}`,
      yields === 'object' ? 'if err == null {' : 'if err == null && is_object(parsed) {',
      ...drop,
      yields === 'object' ? '  . = merge(., parsed)' : '  . = merge(., object!(parsed))',
      '}',
    ];
  });
}

/** `body` on the field's one path, or on whichever of its two is there. */
function whereFound(field: string, body: (path: string) => string[]): string[] {
  const paths = vrlReadPaths(field);
  const [only] = paths;

  return paths.length === 1 && only !== undefined ? body(only) : onField(field, 'exists', body);
}

function setLines(config: Extract<ProcessorConfig, { op: 'set' }>): string[] {
  const target = vrlPath(config.field);

  if (config.valueType === 'field') {
    return onField(config.value, 'exists', (path) => [`${target} = ${path}`]);
  }
  if (config.valueType === 'number') return [`${target} = ${numberLiteral(config.value) ?? '0'}`];
  if (config.valueType === 'boolean') return [`${target} = ${config.value === 'true' ? 'true' : 'false'}`];

  return [`${target} = ${vrlString(config.value)}`];
}

function maskLines(config: Extract<ProcessorConfig, { op: 'mask' }>): string[] {
  if (config.mode === 'full') {
    return onField(config.field, 'exists', (path) => [`${path} = "***"`]);
  }
  if (config.mode === 'hash') {
    return onField(config.field, 'is_string', (path) => [
      `${path} = sha2(string!(${path}), variant: "SHA-256")`,
    ]);
  }

  return onField(config.field, 'is_string', (path) => [
    `value = string!(${path})`,
    `${path} = if length(value) > 4 { "****" + slice!(value, -4) } else { "****" }`,
  ]);
}

/** A number as VRL writes one, or `null` when the text is not a plain number. */
function numberLiteral(value: string): string | null {
  const text = value.trim();
  if (!/^-?\d+(\.\d+)?$/.test(text)) return null;

  return text;
}

/** The fields a processor names, for the line under a step's title. */
export function processorSummary(config: ProcessorConfig): string {
  switch (config.op) {
    case 'parse_json':
    case 'parse_key_value':
      return config.target.trim() ? `${config.source} → ${config.target}` : config.source;
    case 'parse_regex':
      return config.source;
    case 'rename':
      return config.from || config.to ? `${config.from} → ${config.to}` : '';
    case 'remove':
      return config.fields.filter((field) => field.trim() !== '').join(', ');
    case 'set':
      return config.field ? `${config.field} = ${config.value}` : '';
    case 'mask':
    case 'case':
      return config.field;
    case 'convert':
      return config.field ? `${config.field} → ${config.to}` : '';
    case 'filter':
      if (!config.field) return '';
      return filterTestTakesValue(config.test)
        ? `${config.field} ${FILTER_SIGNS[config.test]} ${config.value}`
        : `${config.field} ${FILTER_SIGNS[config.test]}`;
    case 'sample':
      return config.by.trim() ? `${config.percent}% · ${config.by}` : `${config.percent}%`;
  }
}

/** How a filter's test reads under the step's title. */
const FILTER_SIGNS: Record<FilterTest, string> = {
  equals: '=',
  not_equals: '≠',
  contains: '∋',
  starts_with: '^=',
  matches: '=~',
  exists: '∃',
  missing: '∄',
  gt: '>',
  gte: '≥',
  lt: '<',
  lte: '≤',
};

const TEXT = (value: unknown): value is string => typeof value === 'string';

/** A stored processor read back, or `null` for anything that is not one. */
export function parseProcessor(value: unknown): ProcessorConfig | null {
  if (typeof value !== 'object' || value === null) return null;
  const raw = value as Record<string, unknown>;

  switch (raw.op) {
    case 'parse_json':
    case 'parse_key_value':
      return TEXT(raw.source) && TEXT(raw.target) && typeof raw.removeSource === 'boolean'
        ? { op: raw.op, source: raw.source, target: raw.target, removeSource: raw.removeSource }
        : null;
    case 'parse_regex':
      return TEXT(raw.source) && TEXT(raw.pattern)
        ? { op: 'parse_regex', source: raw.source, pattern: raw.pattern }
        : null;
    case 'rename':
      return TEXT(raw.from) && TEXT(raw.to) ? { op: 'rename', from: raw.from, to: raw.to } : null;
    case 'remove':
      return Array.isArray(raw.fields) && raw.fields.every(TEXT)
        ? { op: 'remove', fields: raw.fields }
        : null;
    case 'set':
      return TEXT(raw.field) &&
        TEXT(raw.value) &&
        (raw.valueType === 'string' ||
          raw.valueType === 'number' ||
          raw.valueType === 'boolean' ||
          raw.valueType === 'field')
        ? { op: 'set', field: raw.field, valueType: raw.valueType, value: raw.value }
        : null;
    case 'mask':
      return TEXT(raw.field) && (raw.mode === 'full' || raw.mode === 'last4' || raw.mode === 'hash')
        ? { op: 'mask', field: raw.field, mode: raw.mode }
        : null;
    case 'case':
      return TEXT(raw.field) && (raw.to === 'lower' || raw.to === 'upper')
        ? { op: 'case', field: raw.field, to: raw.to }
        : null;
    case 'convert':
      return TEXT(raw.field) &&
        (raw.to === 'integer' || raw.to === 'float' || raw.to === 'boolean' || raw.to === 'string')
        ? { op: 'convert', field: raw.field, to: raw.to }
        : null;
    case 'filter':
      return (raw.action === 'keep' || raw.action === 'drop') &&
        TEXT(raw.field) &&
        TEXT(raw.value) &&
        FILTER_TESTS.includes(raw.test as FilterTest)
        ? { op: 'filter', action: raw.action, field: raw.field, test: raw.test as FilterTest, value: raw.value }
        : null;
    case 'sample':
      return TEXT(raw.percent) && TEXT(raw.by) ? { op: 'sample', percent: raw.percent, by: raw.by } : null;
    default:
      return null;
  }
}
