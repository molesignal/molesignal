import { ExternalLink, GitPullRequest, Workflow } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ChangeEntry } from '@/api/changes';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/shell/ui/hover-card';

import { CHANGE_ICON, changeHref, changeSummary, changeTone, shareOf } from './model';

/** The most changes a rail marks; past it the latest are marked. */
const MOST_MARKS = 100;

/**
 * A dashed line at each change across a plot, laid over it and taking no
 * pointer: where the time axis says a change happened. `start` and `end` are
 * the plot's frame in microseconds.
 */
export function ChangeLines({ changes, start, end }: { changes: readonly ChangeEntry[]; start: number; end: number }) {
  return (
    <>
      {changes.slice(0, MOST_MARKS).map((change) => (
        <span
          key={change.id}
          aria-hidden="true"
          data-change-line={change.kind}
          className={cn('pointer-events-none absolute inset-y-0 w-0 border-l border-dashed border-current opacity-50', changeTone(change))}
          style={{ left: `${shareOf(change.at, start, end)}%` }}
        />
      ))}
    </>
  );
}

/**
 * A row of the changes of a frame, each an icon of its kind where it
 * happened; hovering or focusing one tells what changed, who changed it and
 * where to read more.
 */
export function ChangeRail({
  changes,
  start,
  end,
  className,
}: {
  changes: readonly ChangeEntry[];
  start: number;
  end: number;
  className?: string;
}) {
  const { t } = useTranslation('common');

  if (changes.length === 0) return null;

  return (
    <div data-change-rail="" role="list" aria-label={t('changes.rail', { count: changes.length })} className={cn('relative h-5', className)}>
      {changes.slice(0, MOST_MARKS).map((change) => (
        <ChangeMark key={change.id} change={change} left={shareOf(change.at, start, end)} />
      ))}
    </div>
  );
}

function ChangeMark({ change, left }: { change: ChangeEntry; left: number }) {
  const { t } = useTranslation('common');
  const time = useTimeFormatter();
  const Icon = CHANGE_ICON[change.kind];
  const summary = changeSummary(change, t);
  const href = changeHref(change);

  return (
    <span role="listitem" className="absolute top-0 -translate-x-1/2" style={{ left: `${left}%` }}>
      <HoverCard openDelay={150} closeDelay={100}>
        <HoverCardTrigger asChild>
          <button
            type="button"
            data-change-mark={change.kind}
            aria-label={summary}
            className={cn('grid h-5 w-5 place-items-center rounded outline-none hover:bg-bg-3 focus-visible:bg-bg-3', changeTone(change))}
          >
            <Icon aria-hidden className="h-3 w-3" />
          </button>
        </HoverCardTrigger>
        <HoverCardContent side="top" className="w-72 p-3 font-sans text-xs">
          <div className="flex flex-col gap-1.5">
            <span className="font-strong text-tx-0">{summary}</span>
            <span className="tabular-nums text-tx-3">
              {time.micros(change.at, false)}
              {change.end !== undefined && ` – ${time.micros(change.end, false)}`}
            </span>
            {change.description && change.kind !== 'deployment' && <span className="text-tx-1">{change.description}</span>}
            {(change.author || change.commit) && (
              <span className="text-tx-2">
                {[change.author, change.commit?.slice(0, 7)].filter(Boolean).join(' · ')}
              </span>
            )}
            {(href || change.pullRequestUrl || change.ciUrl) && (
              <span className="flex flex-wrap items-center gap-3 pt-0.5">
                {href && (
                  <Link to={href} className="inline-flex items-center gap-1 font-strong text-indigo-soft hover:underline focus-visible:underline focus-visible:outline-none">
                    <ExternalLink aria-hidden className="h-3 w-3" />
                    {t('changes.open')}
                  </Link>
                )}
                {change.pullRequestUrl && (
                  <a href={change.pullRequestUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-tx-1 hover:underline focus-visible:underline focus-visible:outline-none">
                    <GitPullRequest aria-hidden className="h-3 w-3" />
                    {t('changes.pull_request')}
                  </a>
                )}
                {change.ciUrl && (
                  <a href={change.ciUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-tx-1 hover:underline focus-visible:underline focus-visible:outline-none">
                    <Workflow aria-hidden className="h-3 w-3" />
                    {t('changes.ci_run')}
                  </a>
                )}
              </span>
            )}
          </div>
        </HoverCardContent>
      </HoverCard>
    </span>
  );
}
