import i18n, { type BackendModule, type ResourceLanguage } from 'i18next';
import { initReactI18next } from 'react-i18next';

import { DEFAULT_LOCALE, type Locale, matchLocale, toLocale } from './locales';

export {
  DEFAULT_LOCALE,
  docsLocale,
  intlLocale,
  isChinese,
  isLocale,
  isMainlandChinese,
  LOCALE_LABELS,
  type Locale,
  matchLocale,
  SUPPORTED_LOCALES,
  toLocale,
} from './locales';

/**
 * Locales used to store in persisted state before the BCP-47 rename.
 * Kept exported so `useThemeStore`'s migration step can map legacy
 * values forward without re-implementing the same lookup.
 */
export const LEGACY_LOCALE_MAP: Record<string, Locale> = {
  en: 'en-us',
  'zh-CN': 'zh-cn',
};

type Bundle = Record<string, ResourceLanguage[string]>;

/** Namespace name from a glob key such as `./en-us/common.json`. */
function byNamespace(files: Record<string, unknown>): Bundle {
  const bundle: Bundle = {};

  for (const [path, content] of Object.entries(files)) {
    const namespace = /([^/]+)\.json$/.exec(path)?.[1];
    if (namespace) bundle[namespace] = content as Bundle[string];
  }

  return bundle;
}

// English is the fallback for every missing key, so it ships in the main
// bundle; every other language is a chunk of its own, loaded when chosen.
const ENGLISH = byNamespace(
  import.meta.glob<Record<string, unknown>>('./en-us/*.json', { eager: true, import: 'default' }),
);

const NAMESPACES = Object.keys(ENGLISH).sort();

const LOADERS: Record<Exclude<Locale, 'en-us'>, () => Promise<{ default: Record<string, unknown> }>> = {
  'zh-cn': () => import('./bundles/zh-cn'),
  'zh-tw': () => import('./bundles/zh-tw'),
  'ja-jp': () => import('./bundles/ja-jp'),
  'ko-kr': () => import('./bundles/ko-kr'),
  de: () => import('./bundles/de'),
  es: () => import('./bundles/es'),
  fr: () => import('./bundles/fr'),
  'pt-br': () => import('./bundles/pt-br'),
};

const loading = new Map<Locale, Promise<Bundle>>();

function loadBundle(locale: Locale): Promise<Bundle> {
  if (locale === 'en-us') return Promise.resolve(ENGLISH);

  let pending = loading.get(locale);
  if (!pending) {
    pending = LOADERS[locale]().then((module) => byNamespace(module.default));
    pending.catch(() => loading.delete(locale));
    loading.set(locale, pending);
  }
  return pending;
}

const chunkBackend: BackendModule = {
  type: 'backend',
  init() {},
  read(language, namespace, callback) {
    const locale = matchLocale(language);

    // A language the product does not translate has no entries: its keys
    // come from the English fallback.
    if (!locale) {
      callback(null, {});
      return;
    }

    loadBundle(locale).then(
      (bundle) => callback(null, bundle[namespace] ?? {}),
      (error: unknown) => callback(error as Error, null),
    );
  },
};

/**
 * Best-effort detect a supported locale from the browser's preferred
 * languages. This is only consulted on the first visit (before the user picks
 * a language) and for pages read signed out.
 */
export function detectBrowserLocale(): Locale {
  if (typeof navigator === 'undefined') return DEFAULT_LOCALE;

  const preferred = navigator.languages?.length ? navigator.languages : [navigator.language];
  for (const tag of preferred) {
    const locale = matchLocale(tag);
    if (locale) return locale;
  }
  return DEFAULT_LOCALE;
}

/** Loads a language's translations; resolves once its keys can be read. */
export function loadLocale(language: string | null | undefined): Promise<void> {
  const locale = toLocale(language);

  if (NAMESPACES.every((namespace) => i18n.hasResourceBundle(locale, namespace))) {
    return Promise.resolve();
  }
  return i18n.loadLanguages(locale).then(() => undefined);
}

/**
 * A `t` fixed to a language other than the interface's, such as the one a
 * public status page is written in. It reads English until that language's
 * translations arrive; components that use `useTranslation` render again then.
 */
export function fixedT(language: string, namespace: string) {
  void loadLocale(language);
  return i18n.getFixedT(toLocale(language), namespace);
}

void i18n
  .use(chunkBackend)
  .use(initReactI18next)
  .init({
    resources: { 'en-us': ENGLISH },
    // English is bundled; other languages come from `chunkBackend`.
    partialBundledLanguages: true,
    // English is bundled, so start-up needs no load and `t` works on import.
    initAsync: false,
    lng: DEFAULT_LOCALE,
    fallbackLng: DEFAULT_LOCALE,
    // Load `zh-tw` itself, not also a bare `zh` that has no translations.
    load: 'currentOnly',
    // Locale ids are persisted and bundled as lowercase (`en-us`, `zh-cn`).
    // i18next 26 otherwise canonicalizes regional tags during lookup and misses
    // those resource bundles even though they are registered.
    lowerCaseLng: true,
    defaultNS: 'common',
    ns: NAMESPACES,
    interpolation: { escapeValue: false },
    react: {
      // Untranslated keys read English while a language loads, rather than
      // suspending the tree.
      useSuspense: false,
      // Render again when a language's chunk arrives, for `fixedT` readers.
      bindI18n: 'languageChanged loaded',
    },
    // dev-only: surface missing keys to console; prod builds stay quiet.
    saveMissing: import.meta.env.DEV,
    missingKeyHandler: (lngs, ns, key) => {
      if (import.meta.env.DEV) {
        console.warn(`[i18n] missing key: ${ns}:${key} (${lngs.join(',')})`);
      }
    },
  });

export { NAMESPACES };
export default i18n;
