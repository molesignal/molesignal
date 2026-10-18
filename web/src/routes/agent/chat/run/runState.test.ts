import { describe, expect, it } from 'vitest';

import type { RunEvent } from '@/api/agent/runs';

import { applyEvent, emptyView, memoryChange, todoProgress } from './runState';

const fold = (events: RunEvent[]) =>
  events.reduce(
    applyEvent,
    emptyView({
      id: 'run-1',
      status: 'running',
      initiator_id: 'user-1',
      created_at_micros: 1,
      started_at_micros: 2,
    }),
  );

describe('a followed run', () => {
  it('drops the note written before a tool call and keeps the answer written after', () => {
    const view = fold([
      { seq: 1, kind: 'status', payload: { status: 'running' } },
      { seq: 2, kind: 'text', payload: { text: 'Checking the logs.' } },
      { seq: 3, kind: 'tool_start', payload: { id: 'c1', name: 'query_logs', arguments: { q: '5xx' } } },
      {
        seq: 4,
        kind: 'tool_end',
        payload: { id: 'c1', name: 'query_logs', result: '[]', truncated: false, is_error: false },
      },
      { seq: 5, kind: 'text', payload: { text: 'The API ' } },
      { seq: 6, kind: 'text', payload: { text: 'is failing.' } },
    ]);

    expect(view.text).toBe('The API is failing.');
    expect(view.tools).toEqual([
      { id: 'c1', name: 'query_logs', status: 'done', arguments: '{"q":"5xx"}', result: '[]' },
    ]);
  });

  it('reads an event once, even when the stream is read again from an earlier number', () => {
    const once = fold([
      { seq: 1, kind: 'text', payload: { text: 'a' } },
      { seq: 2, kind: 'text', payload: { text: 'b' } },
    ]);
    const again = applyEvent(once, { seq: 2, kind: 'text', payload: { text: 'b' } });

    expect(again.text).toBe('ab');
    expect(again).toBe(once);
  });

  it('shows the question until it is answered and the task list without their tool rows', () => {
    const waiting = fold([
      { seq: 1, kind: 'tool_start', payload: { id: 't1', name: 'todo_write', arguments: {} } },
      {
        seq: 2,
        kind: 'todo',
        payload: {
          items: [
            { content: 'Read the errors', status: 'completed' },
            { content: 'Check the deploy', status: 'in_progress' },
            { content: 'Write it up', status: 'pending' },
          ],
        },
      },
      { seq: 3, kind: 'tool_start', payload: { id: 'q1', name: 'ask_user', arguments: {} } },
      {
        seq: 4,
        kind: 'question',
        payload: { type: 'question', call_id: 'p1', question: 'Which region?', kind: 'single', options: ['eu', 'us'] },
      },
      { seq: 5, kind: 'status', payload: { status: 'waiting_input' } },
    ]);

    expect(waiting.tools).toEqual([]);
    expect(waiting.pending?.call_id).toBe('p1');
    expect(waiting.status).toBe('waiting_input');
    expect(todoProgress(waiting.todo)).toEqual({ current: 2, total: 3 });

    const answered = applyEvent(waiting, { seq: 6, kind: 'response', payload: { call_id: 'p1' } });
    expect(answered.pending).toBeNull();
  });

  it('names the memory a remember result wrote, for the chat to undo', () => {
    const result = JSON.stringify([
      { type: 'json', json: { remembered: { id: 'm1', kind: 'preference', name: 'Time zone' } } },
    ]);

    expect(memoryChange(result)).toEqual({ action: 'remembered', id: 'm1', name: 'Time zone' });
    expect(memoryChange('not json')).toBeNull();

    const view = fold([
      { seq: 1, kind: 'tool_start', payload: { id: 'r1', name: 'remember', arguments: {} } },
      { seq: 2, kind: 'tool_end', payload: { id: 'r1', name: 'remember', result, truncated: false, is_error: false } },
      { seq: 3, kind: 'done', payload: { status: 'succeeded', content: 'Noted.' } },
    ]);

    expect(view.memories).toEqual([{ action: 'remembered', id: 'm1', name: 'Time zone' }]);
    expect(view.done?.content).toBe('Noted.');
    expect(view.status).toBe('succeeded');
  });

  it('has no progress without a task list, and counts time from the start', () => {
    expect(todoProgress([])).toBeNull();
    expect(fold([]).startedAtMicros).toBe(2);
    expect(fold([]).initiatorId).toBe('user-1');
  });
});
