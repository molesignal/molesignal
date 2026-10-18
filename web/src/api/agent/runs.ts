import { http } from '@/lib/http';
import { useAuthStore } from '@/stores/auth';

import type { Cost } from './usage';

/**
 * One round of answering is a run. Sending a message only queues it; the server
 * runs it in the background and writes what happens as numbered events, which any
 * page reads from a number on (`after`). Leaving the page or reloading never
 * stops an answer, and coming back reads on where it left off.
 */
export type RunStatus =
  | 'queued'
  | 'running'
  | 'waiting_input'
  | 'waiting_approval'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'interrupted';

/** What the run does: answer a message, compact the chat, or distill memories after it. */
export type RunKind = 'answer' | 'compact' | 'distill';

export type QuestionKind = 'single' | 'multiple' | 'text';

export interface PendingQuestion {
  type: 'question';
  call_id: string;
  question: string;
  kind: QuestionKind;
  options: string[];
}

export interface PendingConfirmation {
  type: 'confirmation';
  call_id: string;
  tool: string;
  arguments: Record<string, unknown>;
  risk: string;
}

export type RunPending = PendingQuestion | PendingConfirmation;

export interface Run {
  id: string;
  chat_id: string;
  status: RunStatus;
  source: string;
  kind: RunKind;
  initiator_id: string;
  user_message_id?: string | null;
  assistant_message_id?: string | null;
  pending?: RunPending | null;
  cancel_requested: boolean;
  last_seq: number;
  prompt_tokens: number;
  completion_tokens: number;
  /** In the currency of the model provider; null when it has no prices. */
  cost?: Cost | null;
  error?: string | null;
  created_at_micros: number;
  started_at_micros?: number | null;
  finished_at_micros?: number | null;
}

export const LIVE_STATUSES: readonly RunStatus[] = ['running', 'waiting_input', 'waiting_approval'];

export function isLive(status: RunStatus): boolean {
  return LIVE_STATUSES.includes(status);
}

export function isOver(status: RunStatus): boolean {
  return (
    status === 'succeeded' || status === 'failed' || status === 'cancelled' || status === 'interrupted'
  );
}

export type TodoStatus = 'pending' | 'in_progress' | 'completed';

export interface TodoItem {
  content: string;
  status: TodoStatus;
}

/** The end of a run: how it ended, and the answer when there is one. */
export interface DoneEvent {
  status: RunStatus;
  message_id?: string | null;
  content?: string | null;
  prompt_tokens?: number;
  completion_tokens?: number;
  /** In the currency of the model provider; null when it has no prices. */
  cost?: Cost | null;
  finish_reason?: string | null;
  error?: string | null;
  /** A fixed reason the page writes in the reader's language, such as the budget. */
  code?: string | null;
}

/** Something said along the way: the budget, a compaction, a team taken into the chat. */
export type RunNotice =
  | {
      kind: 'budget';
      level: 'warning' | 'exhausted';
      used_tokens: number;
      limit_tokens: number;
      resets_at_micros: number;
    }
  | { kind: 'compacted'; automatic: boolean; messages: number }
  | { kind: 'team_mounted'; team_id: string; team: string }
  | { kind: 'no_response'; call_id: string }
  | { kind: 'memories'; count: number; names: string[] }
  /** The chat was named while answering its first question. */
  | { kind: 'title'; title: string }
  /** Bypass permissions was asked for, but the chat's Agent Profile no longer allows it. */
  | { kind: 'bypass_unavailable' };

export type RunEvent =
  | { seq: number; kind: 'status'; payload: { status: RunStatus } }
  | { seq: number; kind: 'text'; payload: { text: string } }
  | {
      seq: number;
      kind: 'tool_start';
      payload: { id: string; name: string; arguments: unknown };
    }
  | {
      seq: number;
      kind: 'tool_end';
      payload: { id: string; name: string; result: string; truncated: boolean; is_error: boolean };
    }
  | { seq: number; kind: 'todo'; payload: { items: TodoItem[] } }
  | { seq: number; kind: 'question'; payload: PendingQuestion }
  | { seq: number; kind: 'confirmation'; payload: PendingConfirmation }
  | { seq: number; kind: 'response'; payload: Record<string, unknown> }
  | { seq: number; kind: 'notice'; payload: RunNotice }
  | { seq: number; kind: 'done'; payload: DoneEvent };

export async function getRun(id: string): Promise<Run> {
  const { data } = await http.get<Run>(`/agent/runs/${encodeURIComponent(id)}`);
  return data;
}

