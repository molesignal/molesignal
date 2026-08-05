import { useQuery } from '@tanstack/react-query';

import * as iamApi from '@/api/iam';

export const iamPermissionCatalogQueryKey = [
  'iam',
  'permission-catalog',
] as const;

export function useIamPermissionCatalog() {
  return useQuery({
    queryKey: iamPermissionCatalogQueryKey,
    queryFn: iamApi.permissionCatalog,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}
