import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ConfirmationCard, QuestionCard } from './RunCards';
import { MemoryChanges, RunNotices, TodoProgress } from './RunExtras';
import type { ChatRun } from './useChatRun';
import { formatStreamErrorContent } from '../../messageRenderState';
import { fallbackToolLabel } from '../../settings/presentation';
import { type ChatUserIdentity, MessageBubble } from '../transcript/MessageBubble';

/** What the answer being written shows: on a failure the reason in place of an answer. */
function liveContent(chatRun: ChatRun): string {
  const done = chatRun.view?.done;
  if (done?.status === 'failed') {
    return formatStreamErrorContent(done.error ?? done.code ?? 'failed');
  }
  if (done?.status === 'succeeded') return done.content ?? chatRun.view?.text ?? '';
  return chatRun.view?.text ?? '';
}

/**
 * The answer being written, read from its run: the text so far, the tools it
 * called, the task list, and a question or a confirmation it waits on. A
 * compaction shows only that it is under way.
 */
export function LiveRun({
  chatRun,
  user,
  viewerId,
  initiatorName,
}: {
  chatRun: ChatRun;
  user: ChatUserIdentity;
  /** Who is reading: only the person who asked decides a confirmation. */
  viewerId: string;
  initiatorName: (id: string) => string;
}) {
  const { t } = useTranslation('agent');
  const view = chatRun.view;
  if (!view) return null;

  if (chatRun.kind === 'compact') {
    return (
      <div data-agent-compacting className="ml-10 flex items-center gap-2 text-xs text-tx-2">
        <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin text-indigo" />
        {t('run.notice.compacting')}
      </div>
    );
  }

  const writing = !view.done;
  const pending = view.pending;
  const toolLabel = (name: string) => {
    const key = `tools.${name}.title`;
    const translated = t(key);
    return translated === key ? fallbackToolLabel(name) : translated;
  };

  return (
    <div className="flex flex-col gap-3" data-agent-live-run={view.runId} data-run-status={view.status}>
      <MessageBubble
        role="assistant"
        content={liveContent(chatRun)}
        t={t}
        user={user}
        streaming={writing && !pending}
        liveTools={view.tools}
        investigationStartedAtMicros={view.startedAtMicros}
      />
      <TodoProgress items={view.todo} />
      {pending?.type === 'question' && (
        <QuestionCard key={pending.call_id} pending={pending} onRespond={chatRun.respond} />
      )}
      {pending?.type === 'confirmation' && (
        <ConfirmationCard
          key={pending.call_id}
          pending={pending}
          canDecide={view.initiatorId === viewerId}
          initiatorName={initiatorName(view.initiatorId)}
          toolLabel={toolLabel(pending.tool)}
          onRespond={chatRun.respond}
        />
      )}
      <MemoryChanges changes={view.memories} />
      <RunNotices notices={view.notices} />
    </div>
  );
}
