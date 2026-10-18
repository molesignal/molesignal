/**
 * Mock Mole Agent runs: a message is kept and queued as a run, the runs of a
 * chat answer one after another, and what each does is written as numbered
 * events that `/agent/runs/:id/events?after=` reads from any number on, the way
 * the backend does. What a run does depends on the question:
 *
 * - by default it lists the streams and answers;
 * - "ask me" asks which environment and waits for the answer;
 * - "restart" asks to confirm a tool in confirmation mode and waits;
 * - "remember" remembers a preference;
 * - "plan" keeps a task list of three steps;
 * - "slow" takes its time between steps, for stopping and queueing.
 *
 * Runs are kept per test.
 */
import type { Express, Response } from 'express';

import type { Run, RunEvent, RunStatus } from '../../src/api/agent/runs';

type Message = Record<string, unknown>;

interface Deps {
  chatMessages: Map<string, Message[]>;
  agentChats: Array<Record<string, unknown>>;
  /** A command the backend refuses, with its status and body; otherwise the command kept. */
  readCommand: (content: string) => { status: number; body: unknown } | { command: unknown };
  budgetRefusal: () => unknown;
  budgetNotice: () => unknown;
  userId: string;
}

interface MockRun {
  run: Run;
  content: string;
  events: RunEvent[];
  listeners: Set<() => void>;
  /** Resolves the question or confirmation the run waits on. */
  answer: ((response: Record<string, unknown>) => void) | null;
}

const LIVE: RunStatus[] = ['running', 'waiting_input', 'waiting_approval'];
const OVER: RunStatus[] = ['succeeded', 'failed', 'cancelled', 'interrupted'];

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * A name for a chat from its first question, standing in for the model the server asks: what it
 * is about in a few words, without the question's lead-in. A greeting has no subject to name.
 */
function mockTitle(question: string): string | null {
  const subject = question
    .trim()
    .replace(/[?？!！.。]+$/u, '')
    .replace(/^(why|how|what|when|where|which|who)\s+(is|are|was|were|do|does|did|can|could|should)\s+/i, '');
  if (!subject || /^(hi|hello|hey|你好)$/i.test(subject)) return null;
  const words = subject.split(/\s+/).slice(0, 5).join(' ');
  return words.charAt(0).toLocaleUpperCase() + words.slice(1);
}

