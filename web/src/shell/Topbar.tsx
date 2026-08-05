import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Bell,
  BookOpen,
  Bot,
  Check,
  ChevronDown,
  CreditCard,
  HelpCircle,
  Info,
  Keyboard,
  LifeBuoy,
  LogOut,
  Monitor,
  Moon,
  Palette,
  PanelLeft,
  Search,
  Sun,
  UserRound,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as meApi from '@/api/me';
import { resolveDefaultHomeRoute } from '@/lib/homeRoute';
import { toApiError } from '@/lib/http';
import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import {
  FEATURE_DEFINITIONS,
  selectFeatureGate,
  useEditionMetadata,
} from '@/product/edition';
import { useFeatureGateCopy } from '@/product/FeatureGate';
import { AboutDialog } from '@/shell/AboutDialog';
import { cn } from '@/shell/lib/cn';
import { LogoMark } from '@/shell/LogoMark';
import { NotificationCenter } from '@/shell/NotificationCenter';
import {
  USER_PREFERENCES_QUERY_KEY,
  useApplyUserPreferences,
} from '@/shell/PreferenceRuntime';
import { SystemStatusIndicator } from '@/shell/SystemStatusIndicator';
import { useTheme } from '@/shell/ThemeBootstrap';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { Kbd } from '@/shell/ui/kbd';
import { toast } from '@/shell/ui/sonner';
import { normalizeRole, useAuthStore } from '@/stores/auth';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import { useCurrentOrgSelection, useOrgStore } from '@/stores/useOrgStore';

interface TopbarProps {
  onToggleSidebar: () => void;
  onPaletteOpen: () => void;
  onNocOpen: () => void;
}

