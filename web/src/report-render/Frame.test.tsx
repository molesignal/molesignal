import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as meApi from '@/api/me';
import { DEFAULT_USER_PREFERENCES, type UserPreferences } from '@/api/me';
import { activeTimeFormat, activeTimeZone, setActiveTimePreference } from '@/lib/time';
import type { ProductAccess } from '@/product/access';
import { ThemeBootstrap } from '@/shell/ThemeBootstrap';
import { useAuthStore } from '@/stores/auth';
import { useThemeStore } from '@/stores/useThemeStore';
import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

import { ReportRenderFrame } from './Frame';

const mocks = vi.hoisted(() => ({
  preferences: vi.fn(),
  access: { current: null as unknown },
  allowed: { current: true },
}));

vi.mock('@/api/me', async (importOriginal) => ({
  ...(await importOriginal<typeof meApi>()),
  preferences: mocks.preferences,
}));

vi.mock('@/product/access', () => ({
  useProductAccess: () => mocks.access.current,
  isKnownProductPath: () => true,
  canAccessProductPath: () => mocks.allowed.current,
}));

const READY: ProductAccess = {
  organizationId: 'org-1',
  role: 'owner',
  scope: 'organization',
  permissions: new Set(),
  version: 1,
  routeCatalogVersion: 1,
  routes: [],
  status: 'ready',
};

const root = document.documentElement;

/** The page the frame is asked to print: it says what it was rendered with. */
function Content() {
  const window = useTimeStore((state) => state.window);

  return (
    <p data-testid="content">
      {`${window.from}..${window.to} in ${activeTimeZone()} as ${activeTimeFormat()}`}
    </p>
  );
}

function renderFrame(url: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });

  return render(
    <ThemeBootstrap>
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={[url]}>
          <Routes>
            <Route element={<ReportRenderFrame />}>
              <Route path="/dashboards/:id" element={<Content />} />
            </Route>
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    </ThemeBootstrap>,
  );
}

function preferences(overrides: Partial<UserPreferences> = {}): UserPreferences {
  return { ...DEFAULT_USER_PREFERENCES, ...overrides };
}

beforeEach(() => {
  useAuthStore.setState({
    token: 'token',
    ctx: { user_id: 'user-1', org_id: 'org-1', display_role: 'Owner', roles: [] },
  });
  mocks.access.current = READY;
  mocks.allowed.current = true;
  mocks.preferences.mockResolvedValue(preferences());
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  mocks.preferences.mockReset();
  setActiveTimePreference('', 'iso_24h');
  useThemeStore.setState({ language: 'en-us' });
  useTimeStore.setState({ window: DEFAULT_WINDOW });
  useAuthStore.setState({ token: null, ctx: null });
});

describe('ReportRenderFrame', () => {
  it('puts the route on a light document that is as long as its content', async () => {
    localStorage.setItem('molesignal-theme', 'dark');
    const view = renderFrame('/dashboards/d1?report_render=1');

    expect(await screen.findByTestId('content')).toBeTruthy();
    await waitFor(() => expect(root.getAttribute('data-theme')).toBe('light'));
    expect(document.body.getAttribute('data-theme')).toBe('light');
    expect(root.hasAttribute('data-report-document')).toBe(true);

    // The application gets its own document back when the report is gone.
    view.unmount();
    expect(root.hasAttribute('data-report-document')).toBe(false);
    expect(root.getAttribute('data-theme')).toBe('dark');
  });

  it('takes the time range from the URL before the content is drawn', async () => {
    renderFrame('/dashboards/d1?report_render=1&time=now-7d..now');

    expect((await screen.findByTestId('content')).textContent).toContain('now-7d..now');
  });

  it("reads in the reader's language and formats, and leaves their theme alone", async () => {
    mocks.preferences.mockResolvedValue(
      preferences({
        theme: 'dark',
        language: 'zh-cn',
        timezone: 'Europe/Berlin',
        time_format: 'local_12h',
      }),
    );
    renderFrame('/dashboards/d1?report_render=1');

    expect((await screen.findByTestId('content')).textContent).toContain(
      'in Europe/Berlin as local_12h',
    );
    expect(useThemeStore.getState().language).toBe('zh-cn');
    // A report looks the same for everyone, and opening one changes nothing the reader keeps.
    expect(localStorage.getItem('molesignal-theme')).toBeNull();
    expect(root.getAttribute('data-theme')).toBe('light');
  });

  it('reads in the timezone the report names over the reader’s own', async () => {
    mocks.preferences.mockResolvedValue(preferences({ timezone: 'Europe/Berlin' }));
    renderFrame('/dashboards/d1?report_render=1&report_tz=Asia%2FShanghai');

    expect((await screen.findByTestId('content')).textContent).toContain('in Asia/Shanghai');
  });

  it('holds the content back until the preferences are in', async () => {
    let settle: (value: UserPreferences) => void = () => undefined;
    mocks.preferences.mockReturnValue(
      new Promise<UserPreferences>((resolve) => {
        settle = resolve;
      }),
    );
    renderFrame('/dashboards/d1?report_render=1');

    await waitFor(() => expect(mocks.preferences).toHaveBeenCalled());
    expect(screen.queryByTestId('content')).toBeNull();
    expect(root.dataset.reportRenderState).toBeUndefined();

    settle(preferences({ timezone: 'UTC' }));
    expect((await screen.findByTestId('content')).textContent).toContain('in UTC');
  });

  it('still prints when the preferences cannot be loaded', async () => {
    useThemeStore.setState({ language: 'zh-cn' });
    mocks.preferences.mockRejectedValue(new Error('offline'));
    renderFrame('/dashboards/d1?report_render=1&report_tz=Asia%2FTokyo');

    expect((await screen.findByTestId('content')).textContent).toContain('in Asia/Tokyo');
    expect(useThemeStore.getState().language).toBe('zh-cn');
  });

  it('waits for the access check and draws nothing meanwhile', async () => {
    mocks.access.current = { ...READY, status: 'loading' };
    renderFrame('/dashboards/d1?report_render=1');

    await waitFor(() => expect(mocks.preferences).toHaveBeenCalled());
    await Promise.resolve();
    expect(screen.queryByTestId('content')).toBeNull();
    expect(root.dataset.reportRenderState).toBeUndefined();
  });

  it('fails the report when the reader may not open the page', async () => {
    mocks.allowed.current = false;
    renderFrame('/dashboards/d1?report_render=1');

    const reason = await screen.findByRole('alert');
    expect(reason.textContent).toBe('Your current permissions do not allow access to this page.');
    expect(screen.queryByTestId('content')).toBeNull();
    expect(root.dataset.reportRenderState).toBe('error');
    expect(root.dataset.reportRenderError).toBe(reason.textContent);
  });

  it('fails the report when access cannot be established', async () => {
    mocks.access.current = { ...READY, status: 'error' };
    renderFrame('/dashboards/d1?report_render=1');

    expect((await screen.findByRole('alert')).textContent).toBe('Unable to load access permissions.');
    expect(root.dataset.reportRenderState).toBe('error');
  });
});
