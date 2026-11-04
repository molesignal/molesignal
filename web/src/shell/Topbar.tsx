import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Bell,
  BookOpen,
  Bot,
  HelpCircle,
  Info,
  Keyboard,
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
import { AboutDialog } from '@/shell/AboutDialog';
import { chromeIconButtonSizeClass, uiUserAvatarClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { LogoMark } from '@/shell/LogoMark';
import { NotificationCenter } from '@/shell/notifications/NotificationCenter';
import { OrgStatusSwitcher } from '@/shell/OrgStatusSwitcher';
import {
  USER_PREFERENCES_QUERY_KEY,
  useApplyUserPreferences,
} from '@/shell/PreferenceRuntime';
import { animateThemeChange, useTheme } from '@/shell/ThemeBootstrap';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { Kbd } from '@/shell/ui/kbd';
import { toast } from '@/shell/ui/sonner';
import { UserMenuLanguage } from '@/shell/UserMenuLanguage';
import { normalizeRole, useAuthStore } from '@/stores/auth';
import { signOut } from '@/stores/auth/signOut';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import { isOrgEntryDeclined } from '@/stores/useOrgEntry';
import { useCurrentOrgSelection, useOrgStore } from '@/stores/useOrgStore';

interface TopbarProps {
  onToggleSidebar: () => void;
  onPaletteOpen: () => void;
  onNocOpen: () => void;
  surfaceWorkbench?: boolean;
}

export function Topbar({
  onToggleSidebar,
  onPaletteOpen,
  onNocOpen,
  surfaceWorkbench = false,
}: TopbarProps) {
  const { t, i18n } = useTranslation([
    'shell',
    'common',
    'errors',
    'nav',
    'account',
  ]);
  const ctx = useAuthStore((s) => s.ctx);
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
  const displayName = profile?.displayName ?? ctx?.displayName ?? ctx?.userId ?? 'unknown';
  const email = profile?.email ?? ctx?.email ?? '';
  const currentOrgName = profile?.orgName ?? ctx?.orgName ?? orgLabel;
  const role = normalizeRole(profile?.displayRole ?? ctx?.displayRole);
  const access = useProductAccess();
  const canUseMoleAgent = canAccessProductPath('/agent', access);
  const avatarUrl = profile?.avatarUrl?.trim() ?? '';

  const initial = displayName[0]?.toUpperCase() ?? 'M';
  const preferredHome = (orgId = ctx?.orgId ?? '') => {
    const preferences =
      queryClient.getQueryData<meApi.UserPreferences>(
        USER_PREFERENCES_QUERY_KEY,
      ) ?? meApi.DEFAULT_USER_PREFERENCES;
    return resolveDefaultHomeRoute(
      preferences.defaultHomeRoute,
      ctx?.userId ?? '',
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
      if (isOrgEntryDeclined(err)) return;
      const e = toApiError(err);
      toast.error(t('errors:switch_org_failure', { message: e.message }));
    }
  };

  return (
    <header
      role="banner"
      className={cn(
        'fixed inset-x-0 top-0 z-50 flex h-topbar min-w-0 items-center gap-3 overflow-hidden px-4',
        surfaceWorkbench
          ? 'border-b-0 bg-[var(--app-header-surface)] shadow-sm'
          : 'border-b border-bd-0 bg-bg-1',
      )}
    >
      {/* brand */}
      <button
        type="button"
        onClick={() => nav(preferredHome())}
        className="group ms-press flex shrink-0 items-center gap-2 rounded-md focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none"
        aria-label={t('nav:home')}
        title={t('nav:home')}
      >
        <LogoMark size={24} />
        <span className="hidden font-sans text-md font-bold tracking-[-0.02em] text-tx-0 sm:inline">
          MoleSignal
        </span>
      </button>

      <IconBtn
        onClick={onToggleSidebar}
        title={t('shell:topbar.toggle_sidebar')}
        testid="sidebar-toggle"
      >
        <PanelLeft className="h-4 w-4" />
      </IconBtn>

      {/* Global search — the product's "launch anything" control surface. */}
      <button
        type="button"
        onClick={onPaletteOpen}
        className="flex h-9 w-9 shrink-0 items-center justify-center gap-2.5 rounded-md border-0 bg-[var(--control-surface)] px-0 sm:px-3 text-left text-tx-3 transition-colors duration-fast ease-default focus-visible:text-tx-3 focus-visible:outline-none sm:w-auto sm:max-w-[720px] sm:flex-1 sm:justify-start [@media(pointer:coarse)]:h-[44px] [@media(pointer:coarse)]:min-w-[44px]"
        aria-label={t('shell:chrome.command_palette')}
        data-ui="search-control"
        data-testid="command-palette-trigger"
      >
        <Search className="h-4 w-4" />
        <span className="hidden flex-1 truncate text-sm sm:block">
          {t('shell:topbar.search_placeholder')}
        </span>
        <Kbd variant="plain" className="hidden sm:inline-flex">⌘ K</Kbd>
      </button>

      {/* right cluster */}
      <div className="ml-auto flex shrink-0 items-center gap-1">
        {/* Mole Agent — shell-level slide-out, available from any route (⌘J). */}
        {canUseMoleAgent && (
          <IconBtn
            onClick={toggleMoleAgent}
            title={t('shell:topbar.mole_agent')}
            testid="mole-agent-trigger"
            className="hidden lg:flex"
          >
            <Bot className="h-4 w-4 text-indigo-soft" />
          </IconBtn>
        )}
        <IconBtn
          onClick={onNocOpen}
          title={t('shell:topbar.open_noc')}
          className="hidden lg:flex"
        >
          {/* A screen fills its box where the bell, the help ring and the moon are round or open:
              drawn at 14px it reads as large as they do at 16. */}
          <Monitor className="h-[14px] w-[14px]" />
        </IconBtn>
        <NotificationCenter />
        <HelpMenu
          title={t('shell:topbar.help')}
          docsLabel={t('shell:topbar.docs')}
          keyboardLabel={t('shell:topbar.keyboard_shortcuts')}
          aboutLabel={t('shell:topbar.about')}
          onAbout={() => setAboutOpen(true)}
          onDocs={() => {
            const docsLang = i18n.language?.toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US';
            window.open(`https://docs.molesignal.com/${docsLang}`, '_blank', 'noopener,noreferrer');
          }}
          onKeyboard={() => window.dispatchEvent(new CustomEvent('molesignal:open-help'))}
        />
        {/* Theme toggle — single sun/moon, flips dark ↔ light. */}
        <IconBtn
          onClick={(event) => {
            const nextTheme = theme === 'dark' ? 'light' : 'dark';
            // The visible change runs inside the view transition; saving the
            // preference follows it.
            animateThemeChange(event.currentTarget, () => setTheme(nextTheme));
            quickTheme.mutate(nextTheme);
          }}
          title={t('shell:chrome.toggle_theme')}
          testid="theme-toggle"
          className="hidden md:flex"
        >
          {theme === 'dark' ? (
            <Sun key="sun" className="h-4 w-4 animate-pop-in" />
          ) : (
            <Moon key="moon" className="h-4 w-4 animate-pop-in" />
          )}
        </IconBtn>
        <OrgStatusSwitcher
          orgName={currentOrgName}
          role={role}
          currentOrgId={currentOrgId}
          orgOptions={orgOptions}
          onSwitch={(id) => void handleSwitchOrg(id)}
          className="ml-1 mr-2"
        />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className={cn(
                'group ms-press flex items-center justify-center rounded-full focus:outline-none focus-visible:bg-indigo-dim focus-visible:outline-none',
                chromeIconButtonSizeClass,
              )}
              aria-label={t('shell:chrome.user_menu')}
              data-testid="user-menu-trigger"
            >
              {/* The disc is 28px; the button around it stays the shell's 32px
                  target (44px on touch), so the avatar reads smaller without
                  becoming harder to hit. */}
              <span
                data-testid="user-avatar"
                className={cn(
                  uiUserAvatarClass,
                  'h-[28px] w-[28px] text-md transition-[filter] duration-fast group-hover:brightness-95 group-focus-visible:brightness-90',
                )}
              >
                {avatarUrl ? (
                  <img src={avatarUrl} alt="" className="h-full w-full object-cover" />
                ) : (
                  initial
                )}
              </span>
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="end"
            data-ui="user-menu"
            className="w-[300px] p-1.5 shadow-none"
          >
            <DropdownMenuLabel className="px-3 py-3">
              <div className="flex min-w-0 items-center gap-3">
                <span className={cn(uiUserAvatarClass, 'h-10 w-10 text-lg')}>
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
            <UserMenuLanguage />
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="min-h-10 rounded-md px-3 text-tx-1 focus:bg-red-dim focus:text-red-soft"
              onSelect={() => {
                void signOut().then(() => nav('/signin'));
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
            'ms-press relative hidden items-center justify-center rounded-md text-tx-2 md:flex',
            chromeIconButtonSizeClass,
            'hover:bg-bg-3 hover:text-tx-0',
            'focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
          )}
        >
          <HelpCircle className="h-4 w-4" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
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
  onClick?: (event: React.MouseEvent<HTMLButtonElement>) => void;
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
        'ms-press relative flex items-center justify-center rounded-md text-tx-2',
        chromeIconButtonSizeClass,
        'hover:bg-bg-3 hover:text-tx-0',
        'focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
        className,
      )}
    >
      {children}
      {badge && (
        // Notification badges use status red to communicate incident state.
        <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-red" />
      )}
    </button>
  );
}
