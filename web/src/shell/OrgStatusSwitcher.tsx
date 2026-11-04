import { ChevronsUpDown } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import type { Org } from '@/api/orgs';
import { canAccessProductPath, useProductAccess } from '@/product/access';
import { useRoleName } from '@/product/roleName';
import { cn } from '@/shell/lib/cn';
import { SystemStatusDot, useSystemStatus } from '@/shell/SystemStatusIndicator';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

/** Where organizations are created, entered and administered: from the platform organization, for those allowed. */
const MANAGE_ORGANIZATIONS_PATH = '/iam/organizations';

/**
 * The organization the reader works in, said together with the health of the core API: the
 * light in front of the name is the system status light, and the status in words is the button's
 * name. Opened, it lists the organizations to switch to; with only one it shows that one, and no
 * arrows. Whoever may administer organizations finds the way there at the foot of the list.
 */
export function OrgStatusSwitcher({
  orgName,
  role,
  currentOrgId,
  orgOptions,
  onSwitch,
  className,
}: {
  orgName: string;
  /** The reader's role in the current organization, when the list does not carry it. */
  role: string | null;
  currentOrgId: string | null;
  orgOptions: Org[];
  onSwitch: (orgId: string) => void;
  className?: string;
}) {
  const roleName = useRoleName();
  const { t } = useTranslation(['shell', 'common']);
  const navigate = useNavigate();
  const access = useProductAccess();
  const canManage = canAccessProductPath(MANAGE_ORGANIZATIONS_PATH, access);
  const status = useSystemStatus();
  const statusLabel = t(`shell:system_status.${status}`);
  const canSwitch = orgOptions.length > 1;
  // The name on the button is the one the list marks, so the two never disagree.
  const currentName = orgOptions.find((org) => org.id === currentOrgId)?.name ?? orgName;

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            data-testid="org-status-trigger"
            aria-label={t('shell:topbar.organization_status', { name: currentName, status: statusLabel })}
            className={cn(
              'ms-press flex h-8 min-w-8 max-w-[220px] items-center justify-center gap-2 rounded-md px-3 text-tx-0 hover:bg-bg-3 focus-visible:bg-indigo-dim focus-visible:outline-none data-[state=open]:bg-bg-3 md:justify-start md:pl-[10px] md:pr-2',
              className,
            )}
          >
            <SystemStatusDot status={status} />
            <span className="hidden truncate font-sans text-sm font-strong md:inline">{currentName}</span>
            {canSwitch && (
              <ChevronsUpDown aria-hidden className="hidden h-[14px] w-[14px] shrink-0 text-tx-3 md:block" />
            )}
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" data-ui="org-status-menu" className="w-64">
          <DropdownMenuLabel className="type-micro px-[10px] pb-1 pt-2 font-sans font-medium tracking-normal text-tx-3">
            {canSwitch ? t('shell:topbar.switch_organization') : t('shell:topbar.current_organization')}
          </DropdownMenuLabel>
          <DropdownMenuRadioGroup value={currentOrgId ?? ''} onValueChange={onSwitch}>
            {orgOptions.map((org) => {
              const isCurrent = org.id === currentOrgId;
              const orgRole = org.displayRole ?? (isCurrent ? role : null);
              return (
                <DropdownMenuRadioItem
                  key={org.id}
                  value={org.id}
                  data-ui="workspace-option"
                  className="min-h-9 rounded-md"
                  disabled={org.disabled}
                >
                  <span className="min-w-0 flex-1 truncate font-sans text-sm font-strong">{org.name}</span>
                  {orgRole && (
                    <span
                      data-testid={isCurrent ? 'current-org-role' : undefined}
                      className="type-micro shrink-0 rounded-full bg-bg-4 px-1.5 py-0.5 font-sans text-tx-1"
                    >
                      {roleName(orgRole)}
                    </span>
                  )}
                  {org.disabled && <span className="type-micro text-tx-3">{t('common:status.disabled')}</span>}
                </DropdownMenuRadioItem>
              );
            })}
          </DropdownMenuRadioGroup>
          {canManage && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem className="min-h-9 rounded-md" onSelect={() => navigate(MANAGE_ORGANIZATIONS_PATH)}>
                {t('shell:topbar.manage_organizations')}
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
      {/* The light changes on its own; a screen reader hears it say so. */}
      <span role="status" className="sr-only">
        {statusLabel}
      </span>
    </>
  );
}
