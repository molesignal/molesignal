import type { QueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as grantsApi from '@/api/groups';
import type * as teamsApi from '@/api/teams';
import type * as usersApi from '@/api/users';
import {
  permissionDefinition,
  type PermissionCatalog,
  type PermissionKey,
} from '@/product/permissions';
import { Pill } from '@/shell/chrome';

export const RESOURCE_TYPES = ['dashboard', 'stream'] as const;
export type ResourceType = (typeof RESOURCE_TYPES)[number];
export type PrincipalKind = 'user' | 'team';

export function PermissionPills({
  catalog,
  permissions,
}: {
  catalog: PermissionCatalog | undefined;
  permissions: readonly PermissionKey[];
}) {
  const { t } = useTranslation('iam');
  return (
    <div className="flex flex-wrap gap-1">
      {permissions.map((permission) => {
        const definition = permissionDefinition(catalog, permission);
        return (
          <Pill key={permission} tone="neutral">
            {definition ? t(definition.label_key) : permission}
          </Pill>
        );
      })}
    </div>
  );
}

export function principalLabel(
  binding: grantsApi.RoleBinding,
  users: Map<string, usersApi.UserView>,
  teams: Map<string, teamsApi.Team>,
  t: TFunction<'iam'>,
) {
  if (binding.principal_type === 'user') {
    const user = users.get(binding.principal_id);
    return user?.display_name || user?.email || t('groups.unknown_principal');
  }
  if (binding.principal_type === 'team') {
    return teams.get(binding.principal_id)?.name ?? t('groups.unknown_principal');
  }
  return t(`groups.principal.${binding.principal_type}`);
}

export function SectionTitle({
  title,
  description,
}: {
  title: React.ReactNode;
  description: React.ReactNode;
}) {
  return (
    <div className="mb-3">
      <h3 className="font-sans text-sm font-semibold text-tx-0">{title}</h3>
      <p className="mt-0.5 font-sans text-xs text-tx-3">{description}</p>
    </div>
  );
}

export async function invalidateIamAccess(queryClient: QueryClient) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ['iam', 'role-bindings'] }),
    queryClient.invalidateQueries({ queryKey: ['iam', 'cross-org-grants'] }),
    queryClient.invalidateQueries({ queryKey: ['iam', 'capabilities'] }),
  ]);
}
