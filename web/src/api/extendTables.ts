import { http } from '@/lib/http';

export type ExtendFieldType = 'string' | 'number' | 'boolean' | 'object';

export interface ExtendValueField {
  name: string;
  field_type: ExtendFieldType;
  required: boolean;
  description: string;
}

export interface ExtendTableUsage {
  kind: 'pipeline' | string;
  id: string;
  name: string;
}

export interface ExtendTableSummary {
  table_name: string;
  description: string;
  key_field: string;
  value_fields: ExtendValueField[];
  row_count: number;
  updated_at: number;
  usage_locations: ExtendTableUsage[];
}

export interface ExtendRow {
  id: string;
  org_id: string;
  table_name: string;
  key: string;
  value_json: unknown;
  updated_at_micros: number;
}

export interface CreateExtendTableInput {
  table_name: string;
  description: string;
  key_field: string;
  value_fields: ExtendValueField[];
}

export async function listTables(): Promise<ExtendTableSummary[]> {
  const { data } = await http.get<ExtendTableSummary[]>('/extend_tables');
  return data;
}

export async function createTable(
  payload: CreateExtendTableInput,
): Promise<ExtendTableSummary> {
  const { data } = await http.post<ExtendTableSummary>('/extend_tables', payload);
  return data;
}

export async function deleteTable(table: string): Promise<void> {
  await http.delete(`/extend_tables/${encodeURIComponent(table)}`);
}

export async function listRows(table: string): Promise<ExtendRow[]> {
  const { data } = await http.get<ExtendRow[]>(`/extend_tables/${encodeURIComponent(table)}`);
  return data;
}

export async function upsert(table: string, key: string, value_json: unknown): Promise<void> {
  await http.put(
    `/extend_tables/${encodeURIComponent(table)}/rows/${encodeURIComponent(key)}`,
    { value_json },
  );
}

export async function remove(table: string, key: string): Promise<void> {
  await http.delete(
    `/extend_tables/${encodeURIComponent(table)}/rows/${encodeURIComponent(key)}`,
  );
}
