/** Where a field's value leads, when the field names something that has a page of its own. */
export interface FieldJumpTarget {
  to: string;
  /** i18n key: the consumer calls `t(titleKey)`, so the title follows the reader's language. */
  titleKey: string;
}

interface FieldJumpContext {
  record: Record<string, unknown>;
  field: string;
  value: unknown;
}

interface FieldJumpConfig {
  match: (field: string) => boolean;
  resolve: (context: FieldJumpContext) => FieldJumpTarget | null;
}

const TRACE_ID_FIELD_ALIASES = ['trace_id', 'traceid', 'trace.id'];
const SPAN_ID_FIELD_ALIASES = ['span_id', 'spanid', 'span.id'];

function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== '';
}

/** The last part of a dotted or indexed path, in lower case. */
export function leafFieldName(field: string): string {
  const parts = field.toLowerCase().replace(/\[(\d+)\]/g, '.$1').split('.').filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1]! : field.toLowerCase();
}

function matchesFieldAlias(field: string, aliases: string[]): boolean {
  const lower = field.toLowerCase();
  const leaf = leafFieldName(field);
  return aliases.some((alias) => {
    const normalized = alias.toLowerCase();
    return lower === normalized || leaf === normalized || lower.endsWith(`.${normalized}`);
  });
}

function readRecordPath(record: Record<string, unknown>, path: string): unknown {
  if (Object.prototype.hasOwnProperty.call(record, path)) return record[path];
  let current: unknown = record;
  for (const part of path.split('.')) {
    if (!current || typeof current !== 'object' || !Object.prototype.hasOwnProperty.call(current, part)) {
      return undefined;
    }
    current = (current as Record<string, unknown>)[part];
  }
  return current;
}

function findRecordValue(record: Record<string, unknown>, aliases: string[]): unknown {
  for (const alias of aliases) {
    const value = readRecordPath(record, alias);
    if (isPresent(value)) return value;
  }
  for (const [key, value] of Object.entries(record)) {
    if (matchesFieldAlias(key, aliases) && isPresent(value)) return value;
  }
  return undefined;
}

const LOG_FIELD_JUMP_CONFIG: FieldJumpConfig[] = [
  {
    match: (field) => matchesFieldAlias(field, TRACE_ID_FIELD_ALIASES),
    resolve: ({ value }) => {
      if (!isPresent(value)) return null;
      return {
        to: `/traces/${encodeURIComponent(String(value))}`,
        titleKey: 'explore.jump.open_trace',
      };
    },
  },
  {
    match: (field) => matchesFieldAlias(field, SPAN_ID_FIELD_ALIASES),
    resolve: ({ record, value }) => {
      const traceId = findRecordValue(record, TRACE_ID_FIELD_ALIASES);
      if (!isPresent(traceId) || !isPresent(value)) return null;
      return {
        to: `/traces/${encodeURIComponent(String(traceId))}?spanId=${encodeURIComponent(String(value))}`,
        titleKey: 'explore.jump.open_span',
      };
    },
  },
];

export function resolveLogFieldJump(
  record: Record<string, unknown>,
  field: string,
  value: unknown,
): FieldJumpTarget | null {
  for (const config of LOG_FIELD_JUMP_CONFIG) {
    if (!config.match(field)) continue;
    const target = config.resolve({ record, field, value });
    if (target) return target;
  }
  return null;
}
