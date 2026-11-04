import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { formatScore, scoreTone, type ScoreTone } from './model';

const INK: Record<ScoreTone, string> = {
  good: 'text-[var(--green-ink)]',
  fair: 'text-[var(--orange-ink)]',
  poor: 'text-[var(--red-ink)]',
};

/**
 * A score as the 0 to 100 a reader sees, coloured by how it stands against
 * the review threshold: no fill, the way numbers read in tables.
 */
export function ScoreWord({
  score,
  threshold,
  className,
}: {
  score: number | null | undefined;
  threshold: number;
  className?: string;
}) {
  const { t } = useTranslation('agent');

  if (score === null || score === undefined) {
    return <span className={cn('text-tx-3', className)}>{t('quality.no_score')}</span>;
  }

  const tone = scoreTone(score, threshold);

  return (
    <span data-score-tone={tone} className={cn('font-strong tabular-nums', INK[tone], className)}>
      {formatScore(score)}
    </span>
  );
}
