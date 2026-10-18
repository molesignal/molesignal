import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ThumbsDown, ThumbsUp } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as feedbackApi from '@/api/agent/feedback';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { toast } from '@/shell/ui/sonner';
import { Textarea } from '@/shell/ui/textarea';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { feedbackKindKey } from './useFeedback';

const COMMENT_LIMIT = 2_000;

/**
 * 👍 / 👎 on something Mole Agent produced. A thumb that is already pressed takes
 * the rating back; 👎 asks what was wrong before it is saved.
 */
export function FeedbackControl({
  kind,
  targetId,
  chatId,
  value,
  className,
  align = 'start',
}: {
  kind: feedbackApi.FeedbackTargetKind;
  targetId: string;
  /** The chat an answer belongs to; needed to rate a chat answer. */
  chatId?: string | undefined;
  value: feedbackApi.AgentFeedback | undefined;
  className?: string | undefined;
  /** Which edge of the thumbs the "what was wrong" card lines up with. */
  align?: 'start' | 'end';
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [open, setOpen] = React.useState(false);
  const [reasons, setReasons] = React.useState<feedbackApi.FeedbackReason[]>([]);
  const [comment, setComment] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    setReasons(value?.rating === 'down' ? value.reasons : []);
    setComment(value?.rating === 'down' ? value.comment : '');
  }, [open, value]);

  const refresh = () => queryClient.invalidateQueries({ queryKey: feedbackKindKey(kind) });

  const save = useMutation({
    mutationFn: (input: Omit<feedbackApi.FeedbackInput, 'target_kind' | 'target_id' | 'chat_id'>) =>
      feedbackApi.putFeedback({
        ...input,
        target_kind: kind,
        target_id: targetId,
        chat_id: chatId,
      }),
    onSuccess: async (saved) => {
      await refresh();
      setOpen(false);
      if (saved.rating === 'down') toast.success(t('feedback.thanks'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const withdraw = useMutation({
    mutationFn: () => feedbackApi.deleteFeedback(kind, targetId),
    onSuccess: async () => {
      await refresh();
      setOpen(false);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const busy = save.isPending || withdraw.isPending;
  const rating = value?.rating;

  const pressUp = () => {
    if (rating === 'up') withdraw.mutate();
    else save.mutate({ rating: 'up' });
  };

  const toggleReason = (reason: feedbackApi.FeedbackReason) =>
    setReasons((current) =>
      current.includes(reason)
        ? current.filter((item) => item !== reason)
        : [...current, reason],
    );

  return (
    <div
      data-agent-feedback={kind}
      data-agent-feedback-rating={rating ?? 'none'}
      className={cn('inline-flex items-center gap-0.5', className)}
    >
      <Thumb
        label={t(rating === 'up' ? 'feedback.withdraw_up' : 'feedback.up')}
        pressed={rating === 'up'}
        tone="up"
        disabled={busy}
        onClick={pressUp}
      >
        <ThumbsUp />
      </Thumb>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Thumb
            label={t('feedback.down')}
            pressed={rating === 'down'}
            tone="down"
            disabled={busy}
          >
            <ThumbsDown />
          </Thumb>
        </PopoverTrigger>
        <PopoverContent align={align} className="w-80" data-agent-feedback-form>
          <form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              save.mutate({ rating: 'down', reasons, comment: comment.trim() });
            }}
          >
            <p className="font-sans text-xs font-strong text-tx-0">{t('feedback.what_was_wrong')}</p>
            <div className="flex flex-wrap gap-1.5" role="group" aria-label={t('feedback.reasons_label')}>
              {feedbackApi.FEEDBACK_REASONS.map((reason) => {
                const selected = reasons.includes(reason);
                return (
                  <button
                    key={reason}
                    type="button"
                    aria-pressed={selected}
                    onClick={() => toggleReason(reason)}
                    className={cn(
                      'rounded-md px-2 py-1 font-sans text-xs font-strong transition-colors',
                      selected
                        ? 'bg-red-dim text-[var(--red-ink)]'
                        : 'bg-[var(--control-surface)] text-tx-2 hover:bg-bg-3 hover:text-tx-0',
                    )}
                  >
                    {t(`feedback.reasons.${reason}`)}
                  </button>
                );
              })}
            </div>
            <Textarea
              value={comment}
              maxLength={COMMENT_LIMIT}
              onChange={(event) => setComment(event.target.value)}
              placeholder={t('feedback.comment_placeholder')}
              aria-label={t('feedback.comment_label')}
              className="min-h-16 font-sans text-xs"
            />
            <div className="flex items-center gap-2">
              {rating === 'down' && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={busy}
                  onClick={() => withdraw.mutate()}
                >
                  {t('feedback.withdraw')}
                </Button>
              )}
              <span className="flex-1" />
              <Button type="button" variant="outline" size="sm" onClick={() => setOpen(false)}>
                {t('feedback.cancel')}
              </Button>
              <Button type="submit" size="sm" disabled={busy}>
                {t('feedback.submit')}
              </Button>
            </div>
          </form>
        </PopoverContent>
      </Popover>
    </div>
  );
}

const Thumb = React.forwardRef<
  HTMLButtonElement,
  {
    label: string;
    pressed: boolean;
    tone: 'up' | 'down';
    disabled?: boolean;
    onClick?: (() => void) | undefined;
    children: React.ReactNode;
  } & Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'children'>
>(({ label, pressed, tone, disabled, onClick, children, className, ...rest }, ref) => (
  <Tooltip>
    <TooltipTrigger asChild>
      <button
        ref={ref}
        type="button"
        aria-label={label}
        aria-pressed={pressed}
        disabled={disabled}
        onClick={onClick}
        className={cn(
          'grid h-7 w-7 place-items-center rounded-md transition-colors disabled:opacity-50 [&_svg]:h-3.5 [&_svg]:w-3.5',
          pressed && tone === 'up' && 'bg-[var(--green-tint)] text-[var(--green-ink)]',
          pressed && tone === 'down' && 'bg-red-dim text-[var(--red-ink)]',
          !pressed && 'text-tx-3 hover:bg-bg-2 hover:text-tx-0',
          className,
        )}
        {...rest}
      >
        {children}
      </button>
    </TooltipTrigger>
    <TooltipContent>{label}</TooltipContent>
  </Tooltip>
));
Thumb.displayName = 'FeedbackThumb';
