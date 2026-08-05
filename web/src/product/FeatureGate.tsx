import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { type FeatureGateResult, type FeatureGateStatus, type FeatureKey, useFeatureGate } from './edition';
import { ProductState, type ProductStateVariant } from './states';
import { GatePage } from './templates';

export function FeatureGate({
  feature,
  children,
  fallback,
  compact = false,
}: {
  feature: FeatureKey;
  children: React.ReactNode;
  fallback?: React.ReactNode | undefined;
  compact?: boolean | undefined;
}) {
  const gate = useFeatureGate(feature);
  if (gate.status === 'allowed') return <>{children}</>;
  return fallback ?? <EditionGate gate={gate} compact={compact} />;
}

export function EditionGate({
  gate,
  compact = false,
  className,
}: {
  gate: FeatureGateResult;
  compact?: boolean | undefined;
  className?: string | undefined;
}) {
  const copy = useFeatureGateCopy(gate);
  return (
    <ProductState
      variant={stateVariantForGate(gate.status)}
      title={copy.title}
      description={copy.description}
      compact={compact}
      className={className}
    />
  );
}

export function EditionGatePage({
  gate,
  title,
  subtitle,
}: {
  gate: FeatureGateResult;
  title?: string | undefined;
  subtitle?: string | undefined;
}) {
  const copy = useFeatureGateCopy(gate);
  return (
    <GatePage
      title={title ?? copy.title}
      subtitle={subtitle}
      state={{
        variant: stateVariantForGate(gate.status),
        title: copy.title,
        description: copy.description,
      }}
    />
  );
}

export function FeatureBadge({
  feature,
  className,
}: {
  feature: FeatureKey;
  className?: string | undefined;
}) {
  const gate = useFeatureGate(feature);
  const { t } = useTranslation('edition');
  const label =
    gate.status === 'allowed'
      ? t('badges.available')
      : gate.status === 'loading'
        ? t('badges.checking')
        : t(`badges.${gate.status}`);

  return (
    <span
      className={cn(
        'inline-flex h-5 items-center rounded border px-1.5 font-sans text-xs font-semibold tracking-normal',
        gate.status === 'allowed' && 'border-green/30 bg-green-dim text-green-soft',
        gate.status === 'loading' && 'border-bd-1 bg-bg-2 text-tx-3',
        // Phase 4: gates that aren't allowed are a warning state (yellow),
        // not a brand surface (indigo) and not an error (red).
        gate.status !== 'allowed' && gate.status !== 'loading' && 'border-yellow/30 bg-yellow-dim text-yellow-soft',
        className,
      )}
    >
      {label}
    </span>
  );
}

export function useFeatureGateCopy(gate: FeatureGateResult): {
  title: string;
  description: string;
} {
  const { t } = useTranslation('edition');
  const feature = t(gate.feature.labelKey);
  return {
    title: t(`gates.${gate.status}.title`, { feature }),
    description: t(`gates.${gate.status}.description`, { feature }),
  };
}

function stateVariantForGate(status: FeatureGateStatus): ProductStateVariant {
  if (status === 'loading') return 'loading';
  if (status === 'backend-pending') return 'backend-pending';
  if (status === 'permission-denied') return 'permission-denied';
  if (status === 'saas-only') return 'saas-only';
  if (status === 'trial-available') return 'trial-available';
  return 'pro-required';
}
