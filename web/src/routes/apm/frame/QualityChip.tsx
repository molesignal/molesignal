import { AlertTriangle, Clock3 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ApmMeta } from '@/api/apm';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

/** How old the newest complete bucket may be before the data reads as behind. */
function staleAfterMicros(meta: ApmMeta): number {
  return meta.resolution === 'minute' ? 3 * 60 * 1_000_000 : 2 * 60 * 60 * 1_000_000;
}

export type QualityState = 'activation' | 'stale' | 'partial' | 'overflow' | null;

/** The one thing worth saying about the data of a page, the most pressing first; `null` when nothing is. */
export function qualityState(meta: ApmMeta, nowMicros = Date.now() * 1_000): QualityState {
  const stale = Boolean(meta.lastCompleteBucketAt && nowMicros - meta.lastCompleteBucketAt > staleAfterMicros(meta));

  if (meta.dataQuality.gaps.length > 0) return 'partial';
  if (stale) return 'stale';
  if (meta.activationBoundary) return 'activation';
  if (meta.dataQuality.overflowDimensions.length > 0) return 'overflow';
  return null;
}

/**
 * A word in the navigation card about the data in view, only while there is
 * something to say: data still to come, gaps the projection left, names
 * folded into "other" past the limits. The details open from it.
 */
export function ApmQualityChip({ meta }: { meta: ApmMeta | undefined }) {
  const { t } = useTranslation('apm');
  const time = useTimeFormatter();

  if (!meta) return null;

  const state = qualityState(meta);

  if (!state) return null;

  const Icon = state === 'activation' || state === 'stale' ? Clock3 : AlertTriangle;
  const warning = state === 'partial' || state === 'stale';

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-apm-quality={state}
          className={cn(
            'inline-flex h-7 items-center gap-1.5 rounded-md px-2 font-sans text-xs font-strong outline-none',
            warning
              ? 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)] hover:brightness-95 focus-visible:brightness-95'
              : 'bg-[var(--control-surface)] text-tx-2 hover:bg-bg-3 focus-visible:bg-bg-3',
          )}
        >
          <Icon aria-hidden className="h-3.5 w-3.5" />
          {t(`quality.${state}`)}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 space-y-2 p-3 font-sans text-xs text-tx-1">
        <p className="font-strong text-tx-0">{t(`quality.${state}_title`)}</p>
        <p className="leading-relaxed text-tx-2">
          {t(`quality.${state}_description`, {
            time: time.micros(state === 'activation' ? meta.projectionStartedAt : meta.lastCompleteBucketAt, false),
            count: meta.dataQuality.gaps.length,
          })}
        </p>
        {meta.dataQuality.overflowDimensions.length > 0 && (
          <div className="space-y-1">
            <p className="text-tx-2">{t('quality.overflow_dimensions')}</p>
            <div className="flex flex-wrap gap-1">
              {meta.dataQuality.overflowDimensions.map((dimension) => (
                <span key={dimension} className="rounded bg-bg-3 px-1.5 py-0.5 text-tx-1">
                  {t(`quality.dimension.${dimension}`, {
                    defaultValue: dimension,
                  })}
                </span>
              ))}
            </div>
          </div>
        )}
        {meta.lastCompleteBucketAt && (
          <p className="text-tx-3">
            {t('quality.complete_through', {
              time: time.micros(meta.lastCompleteBucketAt, false),
            })}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}
