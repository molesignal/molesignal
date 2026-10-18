import type { TFunction } from 'i18next';
import { Pin } from 'lucide-react';

import type { ChatMessageRow } from '@/api/agent/chat';

import { parseStructuredAnswer } from '../../answer';
import { sanitizeAssistantContent } from '../../settings/presentation';

/** An answer in a line: its conclusion, or its first line of text without Markdown marks. */
export function answerHeadline(content: string): string {
  const clean = sanitizeAssistantContent(content);
  const structured = parseStructuredAnswer(clean);
  const text =
    structured?.summary ??
    clean
      .split('\n')
      .map((line) => line.replace(/^[#>*\-\d.\s`]+/, '').trim())
      .find(Boolean) ??
    '';
  return text.length > 60 ? `${text.slice(0, 60)}…` : text;
}

/**
 * The answers pinned in this chat, on a line over it that does not scroll: each
 * opens where it stands in the chat. Unpinned from the answer itself.
 */
export function PinnedAnswers({
  pinnedIds,
  messages,
  t,
  onJump,
}: {
  pinnedIds: readonly string[];
  messages: readonly ChatMessageRow[];
  t: TFunction<'agent'>;
  onJump: (messageId: string) => void;
}) {
  const pinned = pinnedIds
    .map((id) => messages.find((message) => message.id === id))
    .filter((message): message is ChatMessageRow => message !== undefined);
  if (pinned.length === 0) return null;
  return (
    <div
      data-agent-pinned-answers
      className="flex flex-wrap items-center gap-1.5 py-1.5"
    >
      <span className="inline-flex shrink-0 items-center gap-1 text-xs font-strong text-tx-2">
        <Pin aria-hidden="true" className="h-3.5 w-3.5" />
        {t('message_actions.pinned', { count: pinned.length })}
      </span>
      {pinned.map((message) => (
        <button
          key={message.id}
          type="button"
          onClick={() => onJump(message.id)}
          className="inline-flex h-7 min-w-0 max-w-[320px] items-center rounded-md bg-bg-2 px-2.5 text-xs text-tx-1 hover:bg-bg-3 hover:text-tx-0"
        >
          <span className="truncate">{answerHeadline(message.content)}</span>
        </button>
      ))}
    </div>
  );
}
