import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { SPAN_SHARE_FILL, SPAN_SHARE_STEPS } from './model';

/**
 * The key to the colours of the bars: the steps of a span's share of the trace,
 * from none of it to all of it.
 */
export function ShareKey({ className }: { className?: string }) {
  const { t } = useTranslation('traces');
  const [, low, mid, high] = SPAN_SHARE_STEPS.map((step) => step.from);

  return (
    <span
      role="img"
      aria-label={t('detail.share_key_aria', { low, mid, high })}
      data-testid="trace-share-key"
      className={cn('flex shrink-0 items-center gap-1.5 font-sans text-xs font-body text-tx-3', className)}
    >
      <span>{t('detail.share_key')}</span>
      <span className="font-mono tabular-nums">0</span>
      <span className="flex gap-px">
        {SPAN_SHARE_STEPS.map((step) => (
          <span
            key={step.share}
            className={cn('h-[6px] w-[14px] first:rounded-l-[2px] last:rounded-r-[2px]', SPAN_SHARE_FILL[step.share])}
          />
        ))}
      </span>
      <span className="font-mono tabular-nums">100%</span>
    </span>
  );
}
