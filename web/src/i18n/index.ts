import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import enAccount from './en-us/account.json';
import enAlerts from './en-us/alerts.json';
import enApm from './en-us/apm.json';
import enCommon from './en-us/common.json';
import enDashboards from './en-us/dashboards.json';
import enDesignSystem from './en-us/design-system.json';
import enEdition from './en-us/edition.json';
import enErrors from './en-us/errors.json';
import enFunctions from './en-us/functions.json';
import enIam from './en-us/iam.json';
import enIntelligence from './en-us/intelligence.json';
import enKeyboard from './en-us/keyboard.json';
import enLogs from './en-us/logs.json';
import enMetrics from './en-us/metrics.json';
import enNav from './en-us/nav.json';
import enNotify from './en-us/notify.json';
import enOnboarding from './en-us/onboarding.json';
import enPalette from './en-us/palette.json';
import enPipelines from './en-us/pipelines.json';
import enProduct from './en-us/product.json';
import enProfiles from './en-us/profiles.json';
import enReports from './en-us/reports.json';
import enRum from './en-us/rum.json';
import enSavedViews from './en-us/saved-views.json';
import enSemanticGroups from './en-us/semantic-groups.json';
import enServices from './en-us/services.json';
import enSettingsAdmin from './en-us/settings-admin.json';
import enShell from './en-us/shell.json';
import enStreams from './en-us/streams.json';
import enTraces from './en-us/traces.json';
import zhAccount from './zh-cn/account.json';
import zhAlerts from './zh-cn/alerts.json';
import zhApm from './zh-cn/apm.json';
import zhCommon from './zh-cn/common.json';
import zhDashboards from './zh-cn/dashboards.json';
import zhDesignSystem from './zh-cn/design-system.json';
import zhEdition from './zh-cn/edition.json';
import zhErrors from './zh-cn/errors.json';
import zhFunctions from './zh-cn/functions.json';
import zhIam from './zh-cn/iam.json';
import zhIntelligence from './zh-cn/intelligence.json';
import zhKeyboard from './zh-cn/keyboard.json';
import zhLogs from './zh-cn/logs.json';
import zhMetrics from './zh-cn/metrics.json';
import zhNav from './zh-cn/nav.json';
import zhNotify from './zh-cn/notify.json';
import zhOnboarding from './zh-cn/onboarding.json';
import zhPalette from './zh-cn/palette.json';
import zhPipelines from './zh-cn/pipelines.json';
import zhProduct from './zh-cn/product.json';
import zhProfiles from './zh-cn/profiles.json';
import zhReports from './zh-cn/reports.json';
import zhRum from './zh-cn/rum.json';
import zhSavedViews from './zh-cn/saved-views.json';
import zhSemanticGroups from './zh-cn/semantic-groups.json';
import zhServices from './zh-cn/services.json';
import zhSettingsAdmin from './zh-cn/settings-admin.json';
import zhShell from './zh-cn/shell.json';
import zhStreams from './zh-cn/streams.json';
import zhTraces from './zh-cn/traces.json';

export const SUPPORTED_LOCALES = ['en-us', 'zh-cn'] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];

export const DEFAULT_LOCALE: Locale = 'en-us';

/**
 * Locales used to store in persisted state before the BCP-47 rename.
 * Kept exported so `useThemeStore`'s migration step can map legacy
 * values forward without re-implementing the same lookup.
 */
export const LEGACY_LOCALE_MAP: Record<string, Locale> = {
  en: 'en-us',
  'zh-CN': 'zh-cn',
};

const NAMESPACES = [
  'account',
  'apm',
  'common',
  'intelligence',
  'palette',
  'keyboard',
  'nav',
  'notify',
  'errors',
  'shell',
  'product',
  'design-system',
  'edition',
  'onboarding',
  'rum',
  'profiles',
  'functions',
  'streams',
  'dashboards',
  'pipelines',
  'alerts',
  'reports',
  'traces',
  'logs',
  'metrics',
  'iam',
  'saved-views',
  'semantic-groups',
  'services',
  'settings-admin',
] as const;

const resources = {
  'en-us': {
    account: enAccount,
    apm: enApm,
    common: enCommon,
    intelligence: enIntelligence,
    palette: enPalette,
    keyboard: enKeyboard,
    nav: enNav,
    notify: enNotify,
    errors: enErrors,
    shell: enShell,
    product: enProduct,
    'design-system': enDesignSystem,
    edition: enEdition,
    onboarding: enOnboarding,
    rum: enRum,
    profiles: enProfiles,
    functions: enFunctions,
    streams: enStreams,
    dashboards: enDashboards,
    pipelines: enPipelines,
    alerts: enAlerts,
    reports: enReports,
    traces: enTraces,
    logs: enLogs,
    metrics: enMetrics,
    iam: enIam,
    'saved-views': enSavedViews,
    'semantic-groups': enSemanticGroups,
    services: enServices,
    'settings-admin': enSettingsAdmin,
  },
  'zh-cn': {
    account: zhAccount,
    apm: zhApm,
    common: zhCommon,
    intelligence: zhIntelligence,
    palette: zhPalette,
    keyboard: zhKeyboard,
    nav: zhNav,
    notify: zhNotify,
    errors: zhErrors,
    shell: zhShell,
    product: zhProduct,
    'design-system': zhDesignSystem,
    edition: zhEdition,
    onboarding: zhOnboarding,
    rum: zhRum,
    profiles: zhProfiles,
    functions: zhFunctions,
    streams: zhStreams,
    dashboards: zhDashboards,
    pipelines: zhPipelines,
    alerts: zhAlerts,
    reports: zhReports,
    traces: zhTraces,
    logs: zhLogs,
    metrics: zhMetrics,
    iam: zhIam,
    'saved-views': zhSavedViews,
    'semantic-groups': zhSemanticGroups,
    services: zhServices,
    'settings-admin': zhSettingsAdmin,
  },
} as const;

/**
 * Best-effort detect a supported locale from the browser. Anything beginning
 * with `zh` resolves to `zh-cn`; everything else falls back to `en-us`. This
 * is only consulted on the first visit (before the user picks a language).
 */
export function detectBrowserLocale(): Locale {
  if (typeof navigator === 'undefined') return DEFAULT_LOCALE;
  const lang = navigator.language ?? DEFAULT_LOCALE;
  if (lang.toLowerCase().startsWith('zh')) return 'zh-cn';
  return 'en-us';
}

void i18n.use(initReactI18next).init({
  resources,
  lng: DEFAULT_LOCALE,
  fallbackLng: DEFAULT_LOCALE,
  // Locale ids are persisted and bundled as lowercase (`en-us`, `zh-cn`).
  // i18next 26 otherwise canonicalizes regional tags during lookup and misses
  // those resource bundles even though they are registered.
  lowerCaseLng: true,
  defaultNS: 'common',
  ns: NAMESPACES,
  interpolation: { escapeValue: false },
  // Disable React.Suspense from react-i18next — all resources are bundled
  // synchronously, so suspending the tree is unnecessary and risks a
  // first-paint where pages render raw keys before init resolves.
  react: { useSuspense: false },
  // dev-only: surface missing keys to console; prod builds stay quiet.
  saveMissing: import.meta.env.DEV,
  missingKeyHandler: (lngs, ns, key) => {
    if (import.meta.env.DEV) {
      console.warn(`[i18n] missing key: ${ns}:${key} (${lngs.join(',')})`);
    }
  },
});

export default i18n;
