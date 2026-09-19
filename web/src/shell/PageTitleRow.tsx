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
 * The title and supporting copy always share one baseline and one fixed-height
 * row. Breadcrumbs and other resource navigation live outside this primitive,
 * so they can add context without changing the title rhythm itself.
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

  return (
    <div
      data-page-title-row
      data-page-title-size={size}
      className={cn(
        'flex h-[var(--page-title-row-h)] min-w-0 flex-nowrap items-center gap-2',
        className,
      )}
    >
      {leading}
      <div className="flex min-w-0 flex-1 items-baseline gap-2 overflow-hidden whitespace-nowrap">
        <Heading
          className={cn(
            'shrink-0 truncate font-sans font-display-strong text-tx-0',
            size === 'page'
              ? 'type-page-title max-w-[48%] tracking-[-0.025em]'
              : 'type-section-title max-w-[52%]',
            !hasDescription && !actions && 'max-w-full',
            titleClassName,
          )}
        >
          {title}
        </Heading>
        {hasDescription && (
          <>
            <span aria-hidden className="shrink-0 text-tx-3">
              ·
            </span>
            <p
              className={cn(
                'min-w-0 truncate text-tx-2',
                size === 'page' ? 'type-caption' : 'type-label',
                descriptionClassName,
              )}
            >
              {description}
            </p>
          </>
        )}
      </div>
      {actions && (
        <div
          className={cn(
            'ml-auto flex max-w-[60%] shrink-0 items-center justify-end gap-2 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden',
            actionsClassName,
          )}
        >
          {actions}
        </div>
      )}
    </div>
  );
}
