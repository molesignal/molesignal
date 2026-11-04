import { http } from '@/lib/http';

import type { NotifyCategory, NotifyChannel } from './index';

/** A template a provider approved (Alibaba Cloud sends no other): its code, and what each of its
 * variables is filled with, written like the body with placeholders. */
export interface ProviderTemplate {
  code: string;
  params: Record<string, string>;
  /** Longer values are cut to this many characters; none is the provider's own limit. */
  maxParamChars?: number | null;
}

/** The colour of a card's title: by the alert's severity, or one chosen. */
export type NotifyCardColor = 'severity' | 'blue' | 'green' | 'yellow' | 'orange' | 'red' | 'grey';

/** The card IM apps and group bots send for an instant messaging template; its body is the
 * template's body. A field or link whose placeholders an event cannot fill is left out. */
export interface NotifyCard {
  /** Empty is the notification's own title. */
  title: string;
  color: NotifyCardColor;
  fields: Array<{ label: string; value: string }>;
  links: Array<{ label: string; url: string }>;
  note: string;
}

export interface NotifyTemplate {
  id: string;
  organizationId?: string;
  name: string;
  body: string;
  /** `json` only for webhook templates: values are escaped as JSON string content. */
  format?: 'text' | 'markdown' | 'html' | 'json';
  category: NotifyCategory;
  /** The channel the template is for; null is a general template, for every channel. */
  channel: NotifyChannel | null;
  /** By provider name; only SMS and phone templates have them. */
  providerTemplates: Record<string, ProviderTemplate>;
  /** The card IM apps and group bots send; only instant messaging templates have one. */
  card: NotifyCard | null;
  createdAt?: number;
  updatedAt?: number;
}

interface NotifyTemplateWire
  extends Omit<
    NotifyTemplate,
    'category' | 'channel' | 'providerTemplates' | 'card'
  > {
  category?: NotifyCategory;
  channel?: NotifyChannel | null;
  providerTemplates?: Record<string, ProviderTemplate>;
  card?: NotifyCard | null;
}

function normalize(row: NotifyTemplateWire): NotifyTemplate {
  const {
    category,
    channel,
    providerTemplates: provider_templates,
    card,
    ...rest
  } = row;
  const out: NotifyTemplate = {
    ...rest,
    category: category ?? 'alert',
    channel: channel ?? null,
    providerTemplates: provider_templates ?? {},
    card: card ?? null,
  };
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
  'name' | 'body' | 'format' | 'category' | 'channel' | 'providerTemplates' | 'card'
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
  | 'escalation'
  | 'schedule'
  | 'oncall'
  | 'override';

export interface NotifyTemplateField {
  key: string;
  token: string;
  group: NotifyTemplateFieldGroup;
  categories: NotifyCategory[];
  example: string;
  eventTypes: string[];
}

export interface NotifyTemplatePreset {
  key: string;
  category: NotifyCategory;
  /** The channel the preset is written for; null for a general template. */
  channel: NotifyChannel | null;
  eventType: string;
  format: NonNullable<NotifyTemplate['format']>;
  body: string;
}

export interface NotifyTemplateFields {
  fields: NotifyTemplateField[];
  presets: NotifyTemplatePreset[];
  labelKeys: string[];
  annotationKeys: string[];
}

/** The variables and presets of templates; the presets are in the reader's interface language. */
export async function listFields(): Promise<NotifyTemplateFields> {
  const { data } = await http.get<NotifyTemplateFields>(
    '/notify/template-fields',
  );
  return data;
}
