import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as meApi from '@/api/me';
import { LAST_VISITED_HOME } from '@/lib/homeRoute';
import { buildTimezoneOptions, FOLLOW_TZ_SENTINEL } from '@/lib/time';
import {
  canAccessProductPath,
  hasPermission,
  useProductAccess,
} from '@/product/access';
import { DisabledControl } from '@/shell/DisabledControl';
import { FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import {
  DateTimeFormatPopover,
  type PreferenceOption,
  SegmentedControl,
  TimezoneCombobox,
} from '@/shell/PreferencesFields/controls';
import { Switch } from '@/shell/ui/switch';
import type { Dashboard } from '@/types/dashboard';

const CUSTOM_DASHBOARD = '__custom_dashboard__';

export function normalizeUserPreferences(
  preferences: meApi.UserPreferences,
): meApi.UserPreferences {
  return {
    ...preferences,
    // The product exposes one comfortable baseline and one compact mode.
    // Older "comfortable" rows converge on the baseline token set.
    density:
      preferences.density === 'comfortable' ? 'normal' : preferences.density,
  };
}

export function userPreferencesEqual(
  left: meApi.UserPreferences,
  right: meApi.UserPreferences,
): boolean {
  return (
    left.theme === right.theme &&
    left.density === right.density &&
    left.language === right.language &&
    left.default_home_route === right.default_home_route &&
    left.time_format === right.time_format &&
    left.date_format === right.date_format &&
    left.timezone === right.timezone &&
    left.keyboard_shortcuts_enabled === right.keyboard_shortcuts_enabled
  );
}

function dashboardIdFromHomeRoute(route: string): string | null {
  const match = /^\/dashboards\/([^/]+)$/.exec(route);
  const encodedId = match?.[1];
  if (!encodedId || encodedId === 'import') return null;
  try {
    return decodeURIComponent(encodedId);
  } catch {
    return null;
  }
}

function dashboardHomeRoute(id: string): string {
  return `/dashboards/${encodeURIComponent(id)}`;
}

export function PreferencesFields({
  value,
  dashboards,
  onChange,
  onThemePreview,
  surface = 'drawer',
  disabled = false,
  readOnly = false,
  disabledReason,
}: {
  value: meApi.UserPreferences;
  dashboards: Dashboard[];
  onChange: (patch: Partial<meApi.UserPreferences>) => void;
  onThemePreview: (theme: meApi.PreferenceTheme) => void;
  surface?: 'drawer' | 'page';
  disabled?: boolean;
  readOnly?: boolean;
  disabledReason?: string | undefined;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const access = useProductAccess();
  const controlsDisabled = disabled || readOnly;
  const controlsDisabledReason =
    disabledReason ?? (controlsDisabled ? tc('access.read_only') : undefined);
  const unavailableHomeReason = tc('access.page_unavailable');
  const homeOption = (
    value: string,
    label: string,
  ): PreferenceOption<string> => ({
    value,
    label,
    disabled: !canAccessProductPath(value, access),
    disabledReason: unavailableHomeReason,
  });
  const dashboardId = dashboardIdFromHomeRoute(value.default_home_route);
  const homeOptions: Array<PreferenceOption<string>> = [
    homeOption('/home', t('preferences.values.home_home')),
    {
      value: LAST_VISITED_HOME,
      label: t('preferences.values.home_last_visited'),
    },
    homeOption('/dashboards', t('preferences.values.home_dashboards')),
    homeOption('/logs', t('preferences.values.home_logs')),
    homeOption('/metrics', t('preferences.values.home_metrics')),
    homeOption('/traces', t('preferences.values.home_traces')),
    homeOption(
      '/alerts/incidents',
      t('preferences.values.home_alerts'),
    ),
    homeOption(
      '/intelligence/chat',
      t('preferences.values.home_intelligence'),
    ),
  ];
  const canReadDashboards = hasPermission('dashboards.read', access);
  homeOptions.push({
    value: CUSTOM_DASHBOARD,
    label: t('preferences.values.home_custom_dashboard'),
    disabled: !canReadDashboards || (!dashboardId && dashboards.length === 0),
    disabledReason: !canReadDashboards
      ? tc('access.permission_required', {
          permission: 'dashboards.read',
        })
      : t('preferences.values.home_dashboard_unavailable'),
  });
  const knownHomeRoute = homeOptions.some(
    (option) => option.value === value.default_home_route,
  );
  const homeSelection = dashboardId
    ? CUSTOM_DASHBOARD
    : knownHomeRoute
      ? value.default_home_route
      : '/home';
  const dashboardOptions: Array<PreferenceOption<string>> = dashboards.map(
    (dashboard) => ({
      value: dashboard.id,
      label: dashboard.title,
    }),
  );
  if (
    dashboardId &&
    !dashboardOptions.some((option) => option.value === dashboardId)
  ) {
    dashboardOptions.unshift({
      value: dashboardId,
      label: t('preferences.values.home_dashboard_unavailable'),
    });
  }

  const openKeyboardHelp = () => {
    window.dispatchEvent(new CustomEvent('molesignal:open-help'));
  };

  return (
    <>
      <PreferenceSection
        title={t('preferences.sections.appearance')}
        surface={surface}
        first
      >
        <PreferenceRow
          label={t('preferences.fields.default_theme')}
          surface={surface}
        >
          <SegmentedControl
            disabled={controlsDisabled}
            disabledReason={controlsDisabledReason}
            ariaLabel={t('preferences.fields.default_theme')}
            value={value.theme}
            onChange={(theme) => {
              onChange({ theme });
              onThemePreview(theme);
            }}
            options={[
              {
                value: 'system',
                label: t('preferences.values.theme_system'),
              },
              {
                value: 'light',
                label: t('preferences.values.theme_light'),
              },
              {
                value: 'dark',
                label: t('preferences.values.theme_dark'),
              },
            ]}
          />
        </PreferenceRow>
        <PreferenceRow
          label={t('preferences.fields.density')}
          description={t('preferences.density_hint')}
          surface={surface}
        >
          <SegmentedControl
            disabled={controlsDisabled}
            disabledReason={controlsDisabledReason}
            ariaLabel={t('preferences.fields.density')}
            value={value.density === 'compact' ? 'compact' : 'normal'}
            onChange={(density) =>
              onChange({ density: density as meApi.PreferenceDensity })
            }
            options={[
              {
                value: 'normal',
                label: t('preferences.values.density_comfortable'),
              },
              {
                value: 'compact',
                label: t('preferences.values.density_compact'),
              },
            ]}
          />
        </PreferenceRow>
      </PreferenceSection>

      <PreferenceSection
        title={t('preferences.sections.locale')}
        surface={surface}
      >
        <PreferenceRow
          label={t('preferences.fields.language')}
          surface={surface}
        >
          <FormSelect
            disabled={controlsDisabled}
            disabledReason={controlsDisabledReason}
            value={value.language}
            onChange={(language) =>
              onChange({ language: language as meApi.PreferenceLanguage })
            }
            ariaLabel={t('preferences.fields.language')}
            options={[
              {
                value: 'zh-cn',
                label: t('preferences.values.language_zh_cn'),
              },
              {
                value: 'en-us',
                label: t('preferences.values.language_en_us'),
              },
            ]}
          />
        </PreferenceRow>
        <PreferenceRow
          label={t('preferences.fields.default_timezone')}
          description={t('preferences.default_timezone_hint')}
          surface={surface}
        >
          <TimezoneCombobox
            disabled={controlsDisabled}
            disabledReason={controlsDisabledReason}
            value={value.timezone || FOLLOW_TZ_SENTINEL}
            onChange={(timezone) =>
              onChange({
                timezone: timezone === FOLLOW_TZ_SENTINEL ? '' : timezone,
              })
            }
            label={t('preferences.fields.default_timezone')}
            searchPlaceholder={t('preferences.timezone_search')}
            emptyLabel={t('preferences.timezone_empty')}
            options={buildTimezoneOptions(
              t('preferences.values.timezone_browser'),
            )}
          />
        </PreferenceRow>
        <PreferenceRow
          label={t('preferences.fields.date_time_format')}
          surface={surface}
        >
          <DateTimeFormatPopover
            value={value}
            onChange={onChange}
            disabled={controlsDisabled}
            disabledReason={controlsDisabledReason}
          />
        </PreferenceRow>
      </PreferenceSection>

      <PreferenceSection
        title={t('preferences.sections.startup_interaction')}
        surface={surface}
        last
      >
        <PreferenceRow
          label={t('preferences.fields.open_after_signin')}
          description={t('preferences.default_home_hint')}
          surface={surface}
        >
          <div className="space-y-2">
            <FormSelect
              disabled={controlsDisabled}
              disabledReason={controlsDisabledReason}
              value={homeSelection}
              onChange={(selection) => {
                if (selection === CUSTOM_DASHBOARD) {
                  const nextDashboardId = dashboardId ?? dashboards[0]?.id;
                  if (nextDashboardId) {
                    onChange({
                      default_home_route:
                        dashboardHomeRoute(nextDashboardId),
                    });
                  }
                  return;
                }
                onChange({ default_home_route: selection });
              }}
              ariaLabel={t('preferences.fields.open_after_signin')}
              options={homeOptions}
            />
            {homeSelection === CUSTOM_DASHBOARD && dashboardId && (
              <FormSelect
                disabled={controlsDisabled || !canReadDashboards}
                disabledReason={
                  controlsDisabled
                    ? controlsDisabledReason
                    : tc('access.permission_required', {
                        permission: 'dashboards.read',
                      })
                }
                value={dashboardId}
                onChange={(nextDashboardId) =>
                  onChange({
                    default_home_route:
                      dashboardHomeRoute(nextDashboardId),
                  })
                }
                ariaLabel={t('preferences.fields.dashboard')}
                options={dashboardOptions}
              />
            )}
          </div>
        </PreferenceRow>
        <PreferenceRow
          label={t('preferences.fields.keyboard_shortcuts')}
          description={
            <span>
              {t('preferences.keyboard_shortcuts_hint')}{' '}
              <button
                type="button"
                onClick={openKeyboardHelp}
                className="font-strong text-indigo hover:underline"
              >
                {t('preferences.actions.view_shortcuts')}
              </button>
            </span>
          }
          surface={surface}
          controlClassName={cn(surface === 'page' && 'flex justify-end')}
        >
          <DisabledControl
            disabled={controlsDisabled}
            reason={controlsDisabledReason}
          >
            <Switch
              disabled={controlsDisabled}
              aria-disabled={controlsDisabled || undefined}
              checked={value.keyboard_shortcuts_enabled}
              onCheckedChange={(keyboardShortcutsEnabled) =>
                onChange({
                  keyboard_shortcuts_enabled: keyboardShortcutsEnabled,
                })
              }
              aria-label={t('preferences.fields.keyboard_shortcuts')}
            />
          </DisabledControl>
        </PreferenceRow>
      </PreferenceSection>
    </>
  );
}

function PreferenceSection({
  title,
  children,
  surface,
  first = false,
  last = false,
}: {
  title: string;
  children: React.ReactNode;
  surface: 'drawer' | 'page';
  first?: boolean;
  last?: boolean;
}) {
  return (
    <section
      className={cn(
        surface === 'drawer'
          ? 'px-5 py-5 sm:px-6'
          : cn('py-5', first && 'pt-0'),
        surface === 'drawer' && !last && 'border-b border-bd-0',
      )}
    >
      <h3 className="mb-4 font-sans text-sm font-strong text-tx-0">
        {title}
      </h3>
      <div className="space-y-5">{children}</div>
    </section>
  );
}

function PreferenceRow({
  label,
  description,
  surface,
  controlClassName,
  children,
}: {
  label: string;
  description?: React.ReactNode;
  surface: 'drawer' | 'page';
  controlClassName?: string;
  children: React.ReactNode;
}) {
  return (
    <div
      data-preference-row
      className={cn(
        'min-h-[52px]',
        surface === 'page'
          ? 'grid grid-cols-1 items-start gap-3 min-[1100px]:grid-cols-[260px_minmax(420px,1fr)] min-[1100px]:gap-8'
          : 'grid grid-cols-1 gap-2 sm:grid-cols-[minmax(150px,1fr)_minmax(240px,320px)] sm:items-center sm:gap-6',
      )}
    >
      <div className="min-w-0">
        <div className="font-sans text-xs font-strong text-tx-1">
          {label}
        </div>
        {description && (
          <div className="mt-1 text-xs leading-5 text-tx-3">
            {description}
          </div>
        )}
      </div>
      <div
        className={cn(
          'min-w-0',
          surface === 'page' && 'w-full',
          controlClassName,
        )}
      >
        {children}
      </div>
    </div>
  );
}
