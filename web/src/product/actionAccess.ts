import { useTranslation } from 'react-i18next';

import {
  hasPermission,
  type ProductAccess,
  useProductAccess,
} from '@/product/access';
import type { PermissionKey } from '@/product/permissions';

export interface ActionAccess {
  allowed: boolean;
  disabled: boolean;
  reason?: string | undefined;
}

export function restrictActionAccess(
  access: ActionAccess,
  enabled: boolean,
  disabledReason?: string,
): ActionAccess {
  if (access.disabled) return access;
  if (!enabled) {
    return {
      allowed: false,
      disabled: true,
      reason: disabledReason,
    };
  }
  return access;
}

interface ActionAccessOptions {
  permission?: PermissionKey;
  feature?: string;
  enabled?: boolean;
  disabledReason?: string | undefined;
}

interface ActionAccessCopy {
  loading: string;
  permissionRequired: (permission: PermissionKey) => string;
  featureRequired: (feature: string) => string;
}

export function resolveActionAccess(
  access: ProductAccess | null,
  options: ActionAccessOptions,
  copy: ActionAccessCopy,
): ActionAccess {
  if (!access || access.status === 'loading') {
    return { allowed: false, disabled: true, reason: copy.loading };
  }
  if (access.status !== 'ready') {
    return { allowed: false, disabled: true, reason: copy.loading };
  }
  if (options.permission && !hasPermission(options.permission, access)) {
    return {
      allowed: false,
      disabled: true,
      reason: copy.permissionRequired(options.permission),
    };
  }
  if (
    options.feature &&
    !access.features.has('*') &&
    !access.features.has(options.feature)
  ) {
    return {
      allowed: false,
      disabled: true,
      reason: copy.featureRequired(options.feature),
    };
  }
  if (options.enabled === false) {
    return {
      allowed: false,
      disabled: true,
      reason: options.disabledReason,
    };
  }
  return { allowed: true, disabled: false };
}

/**
 * Central action-level permission and feature decision used by toolbars,
 * empty states, row menus, row click handlers, and drawer submit controls.
 */
export function useActionAccess(
  options: ActionAccessOptions,
): ActionAccess {
  const access = useProductAccess();
  const { t } = useTranslation('common');
  return resolveActionAccess(access, options, {
    loading: t('access.loading'),
    permissionRequired: (permission) =>
      t('access.permission_required', { permission }),
    featureRequired: (feature) =>
      t('access.feature_required', { feature }),
  });
}
