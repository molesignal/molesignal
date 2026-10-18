import type { McpNamedValue, McpNamedValueInput } from '@/api/agent/mcpServers';

/**
 * The words that make a header or a variable secret, as the backend reads them
 * (`crates/modules/agent/src/mcp/values.rs`): its value is kept encrypted and
 * only its last four characters come back.
 */
const SECRET_PARTS = [
  'key',
  'secret',
  'token',
  'password',
  'passwd',
  'credential',
  'auth',
  'cookie',
  'signature',
];

export function looksSecret(name: string): boolean {
  const lower = name.toLowerCase();
  return SECRET_PARTS.some((part) => lower.includes(part));
}

/** One row of the editor. */
export interface NamedValueRow {
  key: string;
  name: string;
  /** What is typed; for a saved secret, empty keeps the saved value. */
  value: string;
  secret: boolean;
  /** A secret already saved under this name. */
  saved?: { last4?: string | null } | undefined;
}

let rowKeys = 0;
const nextKey = () => {
  rowKeys += 1;
  return `row-${rowKeys}`;
};

export function emptyRow(): NamedValueRow {
  return { key: nextKey(), name: '', value: '', secret: false };
}

export function rowsFrom(values: readonly McpNamedValue[] | undefined): NamedValueRow[] {
  return (values ?? []).map((item) => ({
    key: nextKey(),
    name: item.name,
    value: item.secret ? '' : (item.value ?? ''),
    secret: item.secret,
    ...(item.secret && item.set ? { saved: { last4: item.last4 ?? null } } : {}),
  }));
}

/**
 * The rows as they are saved: rows without a name are left out, and a secret
 * left empty is sent without a value so the saved one stays.
 */
export function inputsFrom(rows: readonly NamedValueRow[]): McpNamedValueInput[] {
  return rows
    .filter((row) => row.name.trim())
    .map((row) => {
      const secret = row.secret || looksSecret(row.name);
      const value = row.value.trim();
      return value ? { name: row.name.trim(), secret, value } : { name: row.name.trim(), secret };
    });
}
