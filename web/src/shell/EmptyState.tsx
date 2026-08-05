import {
  Construction,
  Inbox,
  Lock,
  type LucideIcon,
  PlusCircle,
  Search,
  ShieldOff,
  Sparkles,
} from 'lucide-react';
import { Link } from 'react-router-dom';

import type { ProductEmptyStateStrategy } from '@/product/ia';
import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';

/**
 * EmptyState — brief Principle #2 head-of-line citizen. Renders the
 * standard layout that every "no data yet" surface in the product agrees
 * on: a single 48px stroke icon, one short title, optional one-sentence
 * description, optional primary CTA, optional secondary link. Always
 * centered in the available space.
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
  'license-gated': Lock,
  'permission-denied': ShieldOff,
  none: Inbox,
};

export function EmptyState({
  strategy = 'none',
  title,
  description,
  icon,
  primaryAction,
  secondaryAction,
  className,
  'data-testid': testId,
}: EmptyStateProps) {
  const Icon = icon ?? STRATEGY_ICON[strategy];
  return (
    <div
      role="status"
      data-testid={testId}
      data-strategy={strategy}
      className={cn(
        'flex h-full min-h-[280px] w-full flex-col items-center justify-center gap-3 px-6 py-8 text-center',
        className,
      )}
    >
      <Icon
        aria-hidden
        className="h-12 w-12 shrink-0 stroke-[1.5] text-tx-3"
      />
      <h2 className="max-w-md font-sans text-base font-display-strong text-tx-0">{title}</h2>
      {description && (
        <p className="max-w-md font-sans text-xs leading-relaxed text-tx-2">{description}</p>
      )}
      {(primaryAction || secondaryAction) && (
        <div className="mt-2 flex items-center gap-3">
          {primaryAction && <ActionButton action={primaryAction} variant="primary" />}
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
    'transition-colors duration-fast ease-default',
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo focus-visible:ring-offset-2 focus-visible:ring-offset-bg-0',
    variant === 'primary'
      ? 'bg-indigo text-white hover:bg-indigo-soft'
      : 'border border-bd-1 bg-bg-2 text-tx-1 hover:bg-bg-3 hover:text-tx-0',
    action.disabled &&
      'pointer-events-none cursor-not-allowed border-bd-0 bg-bg-2 text-tx-3 hover:bg-bg-2 hover:text-tx-3',
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
