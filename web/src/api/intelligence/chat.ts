import { http } from '@/lib/http';
import { useAuthStore } from '@/stores/auth';

export interface Chat {
  id: string;
  provider: string;
  model: string;
  title: string;
  provider_id?: string | null;
  analysis_mode?: string | null;
  capability?: ChatCapability | null;
  time_range_start_micros?: number | null;
  time_range_end_micros?: number | null;
  archive_object_key?: string | null;
  created_at_micros: number;
  updated_at_micros: number;
}

export type ChatCapability = 'dashboard_authoring';

export interface ChatMessageRow {
  id: string;
  chat_id: string;
  org_id: string;
  role: string;
  content: string;
  prompt_template_id?: string | null;
  prompt_builtin_key?: string | null;
  prompt_version?: number | null;
  prompt_hash?: string | null;
  evidence_json?: unknown;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  cost_usd?: number | null;
  created_at_micros: number;
}

export interface CreateChatInput {
  provider: string;
  model: string;
  title?: string | undefined;
  provider_id?: string | undefined;
  analysis_mode?: string | undefined;
  capability?: ChatCapability | undefined;
}

export interface TimeRange {
  start_micros: number;
  end_micros: number;
}

export interface PostMessageBody {
  content: string;
  regenerate_from_message_id?: string | undefined;
  investigation_id?: string | undefined;
  time_range?: TimeRange | undefined;
  analysis_mode?: string | undefined;
  capability?: ChatCapability | undefined;
  execution_policy?: 'advice_only' | 'read_only' | 'policy' | undefined;
  stream_hints?: string[] | undefined;
  agent_profile_id?: string | undefined;
  provider_id?: string | undefined;
  model?: string | undefined;
  prompt_template_id?: string | undefined;
}

export interface ToolStartEvent {
  id: string;
  name: string;
  arguments: string;
}
export interface ToolEndEvent {
  id: string;
  result: string;
  is_error: boolean;
}
export interface DoneEvent {
  prompt_tokens: number;
  completion_tokens: number;
  finish_reason: string;
}

export interface StreamHandlers {
  onChunk?: (text: string) => void;
  onToolStart?: (e: ToolStartEvent) => void;
  onToolEnd?: (e: ToolEndEvent) => void;
  onDone?: (e: DoneEvent) => void;
  onError?: (message: string) => void;
}

export async function listChats(): Promise<Chat[]> {
  const { data } = await http.get<Chat[]>('/intelligence/chat');
  return data;
}

export async function createChat(input: CreateChatInput): Promise<Chat> {
  const { data } = await http.post<Chat>('/intelligence/chat', input);
  return data;
}

export async function deleteChat(id: string): Promise<void> {
  await http.delete(`/intelligence/chat/${encodeURIComponent(id)}`);
}

export async function listMessages(id: string): Promise<ChatMessageRow[]> {
  const { data } = await http.get<{ messages: ChatMessageRow[] }>(
    `/intelligence/chat/${encodeURIComponent(id)}/messages`,
  );
  return data.messages ?? [];
}

export async function archiveChat(id: string): Promise<{ status: string; object_key?: string | null }> {
  const { data } = await http.post<{ status: string; object_key?: string | null }>(
    `/intelligence/chat/${encodeURIComponent(id)}/archive`,
  );
  return data;
}

/**
 * Stream a chat reply over SSE. Uses fetch (not axios) so we can read the
 * response body incrementally; dispatches typed events to `handlers`.
 */
export async function postMessageStream(
  chatId: string,
  body: PostMessageBody,
  handlers: StreamHandlers,
  signal?: AbortSignal,
): Promise<void> {
  const token = useAuthStore.getState().token;
  let resp: Response;
  try {
    resp = await fetch(
      `/api/v1/intelligence/chat/${encodeURIComponent(chatId)}/messages`,
      {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          accept: 'text/event-stream',
          ...(token ? { authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify(body),
        signal: signal ?? null,
      },
    );
  } catch (e) {
    if ((e as Error)?.name === 'AbortError') return;
    handlers.onError?.(String((e as Error)?.message ?? e));
    return;
  }

  if (!resp.ok || !resp.body) {
    const text = await resp.text().catch(() => '');
    handlers.onError?.(text || `HTTP ${resp.status}`);
    return;
  }

  const reader = resp.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    let chunk: ReadableStreamReadResult<Uint8Array>;
    try {
      chunk = await reader.read();
    } catch (e) {
      if ((e as Error)?.name === 'AbortError') return;
      handlers.onError?.(String((e as Error)?.message ?? e));
      return;
    }
    if (chunk.done) break;
    buffer += decoder.decode(chunk.value, { stream: true });
    let sep: number;
    while ((sep = buffer.indexOf('\n\n')) !== -1) {
      const frame = buffer.slice(0, sep);
      buffer = buffer.slice(sep + 2);
      dispatchFrame(frame, handlers);
    }
  }
}

function dispatchFrame(frame: string, handlers: StreamHandlers): void {
  let event = 'message';
  let data = '';
  for (const line of frame.split('\n')) {
    if (line.startsWith('event:')) event = line.slice(6).trim();
    else if (line.startsWith('data:')) data += line.slice(5).trim();
  }
  if (!data) return;
  let payload: Record<string, unknown>;
  try {
    payload = JSON.parse(data);
  } catch {
    return;
  }
  switch (event) {
    case 'chunk':
      handlers.onChunk?.(String(payload.text ?? ''));
      break;
    case 'tool_start':
      handlers.onToolStart?.(payload as unknown as ToolStartEvent);
      break;
    case 'tool_end':
      handlers.onToolEnd?.(payload as unknown as ToolEndEvent);
      break;
    case 'done':
      handlers.onDone?.(payload as unknown as DoneEvent);
      break;
    case 'error':
      handlers.onError?.(String(payload.message ?? 'error'));
      break;
  }
}
