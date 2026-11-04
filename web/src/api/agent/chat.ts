import { AxiosError } from 'axios';

import { http, toApiError } from '@/lib/http';

import { type CommandRefusal, type MessageCommand, commandRefusal } from './commands';
import type { Run } from './runs';
import type { Cost } from './usage';

export interface Chat {
  id: string;
  provider: string;
  model: string;
  title: string;
  providerId?: string | null;
  analysisMode?: string | null;
  capability?: ChatCapability | null;
  timeRangeStart?: number | null;
  timeRangeEnd?: number | null;
  archiveObjectKey?: string | null;
  /** A personal chat, or one for a team whose members read and continue it. */
  scope?: 'personal' | 'team';
  teamId?: string | null;
  /** Teams whose skills and MCP servers a personal chat has taken in. */
  mountedTeamIds?: string[];
  /** Who started the chat. */
  ownerId?: string;
  /** False when only the title and usage can be seen. */
  readable?: boolean;
  /** What the chat investigates, such as the incident it was started from. */
  subjectKind?: 'incident' | 'service' | 'rum' | 'finding' | null;
  subjectId?: string | null;
  /** The investigation record the answers of the chat write to. */
  investigationId?: string | null;
  /** The chat this one was forked from. */
  parentChatId?: string | null;
  /** A run of this automation: the chat is not listed and cannot be continued. */
  automationId?: string | null;
  createdAt: number;
  updatedAt: number;
}

export type ChatCapability = 'dashboard_authoring';

export interface ChatMessageRow {
  id: string;
  chatId: string;
  orgId: string;
  role: string;
  content: string;
  promptTemplateId?: string | null;
  promptBuiltinKey?: string | null;
  promptVersion?: number | null;
  promptHash?: string | null;
  evidenceJson?: unknown;
  promptTokens?: number | null;
  completionTokens?: number | null;
  /** In the currency of the model provider; null when it has no prices. */
  cost?: Cost | null;
  /** The command the user message was; `content` keeps what was typed. */
  command?: MessageCommand | null;
  /** Who asked: in a team chat the questions can come from different people. */
  senderId?: string | null;
  senderName?: string | null;
  /** What was attached to a question with @. */
  references?: MessageReference[] | null;
  /** The run an answer came from. */
  runId?: string | null;
  createdAt: number;
}

export type ReferenceKind = 'incident' | 'alert_rule' | 'service' | 'rum_application' | 'dashboard' | 'stream';

/** Something attached to a question with @: the model reads it as the subject of the question. */
export interface MessageReference {
  kind: ReferenceKind;
  id: string;
  label: string;
}

export interface CreateChatInput {
  provider: string;
  model: string;
  title?: string | undefined;
  /** The title is the first question for now: the server names the chat while answering it. */
  titleProvisional?: boolean | undefined;
  providerId?: string | undefined;
  analysisMode?: string | undefined;
  capability?: ChatCapability | undefined;
  /** The team a team chat is for; none for a personal chat. */
  teamId?: string | undefined;
  /** What the chat investigates: the incident it is started from, a service by its name, or a RUM application by its id. */
  subject?: { kind: 'incident' | 'service' | 'rum' | 'finding'; id: string } | undefined;
}

export interface TimeRange {
  startTime: number;
  endTime: number;
}

export interface PostMessageBody {
  content: string;
  regenerateFromMessageId?: string | undefined;
  investigationId?: string | undefined;
  timeRange?: TimeRange | undefined;
  analysisMode?: string | undefined;
  capability?: ChatCapability | undefined;
  executionPolicy?:
    | 'advice_only'
    | 'read_only'
    | 'policy'
    | 'auto'
    | 'bypass_permissions'
    | undefined;
  streamHints?: string[] | undefined;
  agentProfileId?: string | undefined;
  providerId?: string | undefined;
  model?: string | undefined;
  promptTemplateId?: string | undefined;
  references?: MessageReference[] | undefined;
}

/**
 * The organization's day budget, said along with an answer once it is close to
 * or past its limit, or instead of one when chats are blocked at the limit.
 */
export interface BudgetNotice {
  kind: 'budget';
  level: 'warning' | 'exhausted';
  usedTokens: number;
  limitTokens: number;
  resetsAt: number;
  /** The question was not sent: the organization blocks chats once the budget is used up. */
  blocked?: boolean;
}