export async function listChatRuns(chatId: string, limit = 20): Promise<Run[]> {
  const { data } = await http.get<{ runs: Run[] }>(
    `/agent/chat/${encodeURIComponent(chatId)}/runs`,
    { params: { limit } },
  );
  return data.runs ?? [];
}

export async function cancelRun(id: string): Promise<Run> {
  const { data } = await http.post<Run>(`/agent/runs/${encodeURIComponent(id)}/cancel`);
  return data;
}

export interface RespondInput {
  call_id: string;
  choices?: string[];
  text?: string;
  approved?: boolean;
}

export async function respondRun(id: string, input: RespondInput): Promise<Run> {
  const { data } = await http.post<Run>(`/agent/runs/${encodeURIComponent(id)}/respond`, input);
  return data;
}

/** `/compact`: the chat so far becomes a summary; answers after it carry the summary instead. */
export async function compactChat(chatId: string): Promise<Run> {
  const { data } = await http.post<{ run: Run }>(`/agent/chat/${encodeURIComponent(chatId)}/compact`);
  return data.run;
}

/** A block of a server-sent event stream: `id:`, `event:` and `data:` lines. */
export function parseFrame(frame: string): RunEvent | null {
  let id = '';
  let event = 'message';
  let data = '';
  for (const line of frame.split('\n')) {
    if (line.startsWith('id:')) id = line.slice(3).trim();
    else if (line.startsWith('event:')) event = line.slice(6).trim();
    else if (line.startsWith('data:')) data += line.slice(5).trim();
  }
  if (!data || !id) return null;
  try {
    return { seq: Number(id), kind: event, payload: JSON.parse(data) } as RunEvent;
  } catch {
    return null;
  }
}

export interface StreamOptions {
  after: number;
  onEvent: (event: RunEvent) => void;
  signal: AbortSignal;
}

/**
 * Read the events of a run from `after` on until its end event. Returns how it
 * stopped: `done` after the end event, `closed` when the server closed the
 * stream early (read on from the last number), `aborted` when the page stopped
 * reading, `failed` on an error the caller may retry.
 */
export async function streamRunEvents(
  runId: string,
  { after, onEvent, signal }: StreamOptions,
): Promise<'done' | 'closed' | 'aborted' | 'failed'> {
  const token = useAuthStore.getState().token;
  let response: Response;
  try {
    response = await fetch(
      `/api/v1/agent/runs/${encodeURIComponent(runId)}/events?after=${after}`,
      {
        headers: {
          accept: 'text/event-stream',
          ...(token ? { authorization: `Bearer ${token}` } : {}),
        },
        signal,
      },
    );
  } catch (error) {
    return (error as Error)?.name === 'AbortError' ? 'aborted' : 'failed';
  }
  if (!response.ok || !response.body) return 'failed';

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    let chunk: ReadableStreamReadResult<Uint8Array>;
    try {
      chunk = await reader.read();
    } catch (error) {
      return (error as Error)?.name === 'AbortError' ? 'aborted' : 'failed';
    }
    if (chunk.done) return 'closed';
    buffer += decoder.decode(chunk.value, { stream: true });
    let separator: number;
    while ((separator = buffer.indexOf('\n\n')) !== -1) {
      const frame = buffer.slice(0, separator);
      buffer = buffer.slice(separator + 2);
      const event = parseFrame(frame);
      if (!event) continue;
      onEvent(event);
      if (event.kind === 'done') {
        void reader.cancel().catch(() => undefined);
        return 'done';
      }
    }
  }
}

function pause(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = window.setTimeout(resolve, ms);
    signal.addEventListener(
      'abort',
      () => {
        window.clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });
}

/**
 * Follow a run to its end: read its events from `after` on, and after a dropped
 * connection read on from the last event seen, waiting a little longer each
 * time it fails. Resolves true once the end event arrived, false when stopped.
 */
export async function followRun(
  runId: string,
  onEvent: (event: RunEvent) => void,
  signal: AbortSignal,
  after = 0,
): Promise<boolean> {
  let seq = after;
  let failures = 0;
  while (!signal.aborted) {
    const outcome = await streamRunEvents(runId, {
      after: seq,
      signal,
      onEvent: (event) => {
        seq = event.seq;
        onEvent(event);
      },
    });
    if (outcome === 'done') return true;
    if (outcome === 'aborted') return false;
    failures = outcome === 'failed' ? failures + 1 : 0;
    await pause(Math.min(500 * 2 ** failures, 10_000), signal);
  }
  return false;
}
