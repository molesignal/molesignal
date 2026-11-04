import { FILTER_TESTS, type FilterTest, type ProcessorConfig } from './catalog';

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
    case 'rename_regex':
      return TEXT(raw.within) && TEXT(raw.pattern) && TEXT(raw.replacement)
        ? { op: 'rename_regex', within: raw.within, pattern: raw.pattern, replacement: raw.replacement }
        : null;
    case 'remove':
      return Array.isArray(raw.fields) && raw.fields.every(TEXT)
        ? { op: 'remove', fields: raw.fields }
        : null;
    case 'keep':
      return TEXT(raw.within) && Array.isArray(raw.fields) && raw.fields.every(TEXT)
        ? { op: 'keep', within: raw.within, fields: raw.fields }
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
    case 'lookup':
      return TEXT(raw.table) &&
        TEXT(raw.key) &&
        Array.isArray(raw.fields) &&
        raw.fields.every((item) => {
          const mapping = item as Record<string, unknown> | null;
          return typeof mapping === 'object' && mapping !== null && TEXT(mapping.column) && TEXT(mapping.field);
        })
        ? {
            op: 'lookup',
            table: raw.table,
            key: raw.key,
            fields: (raw.fields as Array<Record<string, string>>).map((item) => ({
              column: item.column ?? '',
              field: item.field ?? '',
            })),
          }
        : null;
    default:
      return null;
  }
}
