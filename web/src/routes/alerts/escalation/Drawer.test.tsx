import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as escalationsApi from '@/api/escalations';
import * as importanceApi from '@/api/importance';
import * as schedulesApi from '@/api/schedules';
import * as teamsApi from '@/api/teams';
import * as usersApi from '@/api/users';
import i18n from '@/i18n';
import type { EscalationPolicy } from '@/types/alerting';

import { EscalationDrawer } from './Drawer';

vi.mock('@/api/escalations');
vi.mock('@/api/importance');
vi.mock('@/api/schedules');
vi.mock('@/api/teams');
vi.mock('@/api/users');
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false, reason: undefined }),
}));

const policy = {
  id: 'policy-1',
  name: 'Weekday on-call',
  repeat: false,
  maxLoops: 1,
  urgencyRule: { mode: 'by_severity' },
  escalateLowUrgency: false,
  steps: [{ targets: [{ kind: 'user', userId: 'root-id' }], ackTimeoutSecs: 300, minSeverity: null }],
} as unknown as EscalationPolicy;

const root = {
  id: 'root-id',
  email: 'admin@example.com',
  displayName: 'root',
  isRoot: true,
  disabled: false,
  roles: [],
  teamNames: [],
  loginMethod: 'password',
  createdAt: 0,
} as usersApi.UserView;

/** The drawer as the page has it: mounted closed, then opened on a policy. */
function openDrawer(editing: EscalationPolicy = policy) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const tree = (open: boolean) => (
    <QueryClientProvider client={client}>
      <EscalationDrawer open={open} editing={open ? editing : null} onClose={vi.fn()} />
    </QueryClientProvider>
  );
  const view = render(tree(false));
  view.rerender(tree(true));
}

const comboboxTexts = () => screen.getAllByRole('combobox').map((box) => box.textContent ?? '');

const save = () => fireEvent.submit(document.getElementById('escalation-form') as HTMLFormElement);

describe('EscalationDrawer', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US');
    vi.mocked(schedulesApi.list).mockResolvedValue([]);
    vi.mocked(teamsApi.list).mockResolvedValue([]);
    vi.mocked(importanceApi.get).mockReturnValue(new Promise(() => {}));
    vi.mocked(escalationsApi.update).mockResolvedValue(policy);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('keeps the person a step points at when the list of people arrives after the drawer opened', async () => {
    let answer = (_people: usersApi.UserView[]) => {};
    vi.mocked(usersApi.list).mockReturnValue(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );

    openDrawer();
    await screen.findByRole('dialog');

    act(() => answer([root]));

    await waitFor(() => expect(comboboxTexts()).toContain('root'));
  });

  it('does not lose the target when saved before the list of people arrived', async () => {
    vi.mocked(usersApi.list).mockReturnValue(new Promise(() => {}));

    openDrawer();
    await screen.findByRole('dialog');
    save();

    await waitFor(() => expect(escalationsApi.update).toHaveBeenCalled());
    expect(vi.mocked(escalationsApi.update).mock.calls[0]?.[1].steps).toEqual([
      expect.objectContaining({ targets: [{ kind: 'user', userId: 'root-id' }] }),
    ]);
  });

  it('names a person the list does not hold instead of showing an empty choice', async () => {
    vi.mocked(usersApi.list).mockResolvedValue([]);

    openDrawer();

    await waitFor(() => expect(comboboxTexts()).toContain(i18n.t('common:people.non_member')));
  });

  it('offers people the list names without an email', async () => {
    vi.mocked(usersApi.list).mockResolvedValue([
      { id: 'root-id', displayName: 'root', avatarUrl: null },
    ] as usersApi.UserView[]);

    openDrawer();

    await waitFor(() => expect(comboboxTexts()).toContain('root'));
  });

  it('keeps a target and saves it when the list of people cannot be read', async () => {
    vi.mocked(usersApi.list).mockRejectedValue(new Error('internal error'));

    openDrawer();

    await waitFor(() => expect(comboboxTexts()).toContain(i18n.t('common:people.someone')));
    save();

    await waitFor(() => expect(escalationsApi.update).toHaveBeenCalled());
    expect(vi.mocked(escalationsApi.update).mock.calls[0]?.[1].steps).toEqual([
      expect.objectContaining({ targets: [{ kind: 'user', userId: 'root-id' }] }),
    ]);
  });
});
