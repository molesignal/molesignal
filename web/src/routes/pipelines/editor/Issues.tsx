import { CircleAlert, CircleCheck, CircleDashed, TriangleAlert, type LucideIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import type { GraphIssue } from '../PipelineGraph/validation';

type Tone = 'error' | 'pending' | 'warning';

const TONE_ICON: Record<Tone, LucideIcon> = {
  error: CircleAlert,
  pending: CircleDashed,
  warning: TriangleAlert,
};
const TONE_COLOR: Record<Tone, string> = {
  error: 'text-red-soft',
  pending: 'text-tx-2',
  warning: 'text-yellow',
};

/**
 * How an issue is shown. Something not filled in yet is work to do; it turns
 * into an error once a save was held back because of it (`urgent`).
 */
function toneOf(issue: GraphIssue, urgent: boolean): Tone {
  if (issue.level === 'warning') return 'warning';
  return issue.pending && !urgent ? 'pending' : 'error';
}

/** Whether the flow can be saved as it stands, in a word beside its title. */
export function FlowStatus({ issues, urgent = false }: { issues: readonly GraphIssue[]; urgent?: boolean }) {
  const { t } = useTranslation('pipelines');
  const count = (tone: Tone) => issues.filter((issue) => toneOf(issue, urgent) === tone).length;
  const tone = (['error', 'pending', 'warning'] as const).find((item) => count(item) > 0);
  const Icon = tone ? TONE_ICON[tone] : CircleCheck;

  return (
    <span
      data-flow-status={tone ?? 'valid'}
      className={cn(
        'inline-flex items-center gap-1.5 font-sans text-xs font-strong',
        tone ? TONE_COLOR[tone] : 'text-green',
      )}
    >
      <Icon aria-hidden className="h-3.5 w-3.5" />
      {tone === 'error'
        ? t('graph.validation.summary', { count: count('error') })
        : tone === 'pending'
          ? t('graph.validation.pending', { count: count('pending') })
          : tone === 'warning'
            ? t('graph.validation.warnings', { count: count('warning') })
            : t('graph.validation.ok')}
    </span>
  );
}

/**
 * What is wrong with the flow, each line leading to the node it is about.
 * What is only left to fill in is not listed: the node says so itself, and
 * the status counts it. Once a save was held back (`urgent`) those are errors
 * like any other, and the list announces itself.
 */
export function FlowIssues({
  issues,
  urgent,
  onSelect,
}: {
  issues: readonly GraphIssue[];
  urgent: boolean;
  onSelect: (node: string) => void;
}) {
  const { t } = useTranslation('pipelines');
  const listed = issues.filter((issue) => toneOf(issue, urgent) !== 'pending');
  if (listed.length === 0) return null;

  return (
    <div
      data-flow-issues
      role={urgent ? 'alert' : undefined}
      className={cn(
        'mt-[12px] rounded-md px-[12px] py-[8px]',
        urgent ? 'bg-red-dim' : 'bg-[var(--control-surface)]',
      )}
    >
      {urgent && (
        <p className="m-0 mb-[4px] font-sans text-xs font-strong text-red-soft">
          {t('editor.fix_before_saving')}
        </p>
      )}
      <ul className="m-0 list-none space-y-[2px] p-0">
        {listed.map((issue, index) => {
          const tone = toneOf(issue, urgent);
          const Icon = TONE_ICON[tone];
          const line = (
            <>
              <Icon aria-hidden className={cn('mt-[2px] h-3.5 w-3.5 shrink-0', TONE_COLOR[tone])} />
              <span className={tone === 'error' ? 'text-tx-0' : 'text-tx-1'}>
                {t(`graph.validation.${issue.code}`, issue.params ?? {})}
              </span>
            </>
          );
          const shape = 'flex w-full items-start gap-[8px] rounded py-[3px] text-left font-sans text-xs leading-relaxed';

          return (
            <li key={`${issue.code}-${issue.node ?? ''}-${index}`} data-flow-issue={tone}>
              {issue.node ? (
                <button
                  type="button"
                  onClick={() => onSelect(issue.node ?? '')}
                  className={cn(shape, 'outline-none hover:underline focus-visible:underline')}
                >
                  {line}
                </button>
              ) : (
                <div className={shape}>{line}</div>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
