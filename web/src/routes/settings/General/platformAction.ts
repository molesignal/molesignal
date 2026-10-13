import { useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import type { PermissionKey } from '@/product/permissions';
import { toast } from '@/shell/ui/sonner';
import { useOrgStore } from '@/stores/useOrgStore';

/**
 * Where something that is done to the platform is within reach:
 *  - `here`: this session is in the platform organization and holds the permission;
 *  - `platform`: this session is in an organization, and the person can enter the
 *    platform organization: they are taken there to do it;
 *  - `none`: the person cannot enter the platform organization.
 */
export type PlatformReach = 'here' | 'platform' | 'none' | 'loading';

/**
 * Access to an action behind a platform (`sys.*`) permission.
 *
 * The platform's administrator and the owner of an organization both hold
 * the platform permissions. What they administer with them, the organizations
 * and the platform's own telemetry, lives in the platform organization and is
 * only served there, so holding the permission in another organization is not
 * yet being where the action is. Whoever can enter the platform organization
 * is offered the action, and running it enters that organization first.
 */
export function usePlatformAction(permission: PermissionKey): {
  reach: PlatformReach;
  /** Entering the platform organization is under way. */
  entering: boolean;
  /** Runs `action` where the permission is in force, entering the platform organization first if need be. */
  run: (action: () => void) => Promise<void>;
} {
  const { t } = useTranslation('errors');
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const orgs = useOrgStore((state) => state.orgs);
  const loaded = useOrgStore((state) => state.loaded);
  const loadOrgs = useOrgStore((state) => state.loadOrgs);
  const switchOrg = useOrgStore((state) => state.switchOrg);
  const [entering, setEntering] = React.useState(false);
  const here = access?.scope === 'system' && hasPermission(permission, access);
  const platform = orgs.find((org) => org.system === true && !org.disabled);

  React.useEffect(() => {
    if (loaded) return;

    // Without the list nobody is offered the way in, which is the safe side to be wrong on.
    void loadOrgs().catch(() => undefined);
  }, [loadOrgs, loaded]);

  const reach: PlatformReach = here
    ? 'here'
    : access?.status !== 'ready' || !loaded
      ? 'loading'
      : platform
        ? 'platform'
        : 'none';

  const run = async (action: () => void) => {
    if (here) {
      action();
      return;
    }
    if (!platform || entering) return;

    setEntering(true);
    try {
      const entered = await switchOrg(platform.id, { queryClient });
      toast.success(t('switch_org_success', { name: entered.name }));
      action();
    } catch (error) {
      toast.error(t('switch_org_failure', { message: toApiError(error).message }));
    } finally {
      setEntering(false);
    }
  };

  return { reach, entering, run };
}
