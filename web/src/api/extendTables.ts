import { http } from '@/lib/http';

export type ExtendFieldType = 'string' | 'number' | 'boolean' | 'object';

export interface ExtendValueField {
  name: string;
  fieldType: ExtendFieldType;
  required: boolean;
  description: string;
}

export interface ExtendTableUsage {
  kind: 'pipeline' | string;
  id: string;
  name: string;
}

export interface ExtendTableSummary {
  tableName: string;
  description: string;
  keyField: string;
  valueFields: ExtendValueField[];
  rowCount: number;
  updatedAt: number;
  usageLocations: ExtendTableUsage[];
}

export interface ExtendRow {
  id: string;
  orgId: string;
  tableName: string;
  key: string;
  valueJson: unknown;
  updatedAt: number;
}

export interface CreateExtendTableInput {
  tableName: string;
  description: string;
  keyField: string;
  valueFields: ExtendValueField[];
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

export async function updateTableFields(
  table: string,
  value_fields: ExtendValueField[],
): Promise<ExtendTableSummary> {
  const { data } = await http.patch<ExtendTableSummary>(
    `/extend_tables/${encodeURIComponent(table)}`,
    { valueFields: value_fields },
  );
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
    { valueJson: value_json },
  );
}

export async function remove(table: string, key: string): Promise<void> {
  await http.delete(
    `/extend_tables/${encodeURIComponent(table)}/rows/${encodeURIComponent(key)}`,
  );
}