export function Topbar({ onToggleSidebar, onPaletteOpen, onNocOpen }: TopbarProps) {
  const { t, i18n } = useTranslation([
    'shell',
    'common',
    'errors',
    'nav',
    'account',
  ]);
  const ctx = useAuthStore((s) => s.ctx);
  const logout = useAuthStore((s) => s.logout);
  const nav = useNavigate();
  const queryClient = useQueryClient();
  const { theme, themePreference, setTheme } = useTheme();
  const applyUserPreferences = useApplyUserPreferences();
  const { currentOrgId, orgLabel, orgOptions } = useCurrentOrgSelection();
  const switchOrg = useOrgStore((s) => s.switchOrg);
  const toggleMoleAgent = useMoleAgentStore((s) => s.toggle);
  const [aboutOpen, setAboutOpen] = React.useState(false);
  const profileQuery = useQuery({
    queryKey: ['me', 'profile'],
    queryFn: () => meApi.profile(),
    enabled: !!ctx,
  });
  const profile = profileQuery.data;
  const displayName = profile?.display_name ?? ctx?.display_name ?? ctx?.user_id ?? 'unknown';
  const email = profile?.email ?? ctx?.email ?? '';
  const currentOrgName = profile?.org_name ?? ctx?.org_name ?? orgLabel;
  const role = normalizeRole(profile?.display_role ?? ctx?.display_role);
  const access = useProductAccess();
  const canUseMoleAgent = canAccessProductPath('/intelligence', access);
  const editionMetadata = useEditionMetadata();
  const billingGate = selectFeatureGate(
    editionMetadata,
    FEATURE_DEFINITIONS['saas-billing'],
  );
  const supportGate = selectFeatureGate(
    editionMetadata,
    FEATURE_DEFINITIONS['saas-support'],
  );
  const billingGateCopy = useFeatureGateCopy(billingGate);
  const supportGateCopy = useFeatureGateCopy(supportGate);
  const showBillingEntry = canAccessProductPath('/account/billing', access);
  const showSupportEntry = canAccessProductPath('/account/support', access);
  const avatarUrl = profile?.avatar_url?.trim() ?? '';

  const initial = displayName[0]?.toUpperCase() ?? 'M';
  const preferredHome = (orgId = ctx?.org_id ?? '') => {
    const preferences =
      queryClient.getQueryData<meApi.UserPreferences>(
        USER_PREFERENCES_QUERY_KEY,
      ) ?? meApi.DEFAULT_USER_PREFERENCES;
    return resolveDefaultHomeRoute(
      preferences.default_home_route,
      ctx?.user_id ?? '',
      orgId,
    );
  };
  const quickTheme = useMutation({
    mutationFn: async (nextTheme: meApi.PreferenceTheme) => {
      const current =
        queryClient.getQueryData<meApi.UserPreferences>(
          USER_PREFERENCES_QUERY_KEY,
        ) ?? (await meApi.preferences());
      const next = { ...current, theme: nextTheme };
      return meApi.updatePreferences(next);
    },
    onMutate: async (nextTheme) => {
      await queryClient.cancelQueries({
        queryKey: USER_PREFERENCES_QUERY_KEY,
      });
      const previous = queryClient.getQueryData<meApi.UserPreferences>(
        USER_PREFERENCES_QUERY_KEY,
      );
      setTheme(nextTheme);
      if (previous) {
        queryClient.setQueryData(USER_PREFERENCES_QUERY_KEY, {
          ...previous,
          theme: nextTheme,
        });
      }
      return { previous, previousTheme: themePreference };
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(USER_PREFERENCES_QUERY_KEY, saved);
      applyUserPreferences(saved);
    },
    onError: (error, _nextTheme, context) => {
      if (context?.previous) {
        queryClient.setQueryData(
          USER_PREFERENCES_QUERY_KEY,
          context.previous,
        );
      }
      setTheme(context?.previousTheme ?? themePreference);
      toast.error(toApiError(error).message);
    },
  });

  const handleSwitchOrg = async (id: string) => {
    if (id === currentOrgId) return;
    try {
      // switchOrg clears the query cache for tenant isolation, so resolve the
      // user-scoped startup preference before changing workspaces.
      const destination = preferredHome(id);
      const next = await switchOrg(id, { queryClient });
      toast.success(t('errors:switch_org_success', { name: next.name }));
      nav(destination);
    } catch (err) {
      const e = toApiError(err);
      toast.error(t('errors:switch_org_failure', { message: e.message }));
    }
  };

  return (
    <header
      role="banner"
      className="fixed inset-x-0 top-0 z-50 flex h-topbar min-w-0 items-center gap-3 overflow-hidden border-b border-bd-0 bg-bg-1 px-4"
    >
      {/* brand */}
      <button
        type="button"
        onClick={() => nav(preferredHome())}
        className="flex shrink-0 items-center gap-2 rounded-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
        aria-label={t('nav:home')}
        title={t('nav:home')}
      >
        <LogoMark size={24} />
        <span className="hidden font-sans text-md font-bold tracking-[-0.02em] text-tx-0 sm:inline">
          MoleSignal
        </span>
      </button>

      <IconBtn onClick={onToggleSidebar} title={t('shell:topbar.toggle_sidebar')}>
        <PanelLeft className="h-3.5 w-3.5" />
      </IconBtn>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            className="flex h-9 max-w-[180px] items-center gap-2 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm font-semibold text-tx-0 transition-colors hover:bg-bg-3 focus:outline-none focus-visible:outline-none sm:max-w-[240px]"
            data-testid="org-switcher"
            aria-label={t('shell:chrome.org_switcher')}
            title={t('shell:chrome.org_switcher')}
          >
            <span className="truncate text-indigo-soft">{orgLabel}</span>
            <ChevronDown className="h-3 w-3 shrink-0 text-tx-2" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="min-w-[220px]">
          <DropdownMenuLabel>{t('shell:chrome.org_switcher')}</DropdownMenuLabel>
          <DropdownMenuSeparator />
          {orgOptions.map((org) => {
            const isCurrent = org.id === currentOrgId;
            return (
              <DropdownMenuItem
                key={org.id}
                data-current={isCurrent ? 'true' : undefined}
                disabled={org.disabled}
                onSelect={() => void handleSwitchOrg(org.id)}
              >
                <span className="flex w-4 items-center justify-center">
                  {isCurrent && <Check className="h-3.5 w-3.5" />}
                </span>
                <span className="truncate">{org.name}</span>
                {org.disabled && (
                  <span className="ml-auto text-xs text-tx-3">
                    {t('common:status.disabled')}
                  </span>
                )}
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>

      {/* Global search — the product's "launch anything" surface.
           Subtle indigo-hinted border on hover signals this isn't just
           another input; it's the universal entry point (⌘K). */}
      <button
        type="button"
        onClick={onPaletteOpen}
        className="flex h-9 min-w-9 flex-1 items-center gap-2.5 rounded-md border border-bd-1 bg-bg-2 px-3 text-left text-tx-3 transition-colors duration-fast ease-default hover:border-bd-2 hover:bg-bg-3 hover:text-tx-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo sm:max-w-[560px]"
        aria-label={t('shell:chrome.command_palette')}
        data-testid="command-palette-trigger"
      >
        <Search className="h-4 w-4" />
        <span className="hidden flex-1 truncate text-sm sm:block">
          {t('shell:topbar.search_placeholder')}
        </span>
        <Kbd className="hidden font-sans text-xs sm:inline-flex">⌘ K</Kbd>
      </button>

      {/* right cluster */}
      <div className="ml-auto flex shrink-0 items-center gap-1">
        {/* Mole Agent — shell-level slide-out, available from any route (⌘J). */}
        {canUseMoleAgent && (
          <button
            type="button"
            onClick={toggleMoleAgent}
            className="hidden h-9 items-center gap-2 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo lg:flex"
            title={t('shell:topbar.mole_agent')}
            data-testid="mole-agent-trigger"
          >
            <Bot className="h-4 w-4 text-indigo-soft" />
            <span className="hidden xl:inline">{t('shell:topbar.agent')}</span>
            <Kbd className="hidden font-sans text-xs xl:inline-flex">⌘ J</Kbd>
          </button>
        )}
        <button
          type="button"
          onClick={onNocOpen}
          className="hidden h-9 items-center gap-2 rounded-md border border-bd-1 bg-transparent px-2.5 font-sans text-sm font-medium text-tx-2 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo lg:flex"
          title={t('shell:topbar.open_noc')}
        >
          <Monitor className="h-4 w-4" />
          <span className="hidden xl:inline">{t('shell:topbar.noc')}</span>
        </button>
        <NotificationCenter />
        <HelpMenu
          title={t('shell:topbar.help')}
          docsLabel={t('shell:topbar.docs')}
          keyboardLabel={t('shell:topbar.keyboard_shortcuts')}
          aboutLabel={t('shell:topbar.about')}
          onAbout={() => setAboutOpen(true)}
          onDocs={() => {
            const docsLang = i18n.language?.toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US';
            window.open(`https://docs.molesignal.io/${docsLang}`, '_blank', 'noopener,noreferrer');
          }}
          onKeyboard={() => window.dispatchEvent(new CustomEvent('molesignal:open-help'))}
        />
        {/* Theme toggle — single sun/moon, flips dark ↔ light. */}
        <IconBtn
          onClick={() =>
            quickTheme.mutate(theme === 'dark' ? 'light' : 'dark')
          }
          title={t('shell:chrome.toggle_theme')}
          testid="theme-toggle"
          className="hidden md:flex"
        >
          {theme === 'dark' ? <Sun className="h-3.5 w-3.5" /> : <Moon className="h-3.5 w-3.5" />}
        </IconBtn>
        <SystemStatusIndicator />
        <span className="mx-1 h-5 w-px bg-bd-1" />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className="flex h-8 w-8 items-center justify-center overflow-hidden rounded-full border border-indigo/40 bg-indigo font-sans text-xs font-bold text-white focus:outline-none focus-visible:outline-none"
              aria-label={t('shell:chrome.user_menu')}
              data-testid="user-menu-trigger"
            >
              {avatarUrl ? (
                <img src={avatarUrl} alt="" className="h-full w-full object-cover" />
              ) : (
                initial
              )}
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="end"
            className="w-[300px] rounded-xl p-1.5"
          >
            <DropdownMenuLabel className="px-3 py-3">
              <div className="flex min-w-0 items-center gap-3">
                <span className="flex h-10 w-10 shrink-0 items-center justify-center overflow-hidden rounded-full border border-bd-1 bg-indigo font-sans text-sm font-bold text-white">
                  {avatarUrl ? (
                    <img
                      src={avatarUrl}
                      alt=""
                      className="h-full w-full object-cover"
                    />
                  ) : (
                    initial
                  )}
                </span>
                <span className="flex min-w-0 flex-col gap-0.5">
                  <span className="truncate font-sans text-sm font-semibold text-tx-0">
                    {displayName}
                  </span>
                  {email && (
                    <span
                      className="truncate font-sans text-xs font-normal text-tx-2"
                      title={email}
                    >
                      {email}
                    </span>
                  )}
                </span>
              </div>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />

            <DropdownMenuLabel className="type-micro px-3 pb-1 pt-2 font-sans font-medium uppercase tracking-[0.06em] text-tx-3">
              {t('shell:user_menu.current_workspace')}
            </DropdownMenuLabel>
            {orgOptions.length > 1 ? (
              <DropdownMenuSub>
                <DropdownMenuSubTrigger className="mx-1 min-h-10 rounded-md px-2 py-2">
                  <span className="flex min-w-0 flex-1 items-center gap-2">
                    <span className="truncate font-sans text-sm font-strong text-tx-0">
                      {currentOrgName}
                    </span>
                    <span
                      data-testid="current-workspace-role"
                      className="type-micro shrink-0 rounded-full border border-bd-0 bg-bg-3 px-2 py-0.5 font-sans font-strong text-tx-2"
                    >
                      {role}
                    </span>
                  </span>
                  <span className="shrink-0 font-sans text-xs font-strong text-tx-3">
                    {t('shell:user_menu.switch_workspace')}
                  </span>
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="w-60">
                  <DropdownMenuLabel>
                    {t('shell:user_menu.current_workspace')}
                  </DropdownMenuLabel>
                  <DropdownMenuSeparator />
                  {orgOptions.map((org) => {
                    const isCurrent = org.id === currentOrgId;
                    const orgRole = org.display_role ?? (isCurrent ? role : null);
                    return (
                      <DropdownMenuItem
                        key={org.id}
                        className="min-h-10 rounded-md"
                        disabled={org.disabled}
                        onSelect={() => void handleSwitchOrg(org.id)}
                      >
                        <span className="flex w-4 items-center justify-center">
                          {isCurrent && <Check className="h-3.5 w-3.5" />}
                        </span>
                        <span className="min-w-0 flex-1 truncate">
                          {org.name}
                        </span>
                        {orgRole && (
                          <span className="type-micro rounded-full bg-bg-3 px-1.5 py-0.5 font-sans text-tx-3">
                            {orgRole}
                          </span>
                        )}
                        {org.disabled && (
                          <span className="type-micro text-tx-3">
                            {t('common:status.disabled')}
                          </span>
                        )}
                      </DropdownMenuItem>
                    );
                  })}
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            ) : (
              <div
                className="mx-1 flex min-h-10 items-center gap-2 rounded-md px-2 py-2"
                aria-label={t('shell:user_menu.current_workspace')}
              >
                <span className="min-w-0 flex-1 truncate font-sans text-sm font-strong text-tx-0">
                  {currentOrgName}
                </span>
                <span
                  data-testid="current-workspace-role"
                  className="type-micro shrink-0 rounded-full border border-bd-0 bg-bg-3 px-2 py-0.5 font-sans font-strong text-tx-2"
                >
                  {role}
                </span>
              </div>
            )}

            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="min-h-10 rounded-md px-3"
              onSelect={() => nav('/account/settings/profile')}
            >
              <UserRound className="h-4 w-4 text-tx-2" />
              {t('account:title')}
            </DropdownMenuItem>
            <DropdownMenuItem
              className="min-h-10 rounded-md px-3"
              onSelect={() => nav('/account/settings/preferences')}
            >
              <Palette className="h-4 w-4 text-tx-2" />
              {t('account:nav.items.preferences')}
            </DropdownMenuItem>
            <DropdownMenuItem
              className="min-h-10 rounded-md px-3"
              onSelect={() => nav('/account/settings/notify')}
            >
              <Bell className="h-4 w-4 text-tx-2" />
              {t('account:nav.items.notify')}
            </DropdownMenuItem>
            {(showBillingEntry || showSupportEntry) && (
              <DropdownMenuSeparator />
            )}
            {showBillingEntry && (
              <DropdownMenuItem
                className="min-h-10 rounded-md px-3"
                disabled={billingGate.status !== 'allowed'}
                disabledReason={
                  billingGate.status !== 'allowed'
                    ? billingGateCopy.description
                    : undefined
                }
                onSelect={() => {
                  if (billingGate.status === 'allowed') {
                    nav('/account/billing');
                  }
                }}
              >
                <CreditCard className="h-4 w-4 text-tx-2" />
                {t('nav:account_billing')}
              </DropdownMenuItem>
            )}
            {showSupportEntry && (
              <DropdownMenuItem
                className="min-h-10 rounded-md px-3"
                disabled={supportGate.status !== 'allowed'}
                disabledReason={
                  supportGate.status !== 'allowed'
                    ? supportGateCopy.description
                    : undefined
                }
                onSelect={() => {
                  if (supportGate.status === 'allowed') {
                    nav('/account/support');
                  }
                }}
              >
                <LifeBuoy className="h-4 w-4 text-tx-2" />
                {t('nav:account_support')}
              </DropdownMenuItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="min-h-10 rounded-md px-3 text-tx-1 focus:bg-red-dim focus:text-red-soft"
              onSelect={() => {
                logout();
                useOrgStore.getState().reset();
                nav('/signin');
              }}
            >
              <LogOut className="h-4 w-4" />
              {t('common:actions.sign_out')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <AboutDialog open={aboutOpen} onOpenChange={setAboutOpen} />
    </header>
  );
}

function HelpMenu({
  title,
  docsLabel,
  keyboardLabel,
  aboutLabel,
  onDocs,
  onKeyboard,
  onAbout,
}: {
  title: string;
  docsLabel: string;
  keyboardLabel: string;
  aboutLabel: string;
  onDocs: () => void;
  onKeyboard: () => void;
  onAbout: () => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title={title}
          aria-label={title}
          className={cn(
            'relative hidden h-8 w-8 items-center justify-center rounded-md text-tx-2 md:flex',
            'hover:bg-bg-3 hover:text-tx-0',
            'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
          )}
        >
          <HelpCircle className="h-3.5 w-3.5" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuLabel>{title}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={onDocs}>
          <BookOpen className="h-4 w-4" />
          <span>{docsLabel}</span>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onAbout}>
          <Info className="h-4 w-4" />
          <span>{aboutLabel}</span>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onKeyboard}>
          <Keyboard className="h-4 w-4" />
          <span>{keyboardLabel}</span>
          <Kbd className="ml-auto font-sans text-xs">?</Kbd>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function IconBtn({
  children,
  title,
  badge = false,
  onClick,
  testid,
  className,
}: {
  children: React.ReactNode;
  title: string;
  badge?: boolean;
  onClick?: () => void;
  testid?: string;
  className?: string | undefined;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      data-testid={testid}
      className={cn(
        'relative flex h-8 w-8 items-center justify-center rounded-md text-tx-2',
        'hover:bg-bg-3 hover:text-tx-0',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
        className,
      )}
    >
      {children}
      {badge && (
        // Phase 4: notifications use the status `red` token (an incident
        // signal), not the brand accent.
        <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-red" />
      )}
    </button>
  );
}
