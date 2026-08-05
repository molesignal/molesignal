import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';

import * as meApi from '@/api/me';
import { StackPortal } from '@/investigation/StackPortal';
import { useSyncStateToUrl } from '@/investigation/syncUrl';
import { useBindings } from '@/keyboard/controller';
import { HelpOverlay } from '@/keyboard/HelpOverlay';
import { rememberLastVisitedRoute } from '@/lib/homeRoute';
import { CommandPalette } from '@/palette/CommandPalette';
import { canAccessProductPath, useProductAccess } from '@/product/access';
import { AppShell } from '@/shell/AppShell';
import {
  USER_PREFERENCES_QUERY_KEY,
  useApplyUserPreferences,
} from '@/shell/PreferenceRuntime';
import { toast } from '@/shell/ui/sonner';
import { hydrateFromSearchParams } from '@/shell/UrlHydration';
import { useAuthStore } from '@/stores/auth';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { useOrgStore } from '@/stores/useOrgStore';
import { useTimeStore } from '@/stores/useTimeStore';
import { TimePicker } from '@/time/TimePicker';

export function ShellRoot() {
  const { t } = useTranslation(['keyboard', 'errors']);
  const [search] = useSearchParams();
  const location = useLocation();
  const nav = useNavigate();
  const [paletteOpen, setPaletteOpen] = React.useState(false);
  const [pickerOpen, setPickerOpen] = React.useState(false);
  const [helpOpen, setHelpOpen] = React.useState(false);

  const stack = useInvestigationStack();
  const togglePin = useTimeStore((s) => s.togglePin);
  const token = useAuthStore((s) => s.token);
  const authContext = useAuthStore((s) => s.ctx);
  const access = useProductAccess();
  const loadOrgs = useOrgStore((s) => s.loadOrgs);
  const orgsLoaded = useOrgStore((s) => s.loaded);
  const applyUserPreferences = useApplyUserPreferences();
  const preferencesQuery = useQuery({
    queryKey: USER_PREFERENCES_QUERY_KEY,
    queryFn: () => meApi.preferences(),
    enabled: Boolean(token),
    staleTime: 5 * 60_000,
  });

  // Once the shell mounts under an authenticated session, fetch the user's
  // org memberships exactly once — the shell org switcher reads them.
  React.useEffect(() => {
    if (!token || orgsLoaded) return;
    void loadOrgs().catch(() => {
      // soft-fail: switcher will show a single-entry dropdown for current org
    });
  }, [token, orgsLoaded, loadOrgs]);

  React.useEffect(() => {
    if (preferencesQuery.data) applyUserPreferences(preferencesQuery.data);
  }, [applyUserPreferences, preferencesQuery.data]);

  React.useLayoutEffect(() => {
    hydrateFromSearchParams(search);
  }, [location.pathname, search]);

  React.useEffect(() => {
    if (!authContext || !canAccessProductPath(location.pathname, access)) return;
    rememberLastVisitedRoute(
      authContext.user_id,
      authContext.org_id,
      `${location.pathname}${location.search}`,
    );
  }, [access, authContext, location.pathname, location.search]);

  // Reverse direction: store → URL (debounced, replaceState).
  useSyncStateToUrl();

  // open-help via palette custom event
  React.useEffect(() => {
    const onHelp = () => setHelpOpen(true);
    window.addEventListener('molesignal:open-help', onHelp);
    return () => window.removeEventListener('molesignal:open-help', onHelp);
  }, []);

  // Global key bindings — descriptions go through i18n so the help overlay
  // re-renders correctly when the user switches language. `category` is also
  // a translation key so HelpOverlay can label section headers.
  useBindings('global', [
    { keys: 'mod+k', description: t('keyboard:bindings.open_palette'), category: t('keyboard:categories.general'), handler: () => setPaletteOpen(true) },
    { keys: 'mod+/', description: t('keyboard:bindings.show_help'),  category: t('keyboard:categories.general'), handler: () => setHelpOpen(true) },
    { keys: 'mod+alt+e', description: t('keyboard:bindings.open_time_picker'), category: t('keyboard:categories.time'), handler: () => setPickerOpen(true) },
    { keys: 'mod+[', description: t('keyboard:bindings.stack_back'),    category: t('keyboard:categories.investigation'), handler: () => stack.back() },
    { keys: 'mod+]', description: t('keyboard:bindings.stack_forward'), category: t('keyboard:categories.investigation'), handler: () => stack.forwardOne() },
    { keys: 'mod+alt+s', description: t('keyboard:bindings.goto_apm_services'), category: t('keyboard:categories.navigation'), handler: () => nav('/apm/services') },
    { keys: 'mod+alt+a', description: t('keyboard:bindings.goto_incidents'),    category: t('keyboard:categories.navigation'), handler: () => nav('/alerts/incidents') },
    { keys: 'mod+alt+d', description: t('keyboard:bindings.goto_dashboards'),   category: t('keyboard:categories.navigation'), handler: () => nav('/dashboards') },
    { keys: 'mod+alt+t', description: t('keyboard:bindings.goto_investigate_trace'), category: t('keyboard:categories.navigation'), handler: () => nav('/investigate?preset=trace') },
    { keys: 'mod+alt+l', description: t('keyboard:bindings.goto_investigate_log'),   category: t('keyboard:categories.navigation'), handler: () => nav('/investigate?preset=log') },
    { keys: 'mod+alt+r', description: t('keyboard:bindings.goto_user_experience'),  category: t('keyboard:categories.navigation'), handler: () => nav('/rum/overview') },
    { keys: 'mod+alt+f', description: t('keyboard:bindings.goto_functions'),        category: t('keyboard:categories.navigation'), handler: () => nav('/functions') },
    { keys: 'mod+alt+i', description: t('keyboard:bindings.goto_iam'),              category: t('keyboard:categories.navigation'), handler: () => nav('/iam/users') },
    { keys: 'mod+alt+p', description: t('keyboard:bindings.pin_anchor'), category: t('keyboard:categories.time'), handler: () => togglePin(new Date().toISOString()) },
    {
      keys: 'mod+alt+y',
      description: t('keyboard:bindings.copy_link'),
      category: t('keyboard:categories.investigation'),
      handler: () => {
        void navigator.clipboard.writeText(window.location.href).then(() => toast.success(t('errors:link_copied')));
      },
    },
    { keys: 'mod+esc', description: t('keyboard:bindings.dismiss_overlay'), category: t('keyboard:categories.general'), handler: () => {
      if (helpOpen) setHelpOpen(false);
      else if (pickerOpen) setPickerOpen(false);
      else if (paletteOpen) setPaletteOpen(false);
      else stack.pop();
    } },
  ]);

  return (
    <>
      <AppShell
        onPaletteOpen={() => setPaletteOpen(true)}
        onTimePickerOpen={() => setPickerOpen(true)}
      />
      <StackPortal />
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
      <TimePicker open={pickerOpen} onOpenChange={setPickerOpen} />
      <HelpOverlay open={helpOpen} onOpenChange={setHelpOpen} />
    </>
  );
}
