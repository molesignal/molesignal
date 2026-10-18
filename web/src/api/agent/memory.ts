import { http } from '@/lib/http';

/** What a memory is: a preference, a way of doing something, a fact, or a lesson. */
export type MemoryKind = 'preference' | 'procedure' | 'fact' | 'lesson';

export const MEMORY_KINDS: readonly MemoryKind[] = ['preference', 'procedure', 'fact', 'lesson'];

export interface Memory {
  id: string;
  scope: 'personal' | 'team';
  team_id?: string | null;
  kind: MemoryKind;
  name: string;
  /** One sentence on when it applies. */
  description: string;
  body: string;
  source_chat_id?: string | null;
  /** The name of the chat it came from; absent when that chat is gone. */
  source_chat_title?: string | null;
  created_by: string;
  updated_by: string;
  created_at_micros: number;
  updated_at_micros: number;
  forgotten_at_micros?: number | null;
}

export interface MemoryInput {
  kind: MemoryKind;
  name: string;
  description: string;
  body: string;
}

/** The person's own memories, or a team's. */
export async function listMemories(scope: { teamId?: string | null } = {}): Promise<Memory[]> {
  const params = scope.teamId ? { scope: 'team', team_id: scope.teamId } : { scope: 'personal' };
  const { data } = await http.get<{ memories: Memory[] }>('/agent/memories', { params });
  return data.memories ?? [];
}

export async function updateMemory(id: string, input: MemoryInput): Promise<Memory> {
  const { data } = await http.patch<Memory>(`/agent/memories/${encodeURIComponent(id)}`, input);
  return data;
}

/** Delete a memory: also how a "remembered" is undone in the chat. */
export async function deleteMemory(id: string): Promise<void> {
  await http.delete(`/agent/memories/${encodeURIComponent(id)}`);
}

/** Bring back a memory that was forgotten. */
export async function restoreMemory(id: string): Promise<Memory> {
  const { data } = await http.post<Memory>(`/agent/memories/${encodeURIComponent(id)}/restore`);
  return data;
}

export interface MemorySettings {
  /** After a chat goes quiet, the model picks out what is worth remembering from it. */
  distill_after_chat: boolean;
  can_edit: boolean;
  updated_at_micros?: number | null;
}

export async function memorySettings(): Promise<MemorySettings> {
  const { data } = await http.get<MemorySettings>('/agent/memory-settings');
  return data;
}

export async function setMemorySettings(distillAfterChat: boolean): Promise<MemorySettings> {
  const { data } = await http.put<MemorySettings>('/agent/memory-settings', {
    distill_after_chat: distillAfterChat,
  });
  return data;
}
