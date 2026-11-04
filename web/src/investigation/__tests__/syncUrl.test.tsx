import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useFiltersStore } from '@/stores/useFiltersStore';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

import { useSyncStateToUrl } from '../syncUrl';

const { navigate, renderedLocation } = vi.hoisted(() => ({
  navigate: vi.fn(),
  renderedLocation: { pathname: '/rum/explorer', search: '?type=errors', hash: '' },
}));

vi.mock('react-router-dom', () => ({
  useLocation: () => renderedLocation,
  useNavigate: () => navigate,
}));

describe('delayed investigation URL writes', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    navigate.mockClear();
    window.history.replaceState(null, '', '/rum/explorer?type=errors');
    useFiltersStore.setState({ filters: [] });
    useInvestigationStack.setState({ frames: [] });
    useTimeStore.setState({ window: { from: 'now-24h', to: 'now', mode: 'relative' }, anchor: null });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    window.history.replaceState(null, '', '/');
    useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
  });

  it('does not restore a cleared filter while the router render is still on the old address', () => {
    renderHook(useSyncStateToUrl);
    // Navigation has committed to history, but its React transition has not rendered yet.
    window.history.replaceState(null, '', '/rum/explorer');
    act(() => vi.advanceTimersByTime(80));

    expect(navigate).toHaveBeenCalledWith(
      { pathname: '/rum/explorer', search: '?time=now-24h..now', hash: '' },
      { replace: true },
    );
  });

  it('does not send a reader back to the page they left while a write was waiting', () => {
    renderHook(useSyncStateToUrl);
    window.history.replaceState(null, '', '/home');
    act(() => vi.advanceTimersByTime(80));

    expect(navigate).not.toHaveBeenCalled();
  });
});
