import type { TFunction } from 'i18next';
import { ChevronLeft, ChevronRight, FileCheck2, Pin, PinOff, Split } from 'lucide-react';
import * as React from 'react';

import { writeIncidentRca } from '@/api/agent/chat';
import type { AgentFeedback } from '@/api/agent/feedback';
import type { RunStatus } from '@/api/agent/runs';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { toast } from '@/shell/ui/sonner';

import { CopyMessage, MessageAction, QuestionActions } from './MessageActions';
import { type ChatUserIdentity, isStreamErrorContent, MessageBubble } from './MessageBubble';
import { MessageTime } from './MessageTime';
import { FeedbackControl } from '../../feedback/FeedbackControl';
import type { ConversationTurn } from '../../messageRenderState';
import { answerMarkdown, exportLabels } from '../composer/exportChat';

/**
 * A question with its answers: the versions of a regenerated answer and their
 * rating. The question can be copied, given back to the composer to be changed
 * and sent again, or answered again; an answer can be copied. An answer's
 * actions share one line with the fold of the tools it called.
 */
export function ConversationTurnView({
  turn,
  t,
  user,
  chatId,
  feedback,
  canRegenerate,
  regenerating,
  queued = false,
  unanswered = null,
  incidentChat = false,
  onRegenerate,
  onEdit,
  pinnedIds,
  onTogglePin,
  onFork,
}: {
  turn: ConversationTurn;
  t: TFunction<'agent'>;
  user: ChatUserIdentity;
  chatId: string | null;
  feedback: ReadonlyMap<string, AgentFeedback>;
  canRegenerate: boolean;
  regenerating: boolean;
  /** The question waits its turn behind an answer being written. */
  queued?: boolean;
  /** The answer stopped without one: stopped, or its server went away. */
  unanswered?: RunStatus | null;
  /** The chat investigates an incident: an answer can be written back as its RCA. */
  incidentChat?: boolean;
  onRegenerate: () => void;
  /** Gives the question back to the composer, with what it carried. */
  onEdit: () => void;
  /** The answers pinned at the top of the chat. */
  pinnedIds: readonly string[];
  onTogglePin: (messageId: string) => void;
  /** Goes on from an answer in a new chat holding everything up to it. */
  onFork: (messageId: string) => void;
}) {
  const rcaAccess = useActionAccess({ permission: 'alerts.manage' });
  const [writingRca, setWritingRca] = React.useState(false);
  const [answerIndex, setAnswerIndex] = React.useState(
    Math.max(0, turn.answers.length - 1),
  );
  React.useEffect(() => {
    setAnswerIndex(Math.max(0, turn.answers.length - 1));
  }, [turn.answers.length]);
  const answer = turn.answers[answerIndex];
  const hasVersions = turn.answers.length > 1;
  // In a team chat a question can come from someone else: it carries their name.
  const asker =
    turn.user.sender_id && user.id && turn.user.sender_id !== user.id
      ? (turn.user.sender_name ?? null)
      : null;
  // A persisted answer can be rated; a failure to answer cannot.
  const canRate = Boolean(answer && chatId && !isStreamErrorContent(answer.content));
  return (
    <div className="flex flex-col gap-3" data-testid="conversation-turn">
      <MessageBubble
        role="user"
        content={turn.user.content}
        command={turn.user.command}
        t={t}
        user={asker ? { displayName: asker, avatarUrl: '' } : user}
        {...(asker ? { senderName: asker } : {})}
        createdAtMicros={turn.user.created_at_micros}
        references={turn.user.references ?? []}
        queued={queued}
        actions={
          <QuestionActions
            text={turn.user.content}
            t={t}
            canRetry={canRegenerate}
            retrying={regenerating}
            onEdit={onEdit}
            onRetry={onRegenerate}
          />
        }
      />
      {!answer && unanswered && (
        <p data-agent-unanswered={unanswered} className="ml-10 text-xs text-tx-2">
          {t(`run.ended.${unanswered === 'interrupted' ? 'interrupted' : 'cancelled'}`)}
        </p>
      )}
      {answer && (
        <MessageBubble
          role="assistant"
          content={answer.content}
          t={t}
          user={user}
          createdAtMicros={answer.created_at_micros}
          evidence={answer.evidence_json}
          investigationStartedAtMicros={turn.user.created_at_micros}
          messageId={answer.id}
          rating={
            canRate &&
            chatId && (
              <FeedbackControl
                kind="chat_message"
                targetId={answer.id}
                chatId={chatId}
                value={feedback.get(answer.id)}
                align="end"
              />
            )
          }
          actions={
            (hasVersions || canRate) && (
              <>
                {hasVersions && (
                  <>
                    <button
                      type="button"
                      aria-label={t('answer_versions.previous')}
                      disabled={answerIndex === 0}
                      onClick={() => setAnswerIndex((current) => Math.max(0, current - 1))}
                      className="grid h-7 w-7 place-items-center rounded-md text-tx-3 hover:bg-bg-2 hover:text-tx-0 disabled:opacity-35"
                    >
                      <ChevronLeft className="h-3.5 w-3.5" />
                    </button>
                    <span className="inline-flex h-7 items-center rounded-md px-0.5 text-xs tabular-nums text-tx-3">
                      {t('answer_versions.position')
                        .replace('{{current}}', String(answerIndex + 1))
                        .replace('{{total}}', String(turn.answers.length))}
                    </span>
                    <button
                      type="button"
                      aria-label={t('answer_versions.next')}
                      disabled={answerIndex >= turn.answers.length - 1}
                      onClick={() =>
                        setAnswerIndex((current) =>
                          Math.min(turn.answers.length - 1, current + 1),
                        )
                      }
                      className="grid h-7 w-7 place-items-center rounded-md text-tx-3 hover:bg-bg-2 hover:text-tx-0 disabled:opacity-35"
                    >
                      <ChevronRight className="h-3.5 w-3.5" />
                    </button>
                  </>
                )}
                {canRate && (
                  <CopyMessage text={answerMarkdown(answer.content, exportLabels(t))} t={t} />
                )}
                {canRate && (
                  <>
                    <MessageAction label={t('message_actions.fork')} onClick={() => onFork(answer.id)}>
                      {/* One line splitting in two, both arrows to the right. */}
                      <Split aria-hidden="true" className="rotate-90" />
                    </MessageAction>
                    <MessageAction
                      label={
                        pinnedIds.includes(answer.id)
                          ? t('message_actions.unpin')
                          : t('message_actions.pin')
                      }
                      onClick={() => onTogglePin(answer.id)}
                    >
                      {pinnedIds.includes(answer.id) ? (
                        <PinOff aria-hidden="true" />
                      ) : (
                        <Pin aria-hidden="true" />
                      )}
                    </MessageAction>
                  </>
                )}
                {incidentChat && canRate && chatId && rcaAccess.allowed && (
                  <button
                    type="button"
                    disabled={writingRca}
                    onClick={() => {
                      setWritingRca(true);
                      writeIncidentRca(chatId, answer.id)
                        .then(() => toast.success(t('investigate.rca_written')))
                        .catch((error: unknown) => toast.error(toApiError(error).message))
                        .finally(() => setWritingRca(false));
                    }}
                    className="inline-flex h-7 items-center gap-1 rounded-md px-2 text-xs font-strong text-tx-3 hover:bg-bg-2 hover:text-tx-0 disabled:opacity-50"
                  >
                    <FileCheck2 className="h-3 w-3" />
                    {t('investigate.write_rca')}
                  </button>
                )}
                <MessageTime
                  micros={answer.created_at_micros}
                  className="type-micro ml-1.5 text-tx-3"
                />
              </>
            )
          }
        />
      )}
    </div>
  );
}
