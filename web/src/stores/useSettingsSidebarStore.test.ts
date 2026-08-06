import { beforeEach, describe, expect, it } from 'vitest';

import {
  SETTINGS_SIDEBAR_STORAGE_KEY,
  useSettingsSidebarStore,
} from './useSettingsSidebarStore';

describe('settings sidebar state', () => {
  beforeEach(() => {
    window.localStorage.clear();
    useSettingsSidebarStore.setState({ collapsed: false });
  });

  it('persists the explicit collapsed preference as a plain boolean', () => {
    useSettingsSidebarStore.getState().toggle();

    expect(useSettingsSidebarStore.getState().collapsed).toBe(true);
    expect(window.localStorage.getItem(SETTINGS_SIDEBAR_STORAGE_KEY)).toBe(
      'true',
    );

    useSettingsSidebarStore.getState().setCollapsed(false);

    expect(window.localStorage.getItem(SETTINGS_SIDEBAR_STORAGE_KEY)).toBe(
      'false',
    );
  });
});
