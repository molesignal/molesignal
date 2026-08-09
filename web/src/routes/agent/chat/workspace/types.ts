import type { ChatCapability } from '@/api/agent/chat';

export const CHAT_MODES = ['auto', 'quick', 'deep', 'query_only'] as const;
export const EXECUTION_POLICIES = ['advice_only', 'read_only', 'policy'] as const;

export type ChatMode = (typeof CHAT_MODES)[number];
export type ExecutionPolicy = (typeof EXECUTION_POLICIES)[number];

export interface ChatContext {
  environment: string;
  service: string;
  alert: string;
}

export interface StarterSelection {
  prompt: string;
  context?: Partial<ChatContext>;
  rangePreset?: string;
  mode?: ChatMode;
  capability?: ChatCapability;
  executionPolicy?: ExecutionPolicy;
}
