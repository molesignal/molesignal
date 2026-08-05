import { AlertTriangle, Ban, Database, Loader2, LockKeyhole, ServerCrash, type LucideIcon } from 'lucide-react';
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
  | 'license-gated'
  | 'pro-required'
  | 'saas-only'
  | 'trial-available';

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
  'license-gated': LockKeyhole,
  'pro-required': LockKeyhole,
  'saas-only': LockKeyhole,
  'trial-available': LockKeyhole,
} satisfies Record<ProductStateVariant, LucideIcon>;

// Phase 4 status color logic:
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
  'license-gated': 'text-yellow-soft',
  'pro-required': 'text-yellow-soft',
  'saas-only': 'text-yellow-soft',
  'trial-available': 'text-blue',
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
      role={variant === 'error' ? 'alert' : 'status'}
      aria-live={variant === 'loading' ? 'polite' : undefined}
      className={cn(
        'flex flex-col items-center justify-center rounded-lg border border-dashed border-bd-1 bg-bg-1 text-center',
        compact ? 'min-h-40 gap-3 px-5 py-7' : 'min-h-60 gap-4 px-8 py-12',
        className,
      )}
    >
      <div className={cn('grid place-items-center rounded-lg border border-bd-0 bg-bg-2', compact ? 'h-10 w-10' : 'h-12 w-12')}>
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

/**
 * 后端对未授权的 license 特性返回 402/403（如 `forbidden: <feature> feature not
 * licensed`）。本 hook 把这类错误转成本地化的 pro-required gate，避免把后端英文原文
 * 透传给用户。返回 `null` 表示该错误不是 license 拒绝，调用方应按普通 error 处理。
 *
 * 用法：
 * ```tsx
 * const licenseGate = useLicenseErrorGate();
 * const pageState = q.isError
 *   ? (licenseGate(q.error, 'features.intelligence') ?? { variant: 'error', error: q.error })
 *   : productStateFor(state, { ... });
 * ```
 */
export function useLicenseErrorGate(): (
  error: unknown,
  featureLabelKey: string,
) => ProductStateProps | null {
  const { t } = useTranslation('edition');
  return React.useCallback(
    (error, featureLabelKey) => {
      const status = toApiError(error).status;
      if (status !== 402 && status !== 403) return null;
      const feature = t(featureLabelKey);
      return {
        variant: 'pro-required',
        title: t('gates.pro-required.title', { feature }),
        description: t('gates.pro-required.description', { feature }),
      };
    },
    [t],
  );
}
