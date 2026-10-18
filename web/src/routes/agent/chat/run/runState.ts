import type {
  DoneEvent,
  Run,
  RunEvent,
  RunNotice,
  RunPending,
  RunStatus,
  TodoItem,
} from '@/api/agent/runs';

/** A tool the model called in the answer being written. */
export interface LiveTool {
  id: string;
  name: string;
  status: 'running' | 'done' | 'error';
  arguments?: string;
  result?: string;
}

/** A memory the answer wrote or dropped; the chat offers to undo it. */
export interface MemoryChange {
  action: 'remembered' | 'forgotten';
  id: string;
  name: string;
}

/** What a run being followed has shown so far, rebuilt from its events. */
export interface RunView {
  runId: string;
  status: RunStatus;
  /** Who asked: a confirmation is theirs to decide. */
  initiatorId: string;
  /** When the run started, or was queued: how long the answer has taken is counted from it. */
  startedAtMicros: number;
  /** What the model is writing now: the answer, once it stops calling tools. */
  text: string;
  tools: LiveTool[];
  todo: TodoItem[];
  pending: RunPending | null;
  notices: RunNotice[];
  memories: MemoryChange[];
  done: DoneEvent | null;
  lastSeq: number;
}

/** Tools shown otherwise: the question and the task list have cards of their own. */
const CARD_TOOLS = new Set(['ask_user', 'todo_write']);

export function emptyView(
  run: Pick<Run, 'id' | 'status' | 'initiator_id' | 'created_at_micros' | 'started_at_micros'>,
): RunView {
  return {
    runId: run.id,
    status: run.status,
    initiatorId: run.initiator_id,
    startedAtMicros: run.started_at_micros ?? run.created_at_micros,
    text: '',
    tools: [],
    todo: [],
    pending: null,
    notices: [],
    memories: [],
    done: null,
    lastSeq: 0,
  };
}

function stringify(value: unknown): string {
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value);
  } catch {
    return '';
  }
}

/** The memory a `remember` or `forget` result names. */
export function memoryChange(result: string): MemoryChange | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(result);
  } catch {
    return null;
  }
  const blocks = Array.isArray(parsed) ? parsed : [parsed];
  for (const block of blocks) {
    const json = (block as { json?: unknown })?.json ?? block;
    for (const action of ['remembered', 'forgotten'] as const) {
      const memory = (json as Record<string, { id?: unknown; name?: unknown }> | null)?.[action];
      if (memory && typeof memory.id === 'string' && typeof memory.name === 'string') {
        return { action, id: memory.id, name: memory.name };
      }
    }
  }
  return null;
}

/** Fold one event into what is shown. Events already seen (a re-read after a reconnect) change nothing. */
export function applyEvent(view: RunView, event: RunEvent): RunView {
  if (event.seq <= view.lastSeq) return view;
  const next: RunView = { ...view, lastSeq: event.seq };

  switch (event.kind) {
    case 'status':
      return { ...next, status: event.payload.status };
    case 'text':
      return { ...next, text: next.text + event.payload.text };
    case 'tool_start': {
      // What was written before a tool call is a note on the way, not the answer.
      if (CARD_TOOLS.has(event.payload.name)) return { ...next, text: '' };
      return {
        ...next,
        text: '',
        tools: [
          ...next.tools,
          {
            id: event.payload.id,
            name: event.payload.name,
            status: 'running',
            arguments: stringify(event.payload.arguments),
          },
        ],
      };
    }
    case 'tool_end': {
      const { id, name, result, is_error: isError } = event.payload;
      const change =
        !isError && (name === 'remember' || name === 'forget') ? memoryChange(result) : null;
      return {
        ...next,
        tools: next.tools.map((tool) =>
          tool.id === id ? { ...tool, status: isError ? 'error' : 'done', result } : tool,
        ),
        memories: change ? [...next.memories, change] : next.memories,
      };
    }
    case 'todo':
      return { ...next, todo: event.payload.items ?? [] };
    case 'question':
    case 'confirmation':
      return { ...next, pending: event.payload };
    case 'response':
      return { ...next, pending: null };
    case 'notice':
      return { ...next, notices: [...next.notices, event.payload] };
    case 'done':
      return { ...next, status: event.payload.status, pending: null, done: event.payload };
    default:
      return next;
  }
}

/** Which step of the task list is under way: the first not completed, out of all. */
export function todoProgress(items: TodoItem[]): { current: number; total: number } | null {
  if (items.length === 0) return null;
  const completed = items.filter((item) => item.status === 'completed').length;
  return { current: Math.min(completed + 1, items.length), total: items.length };
}
