import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

function textKey(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replaceAll('&', ' and ')
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '');
}

export function translateDashboardText(
  t: TFunction<'dashboards'>,
  value: string,
): string {
  return t(`engine.text.${textKey(value)}`, { defaultValue: value });
}

export function useDashboardText(): (value: string) => string {
  const { t } = useTranslation('dashboards');
  return React.useCallback(
    (value: string) => translateDashboardText(t, value),
    [t],
  );
}
