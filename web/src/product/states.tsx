import { AlertTriangle, Ban, Database, Loader2, ServerCrash, type LucideIcon } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';

export type ProductStateVariant =
  | 'loading'
  | 'empty'
  | 'error'
  | 'backend-pending'
  | 'permission-denied'

export interface ProductStateProps {
  variant: ProductStateVariant;
  title?: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  error?: unknown;
  compact?: boolean | undefined;
  className?: string | undefined;
}

export type QueryProductState = 'loading' | 'empty' | 'error' | null;

export function productStateFor(
  state: QueryProductState,
  options: {
    error?: unknown;
    emptyTitle?: React.ReactNode;
    emptyDescription?: React.ReactNode;
    emptyAction?: React.ReactNode;
    emptyVariant?: Extract<ProductStateVariant, 'empty' | 'backend-pending'>;
  } = {},
): ProductStateProps | null {
  if (state === 'loading') return { variant: 'loading' };
  if (state === 'error') return { variant: 'error', error: options.error };
  if (state === 'empty') {
    return {
      variant: options.emptyVariant ?? 'empty',
      title: options.emptyTitle,
      description: options.emptyDescription,
      action: options.emptyAction,
    };
  }
  return null;
}

const STATE_ICON = {
  loading: Loader2,
  empty: Database,
  error: AlertTriangle,
  'backend-pending': ServerCrash,
  'permission-denied': Ban,
} satisfies Record<ProductStateVariant, LucideIcon>;

// State tones:
//   yellow = warning / pending (waiting on something external)
//   red    = error / denied
//   blue   = info / link
//   indigo = brand (reserved for primary surfaces, not state icons)
const STATE_TONE = {
  loading: 'text-blue',
  empty: 'text-tx-3',
  error: 'text-red-soft',
  'backend-pending': 'text-yellow-soft',
  'permission-denied': 'text-red-soft',
} satisfies Record<ProductStateVariant, string>;

export function ProductState({
  variant,
  title,
  description,
  action,
  error,
  compact = false,
  className,
}: ProductStateProps) {
  const { t } = useTranslation('design-system');
  const Icon = STATE_ICON[variant];
  const stateTitle = title ?? t(`states.${variant}.title`);
  const stateDescription =
    description ??
    (variant === 'error' && error
      ? toApiError(error).message
      : t(`states.${variant}.description`));

  return (
    <section
      data-product-state
      role={variant === 'error' ? 'alert' : 'status'}
      aria-live={variant === 'loading' ? 'polite' : undefined}
      className={cn(
        'flex flex-col items-center justify-center rounded-md border-0 bg-[var(--functional-surface)] text-center [box-shadow:var(--shadow-functional-surface)]',
        compact ? 'min-h-40 gap-3 px-5 py-7' : 'min-h-60 gap-4 px-8 py-12',
        className,
      )}
    >
      <div className={cn('grid place-items-center rounded-md bg-[var(--control-surface)]', compact ? 'h-10 w-10' : 'h-12 w-12')}>
        <Icon className={cn(compact ? 'h-5 w-5' : 'h-6 w-6', STATE_TONE[variant], variant === 'loading' && 'animate-spin')} />
      </div>
      <div className="max-w-lg">
        <div className="type-section-title font-sans font-semibold text-tx-0">{stateTitle}</div>
        {stateDescription && (
          <div className="mt-1.5 font-sans text-sm leading-relaxed text-tx-2">
            {stateDescription}
          </div>
        )}
      </div>
      {action && <div className="mt-1 flex flex-wrap items-center justify-center gap-2">{action}</div>}
    </section>
  );
}
