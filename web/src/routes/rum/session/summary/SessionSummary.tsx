import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Sparkles } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumSessionSummary } from '@/api/rum';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { Skeleton } from '@/shell/ui/skeleton';

import { formatClock } from '../../replay/clock';
import type { ReplayController } from '../../replay/controller';
import { RumSectionHeader } from '../../RumLayout';

/**
 * What a model makes of the session: a few sentences, and the chapters of its replay, each of which
 * moves the replay to where it begins. Nothing is asked of the model until a person asks: the answer
 * counts against the budget of Mole Agent, is kept, and the same events are not asked about again.
 */
export function SessionSummary({
  session,
  summary,
  loading,
  range,
  controller,
  zero,
  queryKey,
}: {
  session: string;
  summary: RumSessionSummary | null | undefined;
  loading: boolean;
  range: { from: number; to: number } | undefined;
  controller: ReplayController;
  /** Where the ruler starts, in microseconds. */
  zero: number;
  /** What the page read the summary under. */
  queryKey: readonly unknown[];
}) {
  const { t } = useTranslation('rum');
  const client = useQueryClient();
  const access = useActionAccess({ permission: 'rum.replay.summaries.generate' });

  const generate = useMutation({
    mutationFn: (regenerate: boolean) => rumApi.generateSessionSummary(session, { regenerate, range }),
    onSuccess: (answer) => client.setQueryData([...queryKey], answer),
  });

  if (loading) {
    return (
      <section>
        <RumSectionHeader title={t('session_detail.summary.title')} />
        <div className="mt-3 flex flex-col gap-2">
          <Skeleton className="h-3 w-full" />
          <Skeleton className="h-3 w-2/3" />
        </div>
      </section>
    );
  }

  return (
    <section data-rum-summary={summary ? 'written' : 'none'}>
      <RumSectionHeader
        title={t('session_detail.summary.title')}
        action={
          access.disabled ? null : (
            <ChromeButton
              size="sm"
              variant="ghost"
              data-rum-summary-generate=""
              disabled={generate.isPending}
              onClick={() => generate.mutate(Boolean(summary))}
            >
              <Sparkles aria-hidden className="h-3.5 w-3.5" />
              {t(generate.isPending ? 'session_detail.summary.generating' : summary ? 'session_detail.summary.regenerate' : 'session_detail.summary.generate')}
            </ChromeButton>
          )
        }
      />
      {generate.isError && (
        <p role="alert" className="m-0 mt-3 rounded-md bg-red-dim px-3 py-2 font-sans text-xs text-red-soft">
          {toApiError(generate.error).message}
        </p>
      )}
      {summary ? (
        <div className="mt-3 space-y-3">
          <p data-rum-summary-text="" className="m-0 whitespace-pre-wrap font-sans text-xs leading-relaxed text-tx-0">
            {summary.summary}
          </p>
          {summary.chapters.length > 0 && (
            <ol aria-label={t('session_detail.summary.chapters')} className="m-0 list-none space-y-1 p-0">
              {summary.chapters.map((chapter) => (
                <li key={`${chapter.atTime}-${chapter.title}`}>
                  <button
                    type="button"
                    data-rum-chapter=""
                    title={chapter.summary}
                    aria-label={t('session_detail.tools.seek', { time: formatClock((chapter.atTime - zero) / 1_000) })}
                    onClick={() => controller.seekToMicros(chapter.atTime)}
                    className="flex w-full items-baseline gap-2 rounded-sm border-0 bg-transparent px-1 py-0.5 text-left font-sans text-xs text-tx-1 outline-none hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0"
                  >
                    <span className="shrink-0 font-mono text-tx-3">{formatClock((chapter.atTime - zero) / 1_000)}</span>
                    <span className="min-w-0 truncate">{chapter.title}</span>
                  </button>
                </li>
              ))}
            </ol>
          )}
          <p className="m-0 font-sans text-xs text-tx-3">
            {t('session_detail.summary.by', {
              model: summary.model,
              tokens: summary.promptTokens + summary.completionTokens,
            })}
          </p>
        </div>
      ) : (
        <p className="m-0 mt-3 font-sans text-xs text-tx-3">{t('session_detail.summary.none')}</p>
      )}
    </section>
  );
}
