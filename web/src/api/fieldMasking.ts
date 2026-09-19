import { http } from '@/lib/http';

import type { StreamType } from './streams';

export type FieldMaskingAlgorithm =
  | { kind: 'full'; replacement: string }
  | { kind: 'range'; start: number; end: number; replacement: string }
  | { kind: 'inner'; prefix_chars: number; suffix_chars: number; replacement: string }
  | { kind: 'outer'; start: number; end: number; replacement: string }
  | { kind: 'hash' };

export interface FieldMaskingRule {
  id: string;
  org_id: string;
  name: string;
  priority: number;
  enabled: boolean;
  field_pattern: string;
  stream_pattern: string | null;
  stream_type: StreamType | null;
  algorithm: FieldMaskingAlgorithm;
  created_at: number;
  updated_at: number;
}

export interface FieldMaskingRuleInput {
  name: string;
  enabled: boolean;
  field_pattern: string;
  stream_pattern?: string | null;
  stream_type?: StreamType | null;
  algorithm: FieldMaskingAlgorithm;
}

export interface EffectiveFieldMaskingEntry {
  field: string;
  masked: boolean;
  source: 'none' | 'global' | 'stream';
  algorithm?: FieldMaskingAlgorithm;
  rule_id?: string;
  rule_name?: string;
  inherited_algorithm?: FieldMaskingAlgorithm;
  inherited_rule_id?: string;
  inherited_rule_name?: string;
}

export interface EffectiveFieldMasking {
  stream_id: string;
  fields: EffectiveFieldMaskingEntry[];
}

export async function listRules(): Promise<FieldMaskingRule[]> {
  const { data } = await http.get<FieldMaskingRule[]>('/field_masking/rules');
  return data;
}

export async function createRule(input: FieldMaskingRuleInput): Promise<FieldMaskingRule> {
  const { data } = await http.post<FieldMaskingRule>('/field_masking/rules', input);
  return data;
}

export async function updateRule(
  id: string,
  input: FieldMaskingRuleInput,
): Promise<FieldMaskingRule> {
  const { data } = await http.put<FieldMaskingRule>(
    `/field_masking/rules/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function deleteRule(id: string): Promise<void> {
  await http.delete(`/field_masking/rules/${encodeURIComponent(id)}`);
}

export async function reorderRules(ids: string[]): Promise<FieldMaskingRule[]> {
  const { data } = await http.put<FieldMaskingRule[]>('/field_masking/rules/reorder', { ids });
  return data;
}

export async function effectiveForStream(streamId: string): Promise<EffectiveFieldMasking> {
  const { data } = await http.get<EffectiveFieldMasking>(
    `/field_masking/effective/${encodeURIComponent(streamId)}`,
  );
  return data;
}
