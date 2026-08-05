import { create } from 'zustand';
import { persist } from 'zustand/middleware';

import { detectBrowserLocale, LEGACY_LOCALE_MAP, type Locale, SUPPORTED_LOCALES } from '@/i18n';

// Phase 4 decision: single palette. The `warm` and `high-contrast` palettes
// were removed; the default palette now meets WCAG AA+ on its own. We keep
// the enum + setter API for forward compatibility (a future pro
// brand palette could plug in here), but the UI no longer surfaces a
// chooser.
export const PALETTES = ['default'] as const;
export type Palette = (typeof PALETTES)[number];

export const DEFAULT_PALETTE: Palette = 'default';

interface ThemeState {
  palette: Palette;
  language: Locale;
  keyboardShortcutsEnabled: boolean;
  setPalette: (p: Palette) => void;
  /** Advance to the next palette in `PALETTES` order, wrapping. With a
   *  single palette today, this is a no-op; preserved for forward compat. */
  cyclePalette: () => Palette;
  setLanguage: (l: Locale) => void;
  setKeyboardShortcutsEnabled: (enabled: boolean) => void;
  /** Advance to the next locale in `SUPPORTED_LOCALES` order, wrapping.
   *  Drives the shell language controls; gear dropdown
   *  remains available for direct selection. */
  cycleLanguage: () => Locale;
}

function defaultLanguage(): Locale {
  return detectBrowserLocale();
}

export const useThemeStore = create<ThemeState>()(
  persist(
    (set, get) => ({
      palette: DEFAULT_PALETTE,
      language: defaultLanguage(),
      keyboardShortcutsEnabled: true,
      setPalette: (p) => set({ palette: p }),
      cyclePalette: () => {
        const current = get().palette;
        const idx = PALETTES.indexOf(current);
        const next = PALETTES[(idx + 1) % PALETTES.length]!;
        set({ palette: next });
        return next;
      },
      setLanguage: (l) => set({ language: l }),
      setKeyboardShortcutsEnabled: (enabled) => set({ keyboardShortcutsEnabled: enabled }),
      cycleLanguage: () => {
        const current = get().language;
        const idx = SUPPORTED_LOCALES.indexOf(current);
        const next = SUPPORTED_LOCALES[(idx + 1) % SUPPORTED_LOCALES.length]!;
        set({ language: next });
        return next;
      },
    }),
    {
      name: 'molesignal-ui-prefs',
      partialize: (state) => ({
        palette: state.palette,
        language: state.language,
        keyboardShortcutsEnabled: state.keyboardShortcutsEnabled,
      }),
      // Reject persisted values that are no longer in the allowed enums
      // (e.g. an old "warm-orange" palette removed in a later release).
      // Also forward-map legacy BCP-47-incomplete locale codes
      // (`en` → `en-us`, `zh-CN` → `zh-cn`) so users who saved a value
      // before the rename land on the new equivalent rather than the
      // detected browser default.
      merge: (persisted, current) => {
        const p = persisted as Partial<ThemeState> | undefined;
        const palette = p?.palette && PALETTES.includes(p.palette) ? p.palette : current.palette;
        const persistedLanguage = p?.language as string | undefined;
        const mapped =
          persistedLanguage && persistedLanguage in LEGACY_LOCALE_MAP
            ? LEGACY_LOCALE_MAP[persistedLanguage]!
            : (persistedLanguage as Locale | undefined);
        const language =
          mapped && SUPPORTED_LOCALES.includes(mapped) ? mapped : current.language;
        return {
          ...current,
          palette,
          language,
          keyboardShortcutsEnabled:
            typeof p?.keyboardShortcutsEnabled === 'boolean'
              ? p.keyboardShortcutsEnabled
              : current.keyboardShortcutsEnabled,
        };
      },
    },
  ),
);
