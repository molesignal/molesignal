import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as qualityApi from '@/api/agent/quality';
import type { Review } from '@/api/agent/quality';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ChromeButton } from '@/shell/chrome';
import { FormTextarea } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';

import { QUALITY_KEY } from './state';

const STATUS_INK: Record<qualityApi.ReviewStatus, string> = {
  pending: 'text-[var(--orange-ink)]',
  accepted: 'text-[var(--red-ink)]',
  rejected: 'text-[var(--green-ink)]',
};

/** How a review stands, as a coloured word. */
export function ReviewStatusWord({ status }: { status: qualityApi.ReviewStatus }) {
  const { t } = useTranslation('agent');

  return (
    <span data-review-status={status} className={cn('font-strong', STATUS_INK[status])}>
      {t(`quality.reviews.status.${status}`)}
    </span>
  );
}

/**
 * An answer waiting for review: why it is here, and the reviewer's decision —
 * accept it (the answer is wrong; it joins the regression set the prompt check
 * replays) or reject it (the answer is fine), with a note on what a good
 * answer gets right.
 */
export function ReviewPanel({ review }: { review: Review }) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [note, setNote] = React.useState(review.note);

  React.useEffect(() => setNote(review.note), [review.id, review.note]);

  const decide = useMutation({
    mutationFn: (status: 'accepted' | 'rejected') => qualityApi.decideReview(review.id, status, note),
    onSuccess: (decided) => {
      toast.success(t(`quality.reviews.decided.${decided.status}`));
      void queryClient.invalidateQueries({ queryKey: QUALITY_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const pending = review.status === 'pending';

  return (
    <section data-review={review.id} className="flex flex-col gap-2 font-sans text-xs">
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <ReviewStatusWord status={review.status} />
        <span className="text-tx-2">{t(`quality.reviews.causes.${review.cause}`)}</span>
        {review.decidedAt && (
          <span className="text-tx-3">{formatMicrosActive(review.decidedAt)}</span>
        )}
      </div>
      <FormTextarea
        aria-label={t('quality.reviews.note')}
        placeholder={t('quality.reviews.note_placeholder')}
        value={note}
        onChange={(event) => setNote(event.target.value)}
        className="min-h-20 text-xs"
        maxLength={2000}
      />
      <div className="flex flex-wrap items-center gap-2">
        <ChromeButton
          {...(pending ? { variant: 'primary' as const } : {})}
          disabled={decide.isPending}
          onClick={() => decide.mutate('accepted')}
        >
          {t('quality.reviews.accept')}
        </ChromeButton>
        <ChromeButton disabled={decide.isPending} onClick={() => decide.mutate('rejected')}>
          {t('quality.reviews.reject')}
        </ChromeButton>
        <span className="text-tx-3">{t('quality.reviews.accept_hint')}</span>
      </div>
    </section>
  );
}
