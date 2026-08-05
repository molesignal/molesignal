import type {
  ExtendFieldType,
  ExtendRow,
  ExtendValueField,
} from '@/api/extendTables';

export interface ImportRecord {
  key: string;
  value: Record<string, unknown>;
}

export function valueAsObject(value: unknown): Record<string, unknown> {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return { value };
}

export function inferValueFields(rows: ExtendRow[]): ExtendValueField[] {
  const values = new Map<string, unknown[]>();
  for (const row of rows) {
    for (const [name, value] of Object.entries(valueAsObject(row.value_json))) {
      const entries = values.get(name) ?? [];
      entries.push(value);
      values.set(name, entries);
    }
  }
  return [...values.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([name, samples]) => ({
      name,
      field_type: inferFieldType(samples),
      required: rows.length > 0 && samples.length === rows.length,
      description: '',
    }));
}

export function inferFieldType(values: unknown[]): ExtendFieldType {
  const defined = values.find((value) => value !== null && value !== undefined);
  if (typeof defined === 'number') return 'number';
  if (typeof defined === 'boolean') return 'boolean';
  if (defined && typeof defined === 'object') return 'object';
  return 'string';
}

export function displayValue(value: unknown): string {
  if (value === null || value === undefined) return '—';
  if (typeof value === 'string') return value || '—';
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return JSON.stringify(value);
}

export function parseFieldValue(raw: string, type: ExtendFieldType): unknown {
  if (type === 'string') return raw;
  if (type === 'number') {
    const number = Number(raw);
    if (!Number.isFinite(number)) throw new Error('number');
    return number;
  }
  if (type === 'boolean') {
    if (raw === 'true') return true;
    if (raw === 'false') return false;
    throw new Error('boolean');
  }
  return JSON.parse(raw) as unknown;
}

export function parseImportText(text: string, keyField: string): ImportRecord[] {
  const trimmed = text.trim();
  if (!trimmed) return [];
  if (trimmed.startsWith('[') || trimmed.startsWith('{')) {
    return parseJsonImport(trimmed, keyField);
  }
  return parseCsvImport(trimmed, keyField);
}

function parseJsonImport(text: string, keyField: string): ImportRecord[] {
  const parsed = JSON.parse(text) as unknown;
  if (Array.isArray(parsed)) {
    return parsed.map((item, index) => {
      if (!item || typeof item !== 'object' || Array.isArray(item)) {
        throw new Error(`row:${index + 1}`);
      }
      const object = { ...(item as Record<string, unknown>) };
      const key = object[keyField] ?? object.key;
      if (typeof key !== 'string' && typeof key !== 'number') {
        throw new Error(`key:${index + 1}`);
      }
      delete object[keyField];
      if (keyField !== 'key') delete object.key;
      const explicitValue = object.value;
      return {
        key: String(key),
        value:
          explicitValue &&
          typeof explicitValue === 'object' &&
          !Array.isArray(explicitValue) &&
          Object.keys(object).length === 1
            ? (explicitValue as Record<string, unknown>)
            : object,
      };
    });
  }
  if (parsed && typeof parsed === 'object') {
    return Object.entries(parsed as Record<string, unknown>).map(([key, value]) => ({
      key,
      value: valueAsObject(value),
    }));
  }
  throw new Error('root');
}

function parseCsvImport(text: string, keyField: string): ImportRecord[] {
  const lines = text.split(/\r?\n/).filter((line) => line.trim());
  if (lines.length < 2) return [];
  const headers = parseCsvLine(lines[0] ?? '');
  const keyIndex = headers.findIndex((header) => header === keyField || header === 'key');
  if (keyIndex < 0) throw new Error('csv-key');
  return lines.slice(1).map((line, rowIndex) => {
    const cells = parseCsvLine(line);
    const key = cells[keyIndex]?.trim();
    if (!key) throw new Error(`key:${rowIndex + 2}`);
    const value: Record<string, unknown> = {};
    headers.forEach((header, index) => {
      if (index !== keyIndex && header) value[header] = cells[index] ?? '';
    });
    return { key, value };
  });
}

function parseCsvLine(line: string): string[] {
  const cells: string[] = [];
  let current = '';
  let quoted = false;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    if (character === '"') {
      if (quoted && line[index + 1] === '"') {
        current += '"';
        index += 1;
      } else {
        quoted = !quoted;
      }
    } else if (character === ',' && !quoted) {
      cells.push(current.trim());
      current = '';
    } else {
      current += character;
    }
  }
  cells.push(current.trim());
  return cells;
}
