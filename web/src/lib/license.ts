import { useQuery } from '@tanstack/react-query';

import * as licenseApi from '@/api/license';
import type { LicenseSnapshot } from '@/api/license';
import { hasPermission, useProductAccess } from '@/product/access';

/**
 * License-feature gating hook. Fetches `/system/license` once per minute
 * (cached) and reports whether `feature` is in the snapshot's `features` list. Used by
 * pages whose backend handler 403s with `<feature> feature not licensed` to
 * pre-empt the failed request and render a friendly license-gated empty
 * state instead of surfacing the raw error string.
 *
 * Usage:
 * ```tsx
 * const { licensed, isLoading } = useLicenseFeature('federated_search');
 * ```
 *
 * `licensed === false` while loading is treated as "not yet known"; the page
 * should render a loader rather than the gated empty state. The hook returns
 * `loaded === true` when the snapshot has resolved at least once.
 */
export function useLicenseFeature(feature: string): {
  licensed: boolean;
  loaded: boolean;
  isLoading: boolean;
  snapshot: LicenseSnapshot | undefined;
} {
  const access = useProductAccess();
  const canReadLicense = hasPermission('sys.licenses.read', access);
  const q = useQuery({
    queryKey: ['license-snapshot'],
    queryFn: () => licenseApi.get(),
    enabled: canReadLicense,
    staleTime: 60_000,
  });
  const snapshot = canReadLicense ? q.data : undefined;
  const licensed = snapshot?.features.includes(feature) ?? false;
  return {
    licensed,
    loaded: !canReadLicense || snapshot !== undefined,
    isLoading: canReadLicense && q.isLoading,
    snapshot,
  };
}
