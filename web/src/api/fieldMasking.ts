import { http } from '@/lib/http';

import type { StreamType } from './streams';

export type FieldMaskingAlgorithm =
  | { kind: 'full'; replacement: string }
  | { kind: 'range'; start: number; end: number; replacement: string }
  | { kind: 'inner'; prefixChars: number; suffixChars: number; replacement: string }
  | { kind: 'outer'; start: number; end: number; replacement: string }
  | { kind: 'hash' };

export interface FieldMaskingRule {
  id: string;
  orgId: string;
  name: string;
  priority: number;
  enabled: boolean;
  fieldPattern: string;
  streamPattern: string | null;
  streamType: StreamType | null;
  algorithm: FieldMaskingAlgorithm;
  createdAt: number;
  updatedAt: number;
}

export interface FieldMaskingRuleInput {
  name: string;
  enabled: boolean;
  fieldPattern: string;
  streamPattern?: string | null;
  streamType?: StreamType | null;
  algorithm: FieldMaskingAlgorithm;
}

export interface EffectiveFieldMaskingEntry {
  field: string;
  masked: boolean;
  source: 'none' | 'global' | 'stream';
  algorithm?: FieldMaskingAlgorithm;
  ruleId?: string;
  ruleName?: string;
  inheritedAlgorithm?: FieldMaskingAlgorithm;
  inheritedRuleId?: string;
  inheritedRuleName?: string;
}

export interface EffectiveFieldMasking {
  streamId: string;
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
