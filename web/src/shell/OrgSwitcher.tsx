import { Building2, ChevronDown, Settings, Users } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import type { Org } from '@/api/orgs';
import { canAccessProductPath, useProductAccess } from '@/product/access';
import { useRoleName } from '@/product/roleName';
import { Pill } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { LogoMark } from '@/shell/LogoMark';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

/** Where organizations are created, entered and administered: from the platform organization, for those allowed. */
const MANAGE_ORGANIZATIONS_PATH = '/iam/organizations';
/** Where the organization's name, logo and description are set. */
const ORGANIZATION_SETTINGS_PATH = '/settings/general';
const MEMBERS_PATH = '/iam/users';

/** The letter an organization without a logo of its own is told apart by in the list. */
export function orgInitial(name: string): string {
  return name.replace(/[^\p{L}\p{N}]/gu, '').charAt(0).toUpperCase() || '·';
}

/**
 * An organization's mark: its own logo, else the platform's. Beside the name in the top bar it
 * stands bare, without a tile or an edge; at the head of the menu it sits on a tile.
 */
export function OrgMark({ logo, size }: { logo: string | null | undefined; size: 'md' | 'lg' }) {
  if (size === 'md') {
    return (
      <span className="grid h-7 w-7 shrink-0 place-items-center">
        {logo ? (
          <img src={logo} alt="" className="h-full w-full rounded-[6px] object-contain outline-none" />
        ) : (
          <LogoMark size={24} />
        )}
      </span>
    );
  }
  if (logo) {
    return <img src={logo} alt="" className="h-11 w-11 shrink-0 rounded-[10px] bg-bg-2 object-contain" />;
  }
  return (
    <span className="grid h-11 w-11 shrink-0 place-items-center rounded-[10px] bg-bg-2">
      <LogoMark size={28} />
    </span>
  );
}

/** The reader's role in an organization, as a pill like the roles of the member list; a long one is cut. */
function RolePill({ name, className }: { name: string; className?: string }) {
  return (
    <Pill tone="neutral" className={cn('max-w-full', className)}>
      <span className="min-w-0 truncate">{name}</span>
    </Pill>
  );
}

/** Another organization in the list: its own logo, else its initial. */
function OrgListMark({ org }: { org: Org }) {
  if (org.logo) {
    return <img src={org.logo} alt="" className="h-6 w-6 shrink-0 rounded-[6px] bg-bg-2 object-contain" />;
  }
  return (
    <span className="grid h-6 w-6 shrink-0 place-items-center rounded-[6px] bg-bg-2 font-sans text-xs font-strong text-tx-1">
      {orgInitial(org.name)}
    </span>
  );
}

/**
 * The organization the reader works in, after Stripe's account switcher: its mark and its name, one
 * button. Opened, the organization heads the menu with the reader's role in it and its description, then
 * the ways to its settings and members, then the organizations to switch to; whoever may administer
 * organizations finds the way there at the foot.
 */
export function OrgSwitcher({
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
  const canOpenSettings = canAccessProductPath(ORGANIZATION_SETTINGS_PATH, access);
  const canOpenMembers = canAccessProductPath(MEMBERS_PATH, access);
  // The name on the button is the one the list marks, so the two never disagree.
  const current = orgOptions.find((org) => org.id === currentOrgId);
  const currentName = current?.name ?? orgName;
  const currentRole = current?.displayRole ?? role;
  const currentRoleName = currentRole ? roleName(currentRole) : null;
  const description = current?.description?.trim() ?? '';
  const others = orgOptions.filter((org) => org.id !== currentOrgId);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          data-testid="org-switcher-trigger"
          aria-label={t('shell:topbar.organization_menu', { name: currentName })}
          className={cn(
            'group ms-press flex h-[40px] min-w-0 max-w-[240px] shrink-0 items-center gap-2.5 rounded-md pl-1.5 pr-2.5 text-left text-tx-0',
            'hover:bg-bg-3 focus-visible:bg-indigo-dim focus-visible:outline-none data-[state=open]:bg-bg-3',
            className,
          )}
        >
          <OrgMark logo={current?.logo} size="md" />
          {/* The name alone, set as the product's name was before it, beside a mark of the same
              size; the role is told in the menu. */}
          <span className="hidden min-w-0 truncate font-sans text-md font-bold tracking-[-0.02em] sm:block">
            {currentName}
          </span>
          <ChevronDown
            aria-hidden
            className="hidden h-3.5 w-3.5 shrink-0 text-tx-3 transition-transform duration-fast group-data-[state=open]:rotate-180 sm:block"
          />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" data-ui="org-switcher-menu" className="w-[240px] p-1.5">
        <div className="flex flex-col items-center px-3 py-3 text-center">
          <OrgMark logo={current?.logo} size="lg" />
          <div className="mt-2.5 max-w-full truncate font-sans text-md font-display text-tx-0">{currentName}</div>
          {currentRoleName && (
            <span data-testid="current-org-role" className="mt-1.5 flex max-w-full">
              <RolePill name={currentRoleName} />
            </span>
          )}
          {description && (
            <p
              data-testid="current-org-description"
              className="mt-2 line-clamp-2 max-w-full font-sans text-xs leading-[18px] text-tx-2 [text-wrap:balance]"
            >
              {description}
            </p>
          )}
        </div>
        {canOpenSettings && (
          <DropdownMenuItem className="min-h-9 gap-2.5 rounded-md" onSelect={() => navigate(ORGANIZATION_SETTINGS_PATH)}>
            <Settings className="h-4 w-4 text-tx-2" />
            {t('shell:topbar.organization_settings')}
          </DropdownMenuItem>
        )}
        {canOpenMembers && (
          <DropdownMenuItem className="min-h-9 gap-2.5 rounded-md" onSelect={() => navigate(MEMBERS_PATH)}>
            <Users className="h-4 w-4 text-tx-2" />
            {t('shell:topbar.members')}
          </DropdownMenuItem>
        )}
        {others.length > 0 && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="type-micro px-[10px] pb-1 pt-2 font-sans font-medium tracking-normal text-tx-3">
              {t('shell:topbar.switch_to')}
            </DropdownMenuLabel>
            {others.map((org) => (
              <DropdownMenuItem
                key={org.id}
                data-ui="workspace-option"
                className="min-h-9 gap-2.5 rounded-md"
                disabled={org.disabled}
                onSelect={() => onSwitch(org.id)}
              >
                <OrgListMark org={org} />
                <span className="min-w-0 flex-1 truncate font-sans text-sm">{org.name}</span>
                {org.disabled ? (
                  <span className="shrink-0 font-sans text-xs text-tx-3">{t('common:status.disabled')}</span>
                ) : (
                  org.displayRole && <RolePill name={roleName(org.displayRole)} className="max-w-[112px] shrink-0" />
                )}
              </DropdownMenuItem>
            ))}
          </>
        )}
        {canManage && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem className="min-h-9 gap-2.5 rounded-md" onSelect={() => navigate(MANAGE_ORGANIZATIONS_PATH)}>
              <Building2 className="h-4 w-4 text-tx-2" />
              {t('shell:topbar.manage_organizations')}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
