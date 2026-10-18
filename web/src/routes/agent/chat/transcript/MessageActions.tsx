import type { TFunction } from 'i18next';
import { Check, Copy, Pencil, RotateCcw } from 'lucide-react';
import * as React from 'react';

import { writeClipboardText } from '@/lib/clipboard';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

/** One action on a message: a sign named by its tooltip, as high as the rating thumbs. */
export function MessageAction({
  label,
  disabled = false,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          disabled={disabled}
          onClick={onClick}
          className="grid h-7 w-7 place-items-center rounded-md text-tx-3 transition-colors hover:bg-bg-2 hover:text-tx-0 disabled:opacity-50 [&_svg]:h-3.5 [&_svg]:w-3.5"
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/** Copies a message as it reads: the question as typed, an answer as Markdown. */
export function CopyMessage({ text, t }: { text: string; t: TFunction<'agent'> }) {
  const [copied, setCopied] = React.useState(false);
  const resetTimer = React.useRef<number | undefined>(undefined);

  React.useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  const copy = async () => {
    try {
      await writeClipboardText(text);
      setCopied(true);
      window.clearTimeout(resetTimer.current);
      resetTimer.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      toast.error(t('message_actions.copy_failed'));
    }
  };

  return (
    <MessageAction
      label={copied ? t('message_actions.copied') : t('message_actions.copy')}
      onClick={() => void copy()}
    >
      {copied ? <Check aria-hidden="true" className="text-[var(--green-ink)]" /> : <Copy aria-hidden="true" />}
    </MessageAction>
  );
}

/**
 * What can be done with a question: copy it, give it back to the composer to be
 * changed and sent again, or have it answered again. Shown while the question is
 * pointed at or holds the focus; always on a screen without a pointer that hovers.
 */
export function QuestionActions({
  text,
  t,
  canRetry,
  retrying,
  onEdit,
  onRetry,
}: {
  text: string;
  t: TFunction<'agent'>;
  /** False while an answer is being written in the chat: a new version waits for none. */
  canRetry: boolean;
  retrying: boolean;
  onEdit: () => void;
  onRetry: () => void;
}) {
  return (
    <span
      data-agent-question-actions
      className="flex items-center opacity-100 transition-opacity duration-fast group-focus-within/question:opacity-100 [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-hover/question:opacity-100"
    >
      <CopyMessage text={text} t={t} />
      <MessageAction label={t('message_actions.edit')} onClick={onEdit}>
        <Pencil aria-hidden="true" />
      </MessageAction>
      <MessageAction
        label={retrying ? t('answer_versions.regenerating') : t('message_actions.retry')}
        disabled={!canRetry || retrying}
        onClick={onRetry}
      >
        <RotateCcw aria-hidden="true" className={cn(retrying && 'animate-spin')} />
      </MessageAction>
    </span>
  );
}
