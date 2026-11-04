import { useTranslation } from 'react-i18next';

import type { RumIssueStatus } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { STATE_INK } from './model';

/** What is being done about an issue, said in a word of the ink of what it says. */
export function IssueState({ status, className }: { status: RumIssueStatus; className?: string | undefined }) {
  const { t } = useTranslation('rum');

  return (
    <span data-rum-issue-state={status} className={cn('shrink-0 font-sans text-sm font-strong', STATE_INK[status], className)}>
      {t(`errors.state.${status}`)}
    </span>
  );
}

const WIDTH = 80;
const HEIGHT = 22;
const MARGIN = 2;

/** How an issue went over the range, as a line: the same height scale for every point of one line. */
export function Sparkline({ values, className }: { values: readonly number[]; className?: string | undefined }) {
  const widest = Math.max(1, ...values);
  const step = values.length > 1 ? WIDTH / (values.length - 1) : 0;
  const points = values
    .map((value, index) => `${Math.round(index * step * 10) / 10},${Math.round((HEIGHT - MARGIN - (value / widest) * (HEIGHT - 2 * MARGIN)) * 10) / 10}`)
    .join(' ');

  return (
    <svg
      aria-hidden
      data-rum-trend=""
      width={WIDTH}
      height={HEIGHT}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      className={cn('block fill-none stroke-current', className)}
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <polyline points={points} />
    </svg>
  );
}
