import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as meApi from '@/api/me';
import i18n from '@/i18n';
import { useTimeFormatter } from '@/lib/time';
import { USER_PREFERENCES_QUERY_KEY } from '@/shell/PreferenceRuntime';
import { createTimeStore, TimeScopeContext, type TimeStoreApi } from '@/stores/useTimeStore';

import { TimezoneSection } from '../TimezoneSection';

vi.mock('@/api/me', async (original) => {
  const actual = await original<typeof meApi>();
  return { ...actual, updatePreferences: vi.fn(async (preferences: meApi.UserPreferences) => preferences) };
});

const MOMENT_MS = Date.UTC(2026, 9, 18, 9, 30);

function Clock() {
  const time = useTimeFormatter();
  return <output data-testid="clock">{time.millis(MOMENT_MS)}</output>;
}

function renderSection(store: TimeStoreApi) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(USER_PREFERENCES_QUERY_KEY, { ...meApi.DEFAULT_USER_PREFERENCES, timezone: 'UTC' });

  render(
    <QueryClientProvider client={client}>
      <TimeScopeContext.Provider value={store}>
        <TimezoneSection />
        <Clock />
      </TimeScopeContext.Provider>
    </QueryClientProvider>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage('en-US');
  // The list of zones keeps its choice in view; jsdom lays nothing out to scroll.
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(cleanup);

describe('the timezone at the foot of the time picker', () => {
  it('starts on the personal default, and a zone picked holds for the time in scope', async () => {
    const store = createTimeStore();
    renderSection(store);

    expect(screen.getByText(/Use personal default · UTC/)).toBeTruthy();
    expect(screen.getByTestId('clock').textContent).toContain('09:30');

    fireEvent.click(screen.getByRole('button', { name: 'Change' }));
    fireEvent.change(screen.getByPlaceholderText('Search timezones…'), { target: { value: 'Asia/Tokyo' } });
    fireEvent.click(await screen.findByRole('option', { name: /Asia\/Tokyo/ }));

    expect(store.getState().timezone).toBe('Asia/Tokyo');
    await waitFor(() => expect(screen.getByTestId('clock').textContent).toContain('18:30'));
    expect(screen.getByText(/Asia\/Tokyo · UTC\+9 · this page only/)).toBeTruthy();
  });

  it('makes the picked zone the personal default, and goes back to following it', async () => {
    const store = createTimeStore({ timezone: 'Asia/Tokyo' });
    renderSection(store);

    fireEvent.click(screen.getByRole('button', { name: 'Set as my default timezone' }));

    await waitFor(() => expect(vi.mocked(meApi.updatePreferences)).toHaveBeenCalledWith(expect.objectContaining({ timezone: 'Asia/Tokyo' })));
    await waitFor(() => expect(store.getState().timezone).toBe(''));
  });
});
