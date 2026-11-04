import { useTranslation } from 'react-i18next';

import {
  type PermissionCatalog,
  type PermissionKey,
  type PermissionScope,
  permissionDefinition,
  permissionDefinitionsOf,
} from '@/product/permissions';
import { Pill } from '@/shell/chrome';

type SummaryScope = 'all' | PermissionScope;

const ALL_OF_SCOPE = {
  all: 'permission_summary.all',
  organization: 'permission_summary.all_organization',
  platform: 'permission_summary.all_platform',
} as const;

/**
 * What a role or a policy grants, in a list cell: the first three permissions
 * as pills and how many more, or, when it is every permission there is, that.
 */
export function PermissionSummary({
  catalog,
  permissions,
  scope = 'all',
}: {
  catalog: PermissionCatalog | undefined;
  permissions: readonly string[];
  /**
   * What every permission is: the whole catalog, or the permissions of one scope for a role or a
   * policy, which holds nothing else.
   */
  scope?: SummaryScope;
}) {
  const { t } = useTranslation('iam');

  if (permissions.length === 0) {
    return <span className="text-tx-3">-</span>;
  }

  // Holding everything is the one fact worth reading, rather than three names and a count.
  if (catalog && holdsEvery(catalog, permissions, scope)) {
    return (
      <span data-permission-summary="all">
        <Pill tone="indigo">
          {t(ALL_OF_SCOPE[scope], { count: permissions.length })}
        </Pill>
      </span>
    );
  }

  const visible = permissions.slice(0, 3);

  return (
    <div className="flex min-w-0 items-center gap-1 overflow-hidden" data-permission-summary="some">
      {visible.map((permission) => {
        const definition = permissionDefinition(catalog, permission as PermissionKey);

        return (
          <Pill key={permission} tone="neutral">
            {definition ? t(definition.labelKey) : permission}
          </Pill>
        );
      })}
      {permissions.length > visible.length && (
        <Pill tone="dim">+{permissions.length - visible.length}</Pill>
      )}
    </div>
  );
}

function holdsEvery(
  catalog: PermissionCatalog,
  permissions: readonly string[],
  scope: SummaryScope,
): boolean {
  const every = scope === 'all' ? catalog.permissions : permissionDefinitionsOf(catalog, scope);
  const held = new Set(permissions);

  return every.length > 0 && every.every((permission) => held.has(permission.key));
}
