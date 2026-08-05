import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { useThemeStore } from '@/stores/useThemeStore';

type ThemePreference = 'system' | 'dark' | 'light';
type ResolvedTheme = 'dark' | 'light';
// Phase 4: three density modes (brief hard constraint). `normal` is default.
type Density = 'compact' | 'normal' | 'comfortable';
const DENSITY_VALUES: readonly Density[] = ['compact', 'normal', 'comfortable'];

const THEME_KEY = 'molesignal-theme';
const DENSITY_KEY = 'molesignal-density';
const EXPLICIT_THEME_KEY = 'molesignal-theme-explicit';

/**
 * localStorage keys we used to write in earlier shipped builds but no longer
 * read. Cleared on app boot so they don't accumulate forever. Add an entry
 * here when removing a persisted preference; never repurpose an old key.
 */
const STALE_KEYS = ['molesignal-workspace'] as const;

function systemTheme(): ResolvedTheme {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return 'light';
  }
  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light';
}

function getInitialThemePreference(): ThemePreference {
  if (typeof window === 'undefined') return 'system';
  const stored = localStorage.getItem(THEME_KEY);
  if (stored === 'system' || stored === 'dark' || stored === 'light') return stored;
  return 'system';
}

function getInitialDensity(): Density {
  if (typeof window === 'undefined') return 'normal';
  const stored = localStorage.getItem(DENSITY_KEY);
  if (stored && (DENSITY_VALUES as readonly string[]).includes(stored)) return stored as Density;
  // Forward-migrate older builds that only knew compact/comfortable.
  if (stored === 'compact' || stored === 'comfortable') return stored;
  return 'normal';
}

interface ThemeApi {
  theme: ResolvedTheme;
  themePreference: ThemePreference;
  density: Density;
  setTheme: (t: ThemePreference) => void;
  setDensity: (d: Density) => void;
  toggleTheme: () => void;
  toggleDensity: () => void;
}

const ThemeContext = React.createContext<ThemeApi | null>(null);

export function ThemeBootstrap({ children }: { children: React.ReactNode }) {
  const [themePreference, setThemePreference] =
    React.useState<ThemePreference>(getInitialThemePreference);
  const [resolvedSystemTheme, setResolvedSystemTheme] =
    React.useState<ResolvedTheme>(systemTheme);
  const [density, setDensityState] = React.useState<Density>(getInitialDensity);
  const palette = useThemeStore((s) => s.palette);
  const language = useThemeStore((s) => s.language);
  const { i18n } = useTranslation();
  const theme =
    themePreference === 'system' ? resolvedSystemTheme : themePreference;

  React.useEffect(() => {
    if (typeof window === 'undefined') return;
    for (const k of STALE_KEYS) {
      localStorage.removeItem(k);
    }
  }, []);

  React.useLayoutEffect(() => {
    document.body.setAttribute('data-theme', theme);
    document.documentElement.setAttribute('data-theme', theme);
  }, [theme]);

  React.useLayoutEffect(() => {
    document.body.setAttribute('data-density', density);
    document.documentElement.setAttribute('data-density', density);
  }, [density]);

  React.useLayoutEffect(() => {
    document.documentElement.setAttribute('data-palette', palette);
  }, [palette]);

  // Sync language: drive i18next + `<html lang>` whenever the store changes.
  // Putting `<html lang>` here (not on every consumer) keeps it as the
  // single source of truth for screen readers.
  React.useEffect(() => {
    if (i18n.language !== language) {
      void i18n.changeLanguage(language);
    }
    document.documentElement.setAttribute('lang', language);
  }, [language, i18n]);

  // Resolve the system preference continuously so OS changes apply live.
  React.useEffect(() => {
    if (typeof window === 'undefined') return;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => {
      setResolvedSystemTheme(e.matches ? 'dark' : 'light');
    };
    setResolvedSystemTheme(mq.matches ? 'dark' : 'light');
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, []);

  const setTheme = React.useCallback((t: ThemePreference) => {
    localStorage.setItem(THEME_KEY, t);
    if (t === 'system') localStorage.removeItem(EXPLICIT_THEME_KEY);
    else localStorage.setItem(EXPLICIT_THEME_KEY, '1');
    setThemePreference(t);
  }, []);

  const setDensity = React.useCallback((d: Density) => {
    localStorage.setItem(DENSITY_KEY, d);
    setDensityState(d);
  }, []);

  const api: ThemeApi = {
    theme,
    themePreference,
    density,
    setTheme,
    setDensity,
    toggleTheme: () => setTheme(theme === 'dark' ? 'light' : 'dark'),
    // Cycle compact → normal → comfortable → compact
    toggleDensity: () => {
      const idx = DENSITY_VALUES.indexOf(density);
      const next = DENSITY_VALUES[(idx + 1) % DENSITY_VALUES.length] ?? 'normal';
      setDensity(next);
    },
  };

  return <ThemeContext.Provider value={api}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeApi {
  const ctx = React.useContext(ThemeContext);
  if (!ctx) throw new Error('useTheme must be used within ThemeBootstrap');
  return ctx;
}
