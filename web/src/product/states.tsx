import { AlertTriangle, Ban, ServerCrash, type LucideIcon } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';
import { PulseLine } from '@/shell/PulseLine';

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

// Loading and empty are drawn with the product's own pulse (see PulseLine);
// the states that need attention keep a glyph, in a tile tinted with the tone.
const STATE_ICON = {
  error: AlertTriangle,
  'backend-pending': ServerCrash,
  'permission-denied': Ban,
} satisfies Partial<Record<ProductStateVariant, LucideIcon>>;

// State tones:
//   yellow = warning / pending (waiting on something external)
//   red    = error / denied
//   indigo = brand (reserved for primary surfaces, not state icons)
const STATE_TILE = {
  error: 'bg-red-dim text-red-soft',
  'backend-pending': 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]',
  'permission-denied': 'bg-red-dim text-red-soft',
} satisfies Partial<Record<ProductStateVariant, string>>;

function StateMark({ variant, compact }: { variant: ProductStateVariant; compact: boolean }) {
  if (variant === 'loading' || variant === 'empty') {
    return (
      <PulseLine
        variant={variant === 'loading' ? 'travel' : 'empty'}
        className={cn('text-tx-3', compact ? 'h-6 w-20' : 'h-10 w-32')}
      />
    );
  }
  const Icon = STATE_ICON[variant];
  return (
    <div
      className={cn(
        'grid place-items-center rounded-md',
        compact ? 'h-10 w-10' : 'h-12 w-12',
        STATE_TILE[variant],
      )}
    >
      <Icon className={compact ? 'h-5 w-5' : 'h-6 w-6'} />
    </div>
  );
}

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
        compact ? 'min-h-40 gap-3 px-5 py-7' : 'min-h-64 gap-5 px-8 py-14',
        className,
      )}
    >
      <StateMark variant={variant} compact={compact} />
      <div className="max-w-lg">
        <div className="type-section-title text-balance font-sans font-semibold text-tx-0">{stateTitle}</div>
        {stateDescription && (
          <div className="mt-1.5 text-pretty font-sans text-sm leading-relaxed text-tx-2">
            {stateDescription}
          </div>
        )}
      </div>
      {action && <div className="mt-1 flex flex-wrap items-center justify-center gap-2">{action}</div>}
    </section>
  );
}
