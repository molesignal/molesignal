import { createContext } from 'react';

import { andFieldClause } from '@/shell/query/fieldExpression';

export type AttributeFilter = (key: string, value: string | number | boolean, operator: '=' | '!=') => void;
// null means SQL mode; undefined lets standalone detail views open the trace explorer.
export const AttributeFilterContext = createContext<AttributeFilter | null | undefined>(undefined);
export function attributeEntries(input: unknown): [string, unknown][] {
  if (typeof input === 'string') {
    try { return attributeEntries(JSON.parse(input)); } catch { return [['value', input]]; }
  }
  if (!input || typeof input !== 'object' || Array.isArray(input)) return [];
  return Object.entries(input).sort(([a], [b]) => a.localeCompare(b));
}
export function attributeValue(value: unknown): string {
  return typeof value === 'string' ? value : JSON.stringify(value) ?? String(value);
}
export function attributeGroup(key: string): string {
  if (key.startsWith('db.')) return 'database';
  if (key.startsWith('service.')) return 'service';
  if (key.startsWith('deployment.')) return 'deployment';
  if (key.startsWith('telemetry.') || key.startsWith('otel.')) return 'telemetry';
  if (/^(host|process|container|k8s|cloud|os)\./.test(key)) return 'resource';
  return 'other';
}
export function attributeClause(key: string, value: string | number | boolean, operator: '=' | '!='): string {
  return `${key} ${operator} ${JSON.stringify(value)}`;
}
/** The query narrowed by `clause`: one that joins alternatives with OR is put in brackets first. */
export function appendAttributeClause(query: string, clause: string): string {
  return andFieldClause(query, clause);
}