/** The backend's code for a request refused because the day's budget is used up. */
export const BUDGET_EXHAUSTED = 'agent_budget_exhausted';

/** A 429 that refused the question for the day's budget, read into a notice. */
export function budgetRefusal(status: number, body: string): BudgetNotice | null {
  if (status !== 429) return null;
  let payload: Record<string, unknown>;
  try {
    payload = JSON.parse(body) as Record<string, unknown>;
  } catch {
    return null;
  }
  if (payload.error !== BUDGET_EXHAUSTED && payload.code !== BUDGET_EXHAUSTED) return null;
  return {
    kind: 'budget',
    level: 'exhausted',
    usedTokens: Number(payload.usedTokens ?? 0),
    limitTokens: Number(payload.limitTokens ?? 0),
    resetsAt: Number(payload.resetsAt ?? 0),
    blocked: true,
  };
}

export async function listChats(): Promise<Chat[]> {
  const { data } = await http.get<Chat[]>('/agent/chat');
  return data;
}

/** One chat the reader can read, listed or not (the run of an automation is not). */
export async function getChat(id: string): Promise<Chat> {
  const { data } = await http.get<Chat>(`/agent/chat/${encodeURIComponent(id)}`);
  return data;
}

export async function createChat(input: CreateChatInput): Promise<Chat> {
  const { data } = await http.post<Chat>('/agent/chat', input);
  return data;
}

/**
 * A new chat holding this one's messages up to `messageId` (all of them without
 * it), to go on another way from there; the person's own.
 */
export async function forkChat(id: string, messageId?: string): Promise<Chat> {
  const { data } = await http.post<Chat>(`/agent/chat/${encodeURIComponent(id)}/fork`, {
    messageId: messageId,
  });
  return data;
}

/** Names the chat; the server no longer names it from its first question afterwards. */
export async function renameChat(id: string, title: string): Promise<Chat> {
  const { data } = await http.patch<Chat>(`/agent/chat/${encodeURIComponent(id)}`, { title });
  return data;
}

export async function deleteChat(id: string): Promise<void> {
  await http.delete(`/agent/chat/${encodeURIComponent(id)}`);
}

export async function listMessages(id: string): Promise<ChatMessageRow[]> {
  const { data } = await http.get<{ messages: ChatMessageRow[] }>(
    `/agent/chat/${encodeURIComponent(id)}/messages`,
  );
  return data.messages ?? [];
}

export async function archiveChat(id: string): Promise<{ status: string; objectKey?: string | null }> {
  const { data } = await http.post<{ status: string; objectKey?: string | null }>(
    `/agent/chat/${encodeURIComponent(id)}/archive`,
  );
  return data;
}

/** What became of a message: queued as a run, or refused before anything was kept. */
export type PostResult =
  | { kind: 'queued'; run: Run; userMessageId: string }
  | { kind: 'budget'; notice: BudgetNotice }
  | { kind: 'refused'; refusal: CommandRefusal }
  | { kind: 'error'; message: string };

/**
 * Send a message. It is kept and queued as a run of the chat; the answer is read
 * from the run's events. A run already answering in the chat goes first.
 */
export async function postMessage(chatId: string, body: PostMessageBody): Promise<PostResult> {
  try {
    const { data } = await http.post<{ run: Run; userMessageId: string }>(
      `/agent/chat/${encodeURIComponent(chatId)}/messages`,
      body,
    );
    return { kind: 'queued', run: data.run, userMessageId: data.userMessageId };
  } catch (error) {
    if (error instanceof AxiosError && error.response) {
      const raw = error.response.data;
      const text = typeof raw === 'string' ? raw : JSON.stringify(raw ?? {});
      const notice = budgetRefusal(error.response.status, text);
      if (notice) return { kind: 'budget', notice };
      const refusal = commandRefusal(text);
      if (refusal) return { kind: 'refused', refusal };
    }
    return { kind: 'error', message: toApiError(error).message };
  }
}

/** Write an answer of a chat started from an incident back to the incident as its RCA. */
export async function writeIncidentRca(chatId: string, messageId: string): Promise<void> {
  await http.post(`/agent/chat/${encodeURIComponent(chatId)}/rca`, { messageId: messageId });
}
