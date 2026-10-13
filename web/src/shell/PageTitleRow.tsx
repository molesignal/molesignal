import * as React from 'react';

import { cn } from '@/shell/lib/cn';

interface PageTitleRowProps {
  title: React.ReactNode;
  description?: React.ReactNode;
  leading?: React.ReactNode;
  actions?: React.ReactNode;
  level?: 1 | 2;
  size?: 'page' | 'section';
  className?: string;
  titleClassName?: string;
  descriptionClassName?: string;
  actionsClassName?: string;
}

/**
 * Shared title row for product pages and their routed sub-pages.
 *
 * Two sizes, two shapes:
 * - `page` is the module masthead. The title and its supporting copy share one
 *   baseline and the copy yields first when the row runs out of room.
 * - `section` is the title of a sub-page inside a card (Settings, Access,
 *   Account). The copy sits under the title and wraps, because a sentence
 *   that explains a page is worth more than a one-line fragment of it.
 *
 * Actions wrap onto their own row on narrow screens. Breadcrumbs and other
 * resource navigation live outside this primitive, so they can add context
 * without changing the title rhythm itself.
 */
export function PageTitleRow({
  title,
  description,
  leading,
  actions,
  level = 1,
  size = 'page',
  className,
  titleClassName,
  descriptionClassName,
  actionsClassName,
}: PageTitleRowProps) {
  const Heading = level === 1 ? 'h1' : 'h2';
  const hasDescription =
    description !== undefined && description !== null && description !== '';
  const stacked = size === 'section';

  const actionsBlock = actions && (
    <div
      className={cn(
        'ml-auto flex w-full min-w-0 flex-wrap items-center justify-start gap-2 sm:w-auto sm:max-w-full sm:justify-end',
        actionsClassName,
      )}
    >
      {actions}
    </div>
  );

  if (stacked) {
    return (
      <div
        data-page-title-row
        data-page-title-size={size}
        className={cn(
          'flex min-h-[var(--page-title-row-h)] min-w-0 flex-wrap items-center gap-x-4 gap-y-2',
          className,
        )}
      >
        {leading}
        <div
          data-page-title-block
          className="min-w-[min(100%,18rem)] flex-1"
        >
          <Heading
            data-page-title-heading
            // Negative tracking squeezes CJK glyphs (they are drawn to fill their em box).
            className={cn(
              'type-heading text-balance font-sans font-display-strong tracking-normal text-tx-0',
              titleClassName,
            )}
          >
            {title}
          </Heading>
          {hasDescription && (
            <p
              data-page-title-meta
              className={cn(
                'type-label mt-1 max-w-3xl text-pretty text-tx-2',
                descriptionClassName,
              )}
            >
              {description}
            </p>
          )}
        </div>
        {actionsBlock}
      </div>
    );
  }

  return (
    <div
      data-page-title-row
      data-page-title-size={size}
      className={cn(
        'flex min-h-[var(--page-title-row-h)] min-w-0 flex-wrap items-center gap-2',
        className,
      )}
    >
      {leading}
      {/* The title block reserves room for a readable title; a toolbar that
          cannot fit beside it drops to its own row instead of squeezing it. */}
      <div
        data-page-title-block
        className="flex min-w-[min(100%,22rem)] flex-1 items-baseline gap-2 overflow-hidden whitespace-nowrap"
      >
        <Heading
          data-page-title-heading
          className={cn(
            'max-w-full shrink-0 truncate font-sans font-display-strong text-tx-0',
            // Negative tracking squeezes CJK glyphs (they are drawn to fill their em box).
            'type-page-title tracking-normal',
            !hasDescription && !actions && 'max-w-full',
            titleClassName,
          )}
        >
          {title}
        </Heading>
        {hasDescription && (
          <>
            <span aria-hidden data-page-title-meta className="shrink-0 text-tx-3">
              ·
            </span>
            <p
              data-page-title-meta
              className={cn('type-caption min-w-0 truncate text-tx-2', descriptionClassName)}
            >
              {description}
            </p>
          </>
        )}
      </div>
      {actionsBlock}
    </div>
  );
}
