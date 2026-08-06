import { beforeEach, describe, expect, it } from 'vitest';

import {
  IAM_SIDEBAR_STORAGE_KEY,
  useIamSidebarStore,
} from './useIamSidebarStore';

describe('IAM sidebar state', () => {
  beforeEach(() => {
    window.localStorage.clear();
    useIamSidebarStore.setState({ collapsed: false });
  });

  it('persists the explicit collapsed preference as a plain boolean', () => {
    useIamSidebarStore.getState().toggle();

    expect(useIamSidebarStore.getState().collapsed).toBe(true);
    expect(window.localStorage.getItem(IAM_SIDEBAR_STORAGE_KEY)).toBe('true');

    useIamSidebarStore.getState().setCollapsed(false);

    expect(window.localStorage.getItem(IAM_SIDEBAR_STORAGE_KEY)).toBe('false');
  });
});
