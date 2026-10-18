import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';

import type { Chat } from '@/api/agent/chat';
import {
  cancelRun,
  followRun,
  isOver,
  listChatRuns,
  respondRun,
  type RespondInput,
  type Run,
  type RunNotice,
} from '@/api/agent/runs';

import { applyEvent, emptyView, type RunView } from './runState';

export interface ChatRun {
  /** The run being followed: the earliest of the chat that has not ended. */
  view: RunView | null;
  /** The kind of the run being followed: an answer, or a compaction. */
  kind: Run['kind'] | null;
  /** A run of the chat is answering or waiting its turn. */
  busy: boolean;
  /** Questions whose answer waits behind the one being written. */
  queuedMessageIds: ReadonlySet<string>;
  /** Questions whose answer stopped without one: stopped, or its server went away. */
  unansweredMessageIds: ReadonlyMap<string, Run['status']>;
  /** Start following a run just queued, unless an earlier one goes first. */
  follow: (run: Run) => void;
  /** Stop the answer being written; the server stops calling the model and tools. */
  stop: () => Promise<void>;
  respond: (input: RespondInput) => Promise<void>;
}

const ANSWERING_KINDS: ReadonlySet<Run['kind']> = new Set(['answer', 'compact']);

/** The earliest run of the chat that has not ended: the one answering, or the first in line. */
export function nextRun(runs: readonly Run[]): Run | null {
  return (
    [...runs]
      .filter((run) => ANSWERING_KINDS.has(run.kind) && !isOver(run.status))
      .sort((a, b) => a.created_at_micros - b.created_at_micros)[0] ?? null
  );
}

/**
 * The answers of a chat as runs. Opening a chat picks up a run still answering
 * (written by this person in another tab, or by a teammate in a team chat) and
 * reads it from its first event; a dropped connection reads on from the last.
 * Sending while an answer is written queues the next one behind it.
 */
/** Where the runs of a chat are kept: named by the run's own chat, never by the chat on screen. */
function runsKeyOf(chatId: string | null) {
  return ['agent', 'chat-runs', chatId] as const;
}

export function useChatRun(
  chatId: string | null,
  onFinished: () => Promise<unknown>,
  /** Something said along the way that outlasts the run, such as the budget. */
  onNotice?: (notice: RunNotice) => void,
): ChatRun {
  const queryClient = useQueryClient();
  const runsQ = useQuery({
    queryKey: runsKeyOf(chatId),
    queryFn: () => listChatRuns(chatId as string),
    enabled: Boolean(chatId),
    retry: false,
  });
  const [view, setView] = React.useState<RunView | null>(null);
  const [kind, setKind] = React.useState<Run['kind'] | null>(null);
  const followed = React.useRef<{ id: string; abort: AbortController } | null>(null);
  const finished = React.useRef(onFinished);
  finished.current = onFinished;
  const noticed = React.useRef(onNotice);
  noticed.current = onNotice;

  const release = React.useCallback(() => {
    followed.current?.abort.abort();
    followed.current = null;
    setView(null);
    setKind(null);
  }, []);

  // Another chat on screen: what was followed is not this chat's.
  React.useEffect(() => release, [chatId, release]);

  const start = React.useCallback(
    (run: Run) => {
      if (followed.current?.id === run.id) return;
      followed.current?.abort.abort();
      const abort = new AbortController();
      followed.current = { id: run.id, abort };
      setView(emptyView(run));
      setKind(run.kind);

      void (async () => {
        const ended = await followRun(
          run.id,
          (event) => {
            if (followed.current?.id !== run.id) return;
            if (event.kind === 'notice' && event.payload.kind === 'title') {
              const { title } = event.payload;
              queryClient.setQueryData<Chat[]>(['agent', 'chats'], (chats) =>
                chats?.map((chat) => (chat.id === run.chat_id ? { ...chat, title } : chat)),
              );
            }
            if (event.kind === 'notice') noticed.current?.(event.payload);
            setView((current) =>
              current?.runId === run.id ? applyEvent(current, event) : current,
            );
          },
          abort.signal,
        );
        if (!ended || followed.current?.id !== run.id) return;
        // The answer is kept by then: show it in the chat before the live copy goes. The
        // first answer of a new chat is followed from before the chat had an id on screen,
        // so its runs are read again by the run's chat.
        await finished.current();
        await queryClient.invalidateQueries({ queryKey: runsKeyOf(run.chat_id) });
        if (followed.current?.id === run.id) {
          followed.current = null;
          setView(null);
          setKind(null);
        }
      })();
    },
    [queryClient],
  );

  // Follow the earliest run that has not ended, once nothing else is followed.
  const runs = runsQ.data ?? [];
  const next = nextRun(runs);
  React.useEffect(() => {
    if (next && !followed.current) start(next);
  }, [next, start]);

  const follow = React.useCallback(
    (run: Run) => {
      if (!followed.current) start(run);
      void queryClient.invalidateQueries({ queryKey: runsKeyOf(run.chat_id) });
    },
    [queryClient, start],
  );

  const stop = React.useCallback(async () => {
    const current = followed.current;
    if (!current) return;
    await cancelRun(current.id);
  }, []);

  const respond = React.useCallback(async (input: RespondInput) => {
    const current = followed.current;
    if (!current) return;
    await respondRun(current.id, input);
  }, []);

  const queuedMessageIds = new Set(
    runs
      .filter(
        (run) =>
          run.kind === 'answer' &&
          run.status === 'queued' &&
          run.id !== view?.runId &&
          run.user_message_id,
      )
      .map((run) => run.user_message_id as string),
  );

  const unansweredMessageIds = new Map(
    runs
      .filter(
        (run) =>
          run.kind === 'answer' &&
          (run.status === 'interrupted' || run.status === 'cancelled') &&
          !run.assistant_message_id &&
          run.user_message_id,
      )
      .map((run) => [run.user_message_id as string, run.status] as const),
  );

  const busy = Boolean(view && !view.done) || Boolean(next);

  return { view, kind, busy, queuedMessageIds, unansweredMessageIds, follow, stop, respond };
}
