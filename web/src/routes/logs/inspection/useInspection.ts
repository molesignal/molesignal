import * as React from 'react';

import * as chatApi from '@/api/agent/chat';
import type { ModelProvider } from '@/api/agent/modelProviders';
import { cancelRun, followRun } from '@/api/agent/runs';
import { toApiError } from '@/lib/http';

/** A tool the model called while it inspected, and how the call ended. */
export interface InspectionTool {
  id: string;
  name: string;
  status: 'running' | 'done' | 'error';
}

export interface InspectionRun {
  status: 'idle' | 'running' | 'done' | 'failed' | 'stopped';
  /** The report as it has arrived so far. */
  report: string;
  tools: InspectionTool[];
  error: string | null;
  /** The Mole Agent chat the inspection is kept in, once it has one. */
  chatId: string | null;
}

interface InspectionRequest {
  /** The model the inspection is run by. */
  provider: ModelProvider;
  title: string;
  prompt: string;
  stream: string;
  range: { start: number; end: number };
}

const IDLE: InspectionRun = { status: 'idle', report: '', tools: [], error: null, chatId: null };

export interface LogInspection {
  run: InspectionRun;
  start: (request: InspectionRequest) => void;
  stop: () => void;
  reset: () => void;
}

/**
 * An inspection of logs by the model, as a Mole Agent chat of its own: the
 * report arrives as it is written, the tools the model calls are listed as it
 * calls them, and the chat stays in Mole Agent to be gone on with.
 *
 * It is kept by the page, not by the view that shows it: a reader who looks at
 * the logs while the model is at work finds the report where they left it.
 */
export function useLogInspection(): LogInspection {
  const [run, setRun] = React.useState<InspectionRun>(IDLE);
  const controller = React.useRef<AbortController | null>(null);
  // The run the inspection is answered in: stopping it stops the model on the server too.
  const runId = React.useRef<string | null>(null);

  // A page that is left stops reading; the answer goes on and stays in Mole Agent.
  React.useEffect(() => () => controller.current?.abort(), []);

  const stop = React.useCallback(() => {
    controller.current?.abort();
    controller.current = null;
    if (runId.current) void cancelRun(runId.current).catch(() => undefined);
    runId.current = null;
    setRun((current) => (current.status === 'running' ? { ...current, status: 'stopped' } : current));
  }, []);
  const reset = React.useCallback(() => {
    controller.current?.abort();
    controller.current = null;
    setRun(IDLE);
  }, []);
  const start = React.useCallback(({ provider, title, prompt, stream, range }: InspectionRequest) => {
    controller.current?.abort();

    const abort = new AbortController();
    controller.current = abort;
    setRun({ ...IDLE, status: 'running' });

    // Only the run that is current writes to the page: one that was stopped or replaced is done with.
    const update = (change: (current: InspectionRun) => InspectionRun) => {
      if (controller.current === abort) setRun(change);
    };

    void (async () => {
      let failed = false;

      try {
        const chat = await chatApi.createChat({
          provider: provider.provider,
          model: provider.default_model,
          provider_id: provider.id,
          title,
        });

        update((current) => ({ ...current, chatId: chat.id }));
        const posted = await chatApi.postMessage(chat.id, {
          content: prompt,
          time_range: { start_micros: range.start, end_micros: range.end },
          // It reads and reports: nothing it finds is acted on.
          execution_policy: 'read_only',
          stream_hints: [stream],
          provider_id: provider.id,
          model: provider.default_model,
        });

        if (posted.kind !== 'queued') {
          failed = true;
          const message =
            posted.kind === 'error'
              ? posted.message
              : posted.kind === 'budget'
                ? 'budget'
                : posted.refusal.code;
          update((current) => ({ ...current, status: 'failed', error: message }));
          return;
        }

        runId.current = posted.run.id;

        // The report is written as the model writes it; its last word is the end event's.
        await followRun(
          posted.run.id,
          (event) => {
            if (event.kind === 'text') {
              update((current) => ({ ...current, report: current.report + event.payload.text }));
            } else if (event.kind === 'tool_start') {
              const { id, name } = event.payload;
              update((current) => ({
                ...current,
                report: '',
                tools: [...current.tools, { id, name, status: 'running' }],
              }));
            } else if (event.kind === 'tool_end') {
              const { id, is_error: isError } = event.payload;
              update((current) => ({
                ...current,
                tools: current.tools.map((item) =>
                  item.id === id ? { ...item, status: isError ? 'error' : 'done' } : item,
                ),
              }));
            } else if (event.kind === 'done') {
              const done = event.payload;
              if (done.status === 'succeeded') {
                update((current) => ({ ...current, report: done.content ?? current.report }));
              } else if (done.status !== 'cancelled') {
                failed = true;
                update((current) => ({
                  ...current,
                  status: 'failed',
                  error: done.error ?? done.code ?? done.status,
                }));
              }
            }
          },
          abort.signal,
        );
      } catch (error) {
        failed = true;
        update((current) => ({ ...current, status: 'failed', error: toApiError(error).message }));
      }

      if (!failed) update((current) => (current.status === 'running' ? { ...current, status: 'done' } : current));
    })();
  }, []);

  return { run, start, stop, reset };
}
