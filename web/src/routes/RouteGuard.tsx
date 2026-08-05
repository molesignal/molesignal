import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Navigate, useLocation } from 'react-router-dom';

import {
  canAccessProductPath,
  deniedProductRouteFallback,
  isKnownProductPath,
  useProductAccess,
} from '@/product/access';

/**
 * Applies the central route-access policy to every authenticated shell route.
 * It reacts to organization switches synchronously, before a denied leaf
 * component can mount or issue its data request.
 */
export function ProductRouteAccessGuard({
  children,
}: {
  children: React.ReactNode;
}) {
  const { t } = useTranslation('iam');
  const location = useLocation();
  const access = useProductAccess();
  if (!access || access.status === 'loading') {
    return (
      <div
        role="status"
        aria-label={t('capabilities.loading')}
        className="flex min-h-[180px] items-center justify-center text-xs text-tx-3"
      >
        {t('capabilities.loading')}
      </div>
    );
  }
  if (access.status === 'error') {
    return (
      <div
        role="alert"
        className="flex min-h-[180px] items-center justify-center text-xs text-red-soft"
      >
        {t('capabilities.load_error')}
      </div>
    );
  }
  if (
    isKnownProductPath(location.pathname, access) &&
    !canAccessProductPath(location.pathname, access)
  ) {
    return (
      <Navigate
        to={deniedProductRouteFallback(location.pathname, access)}
        replace
      />
    );
  }
  return <>{children}</>;
}
