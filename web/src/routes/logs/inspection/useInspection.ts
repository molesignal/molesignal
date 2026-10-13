import * as React from 'react';

import * as chatApi from '@/api/agent/chat';
import type { ModelProvider } from '@/api/agent/modelProviders';
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

  // A page that is left gives up what it was waiting for.
  React.useEffect(() => () => controller.current?.abort(), []);

  const stop = React.useCallback(() => {
    controller.current?.abort();
    controller.current = null;
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
        await chatApi.postMessageStream(
          chat.id,
          {
            content: prompt,
            time_range: { start_micros: range.start, end_micros: range.end },
            // It reads and reports: nothing it finds is acted on.
            execution_policy: 'read_only',
            stream_hints: [stream],
            provider_id: provider.id,
            model: provider.default_model,
          },
          {
            onChunk: (text) => update((current) => ({ ...current, report: current.report + text })),
            onToolStart: (tool) =>
              update((current) => ({
                ...current,
                tools: [...current.tools, { id: tool.id, name: tool.name, status: 'running' }],
              })),
            onToolEnd: (tool) =>
              update((current) => ({
                ...current,
                tools: current.tools.map((item) =>
                  item.id === tool.id ? { ...item, status: tool.is_error ? 'error' : 'done' } : item,
                ),
              })),
            onError: (message) => {
              failed = true;
              update((current) => ({ ...current, status: 'failed', error: message }));
            },
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