export function registerAgentRunRoutes(app: Express, deps: Deps) {
  const runs = new Map<string, MockRun>();
  let counter = 0;

  const emit = (entry: MockRun, kind: RunEvent['kind'], payload: unknown) => {
    entry.run.last_seq += 1;
    entry.events.push({ seq: entry.run.last_seq, kind, payload } as RunEvent);
    for (const listener of entry.listeners) listener();
  };

  const setStatus = (entry: MockRun, status: RunStatus) => {
    entry.run.status = status;
    emit(entry, 'status', { status });
  };

  const finish = (entry: MockRun, status: RunStatus, done: Record<string, unknown> = {}) => {
    entry.run.status = status;
    entry.run.finished_at_micros = Date.now() * 1000;
    emit(entry, 'done', { status, ...done });
    startNext(entry.run.chat_id);
  };

  const chatRuns = (chatId: string) =>
    [...runs.values()]
      .filter((entry) => entry.run.chat_id === chatId)
      .sort((a, b) => a.run.created_at_micros - b.run.created_at_micros);

  const startNext = (chatId: string) => {
    const list = chatRuns(chatId);
    if (list.some((entry) => LIVE.includes(entry.run.status))) return;
    const next = list.find((entry) => entry.run.status === 'queued');
    if (next) void execute(next);
  };

  const stopped = (entry: MockRun) => entry.run.cancel_requested;

  const keepAnswer = (entry: MockRun, content: string, evidence: unknown[] = []) => {
    const messages = deps.chatMessages.get(entry.run.chat_id) ?? [];
    const asked = messages.find((message) => message.id === entry.run.user_message_id);
    const id = `mock-assistant-${Date.now() * 1000}-${counter++}`;
    messages.push({
      id,
      chat_id: entry.run.chat_id,
      org_id: 'default',
      role: 'assistant',
      content,
      run_id: entry.run.id,
      evidence_json: evidence,
      created_at_micros: Number(asked?.created_at_micros ?? Date.now() * 1000) + 1,
    });
    deps.chatMessages.set(entry.run.chat_id, messages);
    const chat = deps.agentChats.find((item) => item.id === entry.run.chat_id);
    if (chat) chat.updated_at_micros = Date.now() * 1000;
    entry.run.assistant_message_id = id;
    return id;
  };

  const wait = (entry: MockRun, kind: 'question' | 'confirmation', pending: Record<string, unknown>) =>
    new Promise<Record<string, unknown> | null>((resolve) => {
      entry.run.pending = pending as unknown as NonNullable<Run['pending']>;
      emit(entry, kind, pending);
      setStatus(entry, kind === 'question' ? 'waiting_input' : 'waiting_approval');
      entry.answer = (response) => {
        entry.answer = null;
        entry.run.pending = null;
        emit(entry, 'response', response);
        setStatus(entry, 'running');
        resolve(response);
      };
      const check = setInterval(() => {
        if (!stopped(entry)) return;
        clearInterval(check);
        entry.answer = null;
        resolve(null);
      }, 50);
    });

  const execute = async (entry: MockRun) => {
    const { run, content } = entry;
    run.started_at_micros = Date.now() * 1000;
    setStatus(entry, 'running');
    const notice = deps.budgetNotice();
    if (notice) emit(entry, 'notice', notice);
    const pause = /slow/i.test(content) ? 1_500 : 120;

    if (run.kind === 'compact') {
      await sleep(pause);
      emit(entry, 'notice', { kind: 'compacted', automatic: false, messages: 4 });
      finish(entry, 'succeeded');
      return;
    }

    if (/plan/i.test(content)) {
      emit(entry, 'todo', {
        items: [
          { content: 'Read the error logs', status: 'completed' },
          { content: 'Compare with the last deploy', status: 'in_progress' },
          { content: 'Write it up', status: 'pending' },
        ],
      });
    }

    // A chat still titled by its first question is named while it is answered.
    const chat = deps.agentChats.find((item) => item.id === run.chat_id);
    const title = chat?.title_provisional ? mockTitle(content) : null;
    if (chat && title) {
      chat.title = title;
      chat.title_provisional = false;
      emit(entry, 'notice', { kind: 'title', title });
    }

    const toolId = `mock-tool-${run.id}`;
    emit(entry, 'text', { text: 'Let me look at the streams first.' });
    emit(entry, 'tool_start', {
      id: toolId,
      name: 'list_streams',
      arguments: { time_range: null },
    });
    await sleep(pause);
    if (stopped(entry)) return finish(entry, 'cancelled');
    emit(entry, 'tool_end', {
      id: toolId,
      name: 'list_streams',
      result: JSON.stringify([{ type: 'json', json: { streams: ['logs/default'], row_count: 1 } }]),
      truncated: false,
      is_error: false,
    });

    let extra = '';
    if (/ask me/i.test(content)) {
      const response = await wait(entry, 'question', {
        type: 'question',
        call_id: `question-${run.id}`,
        question: 'Which environment should I look at?',
        kind: 'single',
        options: ['staging', 'production'],
      });
      if (!response) return finish(entry, 'cancelled');
      const choices = (response.choices as string[] | undefined) ?? [];
      extra = ` In ${choices[0] ?? String(response.text ?? '')}.`;
    }
    if (/restart/i.test(content)) {
      const response = await wait(entry, 'confirmation', {
        type: 'confirmation',
        call_id: `confirm-${run.id}`,
        tool: 'edit_knowledge',
        arguments: { path: 'runbooks/payments.md', summary: 'Add the restart steps', delete: false },
        risk: 'l1',
      });
      if (!response) return finish(entry, 'cancelled');
      extra = response.approved ? ' The runbook was updated.' : ' Nothing was changed.';
    }
    if (/remember/i.test(content)) {
      const rememberId = `mock-remember-${run.id}`;
      emit(entry, 'tool_start', { id: rememberId, name: 'remember', arguments: { name: 'Time zone' } });
      emit(entry, 'tool_end', {
        id: rememberId,
        name: 'remember',
        result: JSON.stringify([
          { type: 'json', json: { remembered: { id: 'memory-time-zone', kind: 'preference', name: 'Time zone' } } },
        ]),
        truncated: false,
        is_error: false,
      });
    }

    for (const piece of ['The selected data scope ', 'is available.']) {
      await sleep(pause / 3);
      if (stopped(entry)) return finish(entry, 'cancelled');
      emit(entry, 'text', { text: piece });
    }

    const answer = `The selected data scope is available.${extra}`;
    const messageId = keepAnswer(entry, answer, [
      {
        tool_call_id: toolId,
        tool: 'list_streams',
        status: 'success',
        summary: '1 row',
        row_count: 1,
        took_ms: 84,
        arguments: { time_range: null },
      },
    ]);
    finish(entry, 'succeeded', {
      message_id: messageId,
      content: answer,
      prompt_tokens: 10,
      completion_tokens: 12,
      finish_reason: 'stop',
    });
  };

  const queue = (chatId: string, kind: Run['kind'], content: string, userMessageId: string | null) => {
    const now = Date.now() * 1000;
    const run: Run = {
      id: `mock-run-${now}-${counter++}`,
      chat_id: chatId,
      status: 'queued',
      source: 'chat',
      kind,
      initiator_id: deps.userId,
      user_message_id: userMessageId,
      assistant_message_id: null,
      pending: null,
      cancel_requested: false,
      last_seq: 0,
      prompt_tokens: 0,
      completion_tokens: 0,
      cost: null,
      error: null,
      created_at_micros: now,
      started_at_micros: null,
      finished_at_micros: null,
    };
    const entry: MockRun = { run, content, events: [], listeners: new Set(), answer: null };
    runs.set(run.id, entry);
    startNext(chatId);
    return run;
  };

  app.post('/api/v1/agent/chat/:id/messages', (req, res) => {
    // A command the backend refuses: nothing is kept and the model is not called.
    const content = String(req.body?.content ?? '');
    const read = deps.readCommand(content);
    if ('status' in read) {
      res.status(read.status).json(read.body);
      return;
    }
    // The day's budget is used up and chats are blocked: the question is not kept.
    const refusal = deps.budgetRefusal();
    if (refusal) {
      res.status(429).json(refusal);
      return;
    }
    const now = Date.now() * 1000;
    const messages = deps.chatMessages.get(req.params.id) ?? [];
    deps.chatMessages.set(req.params.id, messages);
    let questionId = String(req.body?.regenerate_from_message_id ?? '');
    if (!questionId) {
      questionId = `mock-user-${now}-${counter++}`;
      messages.push({
        id: questionId,
        chat_id: req.params.id,
        org_id: 'default',
        role: 'user',
        content,
        command: read.command,
        references: req.body?.references ?? null,
        created_at_micros: now,
      });
    }
    const run = queue(req.params.id, 'answer', content, questionId);
    res.status(202).json({ run, user_message_id: questionId });
  });

  app.post('/api/v1/agent/chat/:id/compact', (req, res) => {
    const run = queue(req.params.id, 'compact', '', null);
    res.status(202).json({ run });
  });

  app.get('/api/v1/agent/chat/:id/runs', (req, res) => {
    res.json({ runs: chatRuns(req.params.id).map((entry) => entry.run).reverse() });
  });

  app.get('/api/v1/agent/runs/:id', (req, res) => {
    const entry = runs.get(req.params.id);
    if (!entry) {
      res.status(404).json({ error: 'not_found', message: 'agent run not found' });
      return;
    }
    res.json(entry.run);
  });

  app.get('/api/v1/agent/runs/:id/events', (req, res: Response) => {
    const entry = runs.get(req.params.id);
    if (!entry) {
      res.status(404).json({ error: 'not_found', message: 'agent run not found' });
      return;
    }
    let seq = Number(req.query.after ?? 0) || 0;
    res.setHeader('content-type', 'text/event-stream');
    res.setHeader('cache-control', 'no-cache');
    res.flushHeaders();
    const flush = () => {
      for (const event of entry.events.filter((item) => item.seq > seq)) {
        seq = event.seq;
        res.write(`id: ${event.seq}\nevent: ${event.kind}\ndata: ${JSON.stringify(event.payload)}\n\n`);
        if (event.kind === 'done') {
          entry.listeners.delete(flush);
          res.end();
          return;
        }
      }
    };
    entry.listeners.add(flush);
    req.on('close', () => entry.listeners.delete(flush));
    flush();
  });

  app.post('/api/v1/agent/runs/:id/cancel', (req, res) => {
    const entry = runs.get(req.params.id);
    if (!entry) {
      res.status(404).json({ error: 'not_found', message: 'agent run not found' });
      return;
    }
    if (entry.run.status === 'queued') {
      entry.run.cancel_requested = true;
      finish(entry, 'cancelled');
    } else if (!OVER.includes(entry.run.status)) {
      entry.run.cancel_requested = true;
    }
    res.json(entry.run);
  });

  app.post('/api/v1/agent/runs/:id/respond', (req, res) => {
    const entry = runs.get(req.params.id);
    const pending = entry?.run.pending;
    if (!entry || !pending || pending.call_id !== req.body?.call_id || !entry.answer) {
      res.status(409).json({ error: 'conflict', message: 'the answer is not waiting for this' });
      return;
    }
    entry.answer({ ...req.body, responder: deps.userId });
    res.json(entry.run);
  });

  return {
    /** The runs of every chat, newest first. */
    all: () => [...runs.values()].map((entry) => entry.run),
  };
}
