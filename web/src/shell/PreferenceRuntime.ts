import * as React from 'react';

import type { UserPreferences } from '@/api/me';
import { setActiveTimePreference } from '@/lib/time';
import { useTheme } from '@/shell/ThemeBootstrap';
import { useThemeStore } from '@/stores/useThemeStore';

export const USER_PREFERENCES_QUERY_KEY = ['me', 'preferences'] as const;

/**
 * Applies persisted personal defaults to the live shell. Persistence remains
 * the caller's responsibility; every entry point uses this one runtime path.
 */
export function useApplyUserPreferences() {
  const { setTheme, setDensity } = useTheme();
  const setLanguage = useThemeStore((state) => state.setLanguage);
  const setKeyboardShortcutsEnabled = useThemeStore(
    (state) => state.setKeyboardShortcutsEnabled,
  );

  return React.useCallback(
    (preferences: UserPreferences) => {
      setTheme(preferences.theme);
      setDensity(preferences.density);
      setLanguage(preferences.language);
      setKeyboardShortcutsEnabled(preferences.keyboard_shortcuts_enabled);
      setActiveTimePreference(
        preferences.timezone,
        preferences.time_format,
        preferences.date_format,
      );
    },
    [setDensity, setKeyboardShortcutsEnabled, setLanguage, setTheme],
  );
}
