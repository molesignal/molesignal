import {
  Construction,
  Inbox,
  type LucideIcon,
  PlusCircle,
  Search,
  ShieldOff,
  Sparkles,
} from 'lucide-react';
import type * as React from 'react';
import { Link } from 'react-router-dom';

import type { ProductEmptyStateStrategy } from '@/product/ia';
import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';

/**
 * EmptyState renders the shared layout for product surfaces with no data: a
 * single 48px stroke icon, one short title, an optional one-sentence
 * description, an optional primary CTA, and an optional secondary link.
 * Content remains centered in the available space.
 *
 * Strategy is sourced from `ia.ts.emptyStateStrategy` — passing it gives
 * a sensible default icon when the caller doesn't supply one. The 7
 * strategies are listed below; the icon mapping is intentional, not
 * decorative.
 */

interface EmptyAction {
  label: string;
  onClick?: () => void;
  to?: string;
  disabled?: boolean;
  disabledReason?: string | undefined;
}

interface EmptyStateProps {
  /** What kind of empty is this? Drives the default icon. */
  strategy?: ProductEmptyStateStrategy | undefined;
  title: string;
  description?: string | undefined;
  /** Override the strategy-derived icon. */
  icon?: LucideIcon | undefined;
  /** Replaces the icon with a custom illustration (decorative; hidden from AT). */
  illustration?: React.ReactNode;
  /**
   * `compact` sits inside a card body: a smaller mark and heading, no forced
   * min-height. `default` owns the whole region it is placed in.
   */
  size?: 'default' | 'compact' | undefined;
  primaryAction?: EmptyAction | undefined;
  secondaryAction?: EmptyAction | undefined;
  className?: string | undefined;
  /** Test hook. */
  'data-testid'?: string | undefined;
}

const STRATEGY_ICON: Record<ProductEmptyStateStrategy, LucideIcon> = {
  activation: Sparkles,
  'query-first': Search,
  'create-first': PlusCircle,
  'backend-pending': Construction,
  'permission-denied': ShieldOff,
  none: Inbox,
};

export function EmptyState({
  strategy = 'none',
  title,
  description,
  icon,
  illustration,
  size = 'default',
  primaryAction,
  secondaryAction,
  className,
  'data-testid': testId,
}: EmptyStateProps) {
  const Icon = icon ?? STRATEGY_ICON[strategy];
  const compact = size === 'compact';
  const Heading = compact ? 'h3' : 'h2';
  return (
    <div
      role="status"
      data-testid={testId}
      data-strategy={strategy}
      data-size={size}
      className={cn(
        'flex h-full w-full flex-col items-center justify-center text-center',
        compact ? 'min-h-0 gap-2 px-5 py-6' : 'min-h-[280px] gap-3 px-6 py-8',
        className,
      )}
    >
      {illustration ?? (
        <Icon
          aria-hidden
          className={cn(
            'shrink-0 stroke-[1.5] text-tx-3',
            compact ? 'h-8 w-8' : 'h-12 w-12',
          )}
        />
      )}
      <Heading
        className={cn(
          'max-w-md text-balance font-sans text-tx-0',
          compact ? 'text-sm font-strong' : 'text-base font-display-strong',
        )}
      >
        {title}
      </Heading>
      {description && (
        <p className="max-w-md text-pretty font-sans text-xs leading-relaxed text-tx-2">
          {description}
        </p>
      )}
      {(primaryAction || secondaryAction) && (
        <div className={cn('flex items-center gap-3', compact ? 'mt-1' : 'mt-2')}>
          {/* Inside a card the CTA stays neutral, so a page with several empty
              cards keeps a single primary action. */}
          {primaryAction && (
            <ActionButton action={primaryAction} variant={compact ? 'secondary' : 'primary'} />
          )}
          {secondaryAction && <ActionButton action={secondaryAction} variant="secondary" />}
        </div>
      )}
    </div>
  );
}

function ActionButton({
  action,
  variant,
}: {
  action: EmptyAction;
  variant: 'primary' | 'secondary';
}) {
  const cls = cn(
    'inline-flex h-8 items-center justify-center rounded-md px-3 font-sans text-xs font-strong',
    'transition-colors duration-fast ease-default focus-visible:outline-none',
    variant === 'primary'
      ? 'bg-indigo text-white hover:brightness-90 focus-visible:brightness-90 focus-visible:text-white'
      : 'bg-bg-2 text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-indigo-dim focus-visible:text-indigo',
    action.disabled &&
      'pointer-events-none cursor-not-allowed bg-bg-2 text-tx-3 hover:bg-bg-2 hover:text-tx-3',
  );
  if (action.disabled) {
    return (
      <DisabledControl disabled reason={action.disabledReason}>
        <button
          type="button"
          disabled
          aria-disabled="true"
          className={cls}
        >
          {action.label}
        </button>
      </DisabledControl>
    );
  }
  if (action.to) {
    return (
      <Link to={action.to} className={cls} onClick={action.onClick}>
        {action.label}
      </Link>
    );
  }
  return (
    <button type="button" onClick={action.onClick} className={cls}>
      {action.label}
    </button>
  );
}
