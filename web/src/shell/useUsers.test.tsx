import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, renderHook, waitFor } from '@testing-library/react';
import * as React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as usersApi from '@/api/users';
import i18n from '@/i18n';
import { useAuthStore } from '@/stores/auth';

import { useUsers } from './useUsers';

vi.mock('@/api/users');

const member = {
  id: 'member',
  email: 'dana@example.com',
  displayName: 'Dana Reyes',
  isRoot: false,
  disabled: false,
  roles: [],
  teamNames: [],
  loginMethod: 'password',
  createdAt: 0,
} as usersApi.UserView;

function useUsersWithClient() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return renderHook(() => useUsers(), { wrapper }).result;
}

describe('useUsers', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US');
    useAuthStore.setState({
      ctx: {
        userId: 'root',
        orgId: 'org',
        displayRole: 'Owner',
        roles: [],
        email: 'admin@example.com',
        displayName: 'root',
      },
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    useAuthStore.setState({ ctx: null });
  });

  it('says nothing about a person until the list has answered, and never their id', () => {
    vi.mocked(usersApi.list).mockReturnValue(new Promise(() => {}));

    const result = useUsersWithClient();

    expect(result.current.nameOf('member')).toBe('…');
    expect(result.current.nameOf('')).toBe('');
  });

  it('names a member by the list', async () => {
    vi.mocked(usersApi.list).mockResolvedValue([member]);

    const result = useUsersWithClient();

    await waitFor(() => expect(result.current.nameOf('member')).toBe('Dana Reyes'));
  });

  it('takes names without an email, as someone without `org.members.read` gets them', async () => {
    vi.mocked(usersApi.list).mockResolvedValue([
      { id: 'member', displayName: 'Dana Reyes', avatarUrl: null },
    ] as usersApi.UserView[]);

    const result = useUsersWithClient();

    await waitFor(() => expect(result.current.users).toHaveLength(1));
    expect(result.current.nameOf('member')).toBe('Dana Reyes');
    expect(result.current.users[0]?.email).toBe('');
  });

  it('calls someone the list leaves out a non-member, not by their id', async () => {
    vi.mocked(usersApi.list).mockResolvedValue([member]);

    const result = useUsersWithClient();

    await waitFor(() => expect(result.current.users).toHaveLength(1));
    expect(result.current.nameOf('gone')).toBe(i18n.t('common:people.non_member'));
  });

  it('does not call anyone a non-member when the list cannot be read', async () => {
    vi.mocked(usersApi.list).mockRejectedValue(new Error('internal error'));

    const result = useUsersWithClient();

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.nameOf('member')).toBe(i18n.t('common:people.someone'));
  });

  it('names whoever is signed in even outside the list, without offering them in a picker', async () => {
    vi.mocked(usersApi.list).mockResolvedValue([member]);

    const result = useUsersWithClient();

    expect(result.current.nameOf('root')).toBe('root');
    await waitFor(() => expect(result.current.users).toHaveLength(1));
    expect(result.current.nameOf('root')).toBe('root');
    expect(result.current.users.map((user) => user.id)).toEqual(['member']);
  });

  it('takes the name of a member from the list, not from the session', async () => {
    vi.mocked(usersApi.list).mockResolvedValue([{ ...member, id: 'root', displayName: 'Renamed' }]);

    const result = useUsersWithClient();

    await waitFor(() => expect(result.current.nameOf('root')).toBe('Renamed'));
  });
});
