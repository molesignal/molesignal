import { http } from '@/lib/http';
import type { Priority, Severity, Urgency } from '@/types/alerting';

export interface PriorityLevel {
  priority: Priority;
  /** Empty: the default name in the reader's language. */
  name: string;
  description: string;
}

export interface UrgencyLevel {
  key: Urgency;
  /** Empty: the default name in the reader's language. */
  name: string;
  /** Pages past the first step of a policy, and repeats it. */
  escalates: boolean;
  /** In quiet hours: never wakes anyone (`block`), or follows the person's switch (`switch`). */
  quietHours: 'block' | 'switch';
  /** Which of a person's two step groups it uses. */
  steps: 'high' | 'low';
  /** The severities "by severity" puts in this level. */
  severities: Severity[];
}

export interface ImportanceLevels {
  priorities: PriorityLevel[];
  /** The most urgent first. */
  urgencies: UrgencyLevel[];
}

export interface StoredImportance {
  levels: ImportanceLevels;
  updatedBy?: string | null;
  updatedAt: number;
}

/** The levels an organization has until it changes them: P1 to P5, high and low. */
export const DEFAULT_LEVELS: ImportanceLevels = {
  priorities: (['P1', 'P2', 'P3', 'P4', 'P5'] as const).map((priority) => ({ priority, name: '', description: '' })),
  urgencies: [
    { key: 'high', name: '', escalates: true, quietHours: 'switch', steps: 'high', severities: ['critical', 'error'] },
    { key: 'low', name: '', escalates: false, quietHours: 'block', steps: 'low', severities: ['warning', 'info'] },
  ],
};

export async function get(): Promise<StoredImportance> {
  const { data } = await http.get<StoredImportance>('/alerts/importance');
  return data;
}

export async function save(levels: ImportanceLevels): Promise<StoredImportance> {
  const { data } = await http.put<StoredImportance>('/alerts/importance', levels);
  return data;
}
