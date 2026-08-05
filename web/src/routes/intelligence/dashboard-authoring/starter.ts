import type { ChatCapability } from '@/api/intelligence/chat';

export interface DashboardStarterSelection {
  prompt: string;
  rangePreset: '1h';
  mode: 'auto';
  capability: ChatCapability;
  executionPolicy: 'policy';
}

export function dashboardStarterSelection(
  prompt: string,
): DashboardStarterSelection {
  return {
    prompt,
    rangePreset: '1h',
    mode: 'auto',
    capability: 'dashboard_authoring',
    executionPolicy: 'policy',
  };
}
