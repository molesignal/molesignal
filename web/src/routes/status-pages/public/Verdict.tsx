import type { TFunction } from 'i18next';
import { CheckCircle2, TriangleAlert, Wrench } from 'lucide-react';
import type { ReactNode } from 'react';

import type { ComponentStatus, StatusPageLanguage } from '@/api/statusPages';
import { cn } from '@/shell/lib/cn';

import { formatMicros } from '../model';
import { microsDateTime } from './format';
import { STATUS_SURFACE } from './tone';

/**
 * The answer a status page exists to give: is it working. It is the first
 * thing on the page and the largest, and what is going on, if anything is,
 * follows inside the same surface: the verdict and its reasons are one
 * statement, not a banner above a list.
 */
export function PublicStatusVerdict({
  status,
  pageName,
  updatedAt,
  timezone,
  language,
  copy,
  children,
}: {
  status: ComponentStatus;
  pageName: string;
  updatedAt: number;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
  /** What is going on now: the incidents and the maintenance under way. */
  children?: ReactNode;
}) {
  const surface = STATUS_SURFACE[status];
  const Mark = status === 'operational' ? CheckCircle2 : status === 'maintenance' ? Wrench : TriangleAlert;

  return (
    <section
      aria-live="polite"
      data-status-verdict={status}
      className="overflow-hidden rounded-2xl border border-bd-0 bg-white"
    >
      <div
        className={cn(
          'flex flex-col gap-5 px-5 py-6 sm:px-7 sm:py-7 md:flex-row md:items-center md:justify-between md:gap-8',
          surface.wash,
        )}
      >
        <div className="flex min-w-0 items-start gap-4 sm:items-center">
          <span className={cn('grid h-12 w-12 shrink-0 place-items-center rounded-full sm:h-14 sm:w-14', surface.mark)}>
            <Mark aria-hidden className="h-6 w-6 sm:h-7 sm:w-7" strokeWidth={2.2} />
          </span>
          <div className="min-w-0">
            <h2 className="text-balance text-2xl font-display-strong tracking-[-0.02em] text-tx-0 sm:text-[28px] sm:leading-9">
              {copy(`public.overall.${status}`)}
            </h2>
            <p className="mt-1.5 max-w-2xl text-pretty text-sm leading-6 text-tx-2">
              {copy(`public.overall_description.${status}`, { name: pageName })}
            </p>
          </div>
        </div>
        <div className="shrink-0 md:text-right">
          <div className="text-xs font-strong text-tx-3">{copy('public.last_updated_label')}</div>
          <time
            dateTime={microsDateTime(updatedAt)}
            className="mt-1 block whitespace-nowrap text-sm font-strong tabular-nums text-tx-1"
          >
            {formatMicros(updatedAt, timezone, language)}
          </time>
        </div>
      </div>
      {children}
    </section>
  );
}
