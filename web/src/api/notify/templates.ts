import { http } from '@/lib/http';

import type { NotifyCategory } from './index';

export interface NotifyTemplate {
  id: string;
  organization_id?: string;
  name: string;
  body: string;
  format?: 'text' | 'markdown' | 'html';
  category: NotifyCategory;
  created_at_micros?: number;
  updated_at_micros?: number;
}

interface NotifyTemplateWire
  extends Omit<
    NotifyTemplate,
    'category' | 'created_at_micros' | 'updated_at_micros'
  > {
  category?: NotifyCategory;
  created_at?: number;
  updated_at?: number;
  created_at_micros?: number;
  updated_at_micros?: number;
}

function normalize(row: NotifyTemplateWire): NotifyTemplate {
  const {
    category,
    created_at,
    updated_at,
    created_at_micros,
    updated_at_micros,
    ...rest
  } = row;
  const out: NotifyTemplate = { ...rest, category: category ?? 'alert' };
  const created = created_at_micros ?? created_at;
  const updated = updated_at_micros ?? updated_at;
  if (created !== undefined) out.created_at_micros = created;
  if (updated !== undefined) out.updated_at_micros = updated;
  return out;
}

export async function list(): Promise<NotifyTemplate[]> {
  const { data } = await http.get<NotifyTemplateWire[]>('/notify/templates');
  return data.map(normalize);
}

export async function get(id: string): Promise<NotifyTemplate> {
  const { data } = await http.get<NotifyTemplateWire>(
    `/notify/templates/${encodeURIComponent(id)}`,
  );
  return normalize(data);
}

export type NotifyTemplateInput = Pick<
  NotifyTemplate,
  'name' | 'body' | 'format' | 'category'
>;

export async function create(payload: NotifyTemplateInput): Promise<NotifyTemplate> {
  const { data } = await http.post<NotifyTemplateWire>(
    '/notify/templates',
    payload,
  );
  return normalize(data);
}

export async function update(id: string, payload: NotifyTemplateInput): Promise<NotifyTemplate> {
  const { data } = await http.put<NotifyTemplateWire>(
    `/notify/templates/${encodeURIComponent(id)}`,
    payload,
  );
  return normalize(data);
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/notify/templates/${encodeURIComponent(id)}`);
}

export type NotifyTemplateFieldGroup =
  | 'event'
  | 'message'
  | 'rule'
  | 'incident'
  | 'trigger'
  | 'labels'
  | 'annotations'
  | 'schedule'
  | 'oncall'
  | 'override';

export interface NotifyTemplateField {
  key: string;
  token: string;
  group: NotifyTemplateFieldGroup;
  categories: NotifyCategory[];
  example: string;
  event_types: string[];
}

export interface NotifyTemplatePreset {
  key: string;
  category: NotifyCategory;
  event_type: string;
  format: NonNullable<NotifyTemplate['format']>;
  body: string;
}

export interface NotifyTemplateFields {
  fields: NotifyTemplateField[];
  presets: NotifyTemplatePreset[];
  label_keys: string[];
  annotation_keys: string[];
}

export async function listFields(): Promise<NotifyTemplateFields> {
  const { data } = await http.get<NotifyTemplateFields>(
    '/notify/template-fields',
  );
  return data;
}
