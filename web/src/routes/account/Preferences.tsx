import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as dashboardsApi from '@/api/dashboards';
import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { AccountSection } from '@/routes/account/AccountSection';
import { ChromeButton } from '@/shell/chrome';
import {
  USER_PREFERENCES_QUERY_KEY,
  useApplyUserPreferences,
} from '@/shell/PreferenceRuntime';
import {
  normalizeUserPreferences,
  PreferencesFields,
  userPreferencesEqual,
} from '@/shell/PreferencesFields';
import { useTheme } from '@/shell/ThemeBootstrap';
import { toast } from '@/shell/ui/sonner';

export function AccountPreferences() {
  const { t } = useTranslation(['account', 'settings-admin', 'common']);
  const queryClient = useQueryClient();
  const { setTheme } = useTheme();
  const applyUserPreferences = useApplyUserPreferences();
  const access = useProductAccess();
  const canReadDashboards = hasPermission('dashboards.read', access);
  const [draft, setDraft] = React.useState<meApi.UserPreferences>(
    meApi.DEFAULT_USER_PREFERENCES,
  );
  const [baseline, setBaseline] = React.useState<meApi.UserPreferences>(
    meApi.DEFAULT_USER_PREFERENCES,
  );
  const editedRef = React.useRef(false);
  const preferencesQuery = useQuery({
    queryKey: USER_PREFERENCES_QUERY_KEY,
    queryFn: () => meApi.preferences(),
  });
  const dashboardsQuery = useQuery({
    queryKey: ['dashboards'],
    queryFn: () => dashboardsApi.list(),
    enabled: canReadDashboards,
    staleTime: 60_000,
  });

  React.useEffect(() => {
    if (!preferencesQuery.data || editedRef.current) return;
    const normalized = normalizeUserPreferences(preferencesQuery.data);
    setDraft(normalized);
    setBaseline(normalized);
  }, [preferencesQuery.data]);

  const save = useMutation({
    mutationFn: () => meApi.updatePreferences(draft),
    onSuccess: (saved) => {
      const normalized = normalizeUserPreferences(saved);
      editedRef.current = false;
      setDraft(normalized);
      setBaseline(normalized);
      queryClient.setQueryData(USER_PREFERENCES_QUERY_KEY, normalized);
      applyUserPreferences(normalized);
      toast.success(t('common:status.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const dirty = !userPreferencesEqual(draft, baseline);

  React.useEffect(() => {
    if (!dirty) return;
    const beforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
    };
    window.addEventListener('beforeunload', beforeUnload);
    return () => window.removeEventListener('beforeunload', beforeUnload);
  }, [dirty]);

  const reset = () => {
    editedRef.current = false;
    setDraft(baseline);
    setTheme(baseline.theme);
  };

  return (
    <AccountSection
      title={t('account:preferences.title')}
      subtitle={t('account:preferences.subtitle')}
    >
      {preferencesQuery.isError ? (
        <div className="rounded-md border border-red/30 bg-red-dim px-3 py-2 font-sans text-xs text-red-soft">
          {toApiError(preferencesQuery.error).message}
        </div>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (dirty && !save.isPending) save.mutate();
          }}
        >
          <PreferencesFields
            value={draft}
            dashboards={dashboardsQuery.data ?? []}
            onChange={(patch) => {
              editedRef.current = true;
              setDraft((current) => ({ ...current, ...patch }));
            }}
            onThemePreview={setTheme}
            surface="page"
            disabled={save.isPending}
            disabledReason={
              save.isPending
                ? t('common:access.operation_pending')
                : undefined
            }
          />
          <div className="mt-5 flex flex-wrap items-center justify-between gap-3">
            <span aria-live="polite" className="font-sans text-xs text-tx-3">
              {dirty ? t('settings-admin:preferences.unsaved') : ''}
            </span>
            <div className="flex items-center gap-2">
              <ChromeButton
                type="button"
                disabled={!dirty || save.isPending}
                disabledReason={
                  !dirty
                    ? t('common:access.no_changes')
                    : save.isPending
                      ? t('common:access.operation_pending')
                      : undefined
                }
                onClick={reset}
              >
                {t('common:actions.cancel')}
              </ChromeButton>
              <ChromeButton
                type="submit"
                variant="primary"
                disabled={!dirty || save.isPending}
                disabledReason={
                  !dirty
                    ? t('common:access.no_changes')
                    : save.isPending
                      ? t('common:access.operation_pending')
                      : undefined
                }
              >
                {save.isPending
                  ? t('settings-admin:preferences.actions.saving')
                  : t('settings-admin:preferences.actions.save')}
              </ChromeButton>
            </div>
          </div>
        </form>
      )}
    </AccountSection>
  );
}
