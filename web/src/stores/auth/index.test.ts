import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { normalizeRole, useAuthStore } from './index';
import { tabSessionSharing } from './tabSession';

describe('auth store', () => {
  beforeEach(() => {
    useAuthStore.getState().logout();
  });

  it('preserves database-defined display roles', () => {
    expect(normalizeRole('  SRE Operator  ')).toBe('SRE Operator');
    expect(normalizeRole('custom_auditor')).toBe('custom_auditor');
  });

  it('stores database role metadata when saving a session', () => {
    useAuthStore.getState().setSession('token', {
      user_id: 'u1',
      org_id: 'org1',
      display_role: 'SRE Operator',
      roles: [
        { id: 'role-1', key: 'sre_operator', name: 'SRE Operator', builtin: false },
      ],
    });

    expect(useAuthStore.getState().ctx?.display_role).toBe('SRE Operator');
    expect(useAuthStore.getState().ctx?.roles[0]?.id).toBe('role-1');
  });

  describe('logout', () => {
    const signedIn = () =>
      useAuthStore.getState().setSession('token', {
        user_id: 'u1',
        org_id: 'org1',
        display_role: 'Owner',
        roles: [],
      });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    it('ends only this tab\'s session by default', () => {
      const announce = vi.spyOn(tabSessionSharing, 'announceSignOut');
      signedIn();

      useAuthStore.getState().logout();

      expect(useAuthStore.getState().token).toBeNull();
      expect(useAuthStore.getState().ctx).toBeNull();
      expect(announce).not.toHaveBeenCalled();
    });

    it('tells the other tabs when the user signs out on purpose', () => {
      const announce = vi.spyOn(tabSessionSharing, 'announceSignOut').mockImplementation(() => {});
      signedIn();

      useAuthStore.getState().logout({ allTabs: true });

      expect(useAuthStore.getState().token).toBeNull();
      expect(announce).toHaveBeenCalledTimes(1);
    });
  });

  it('hydrates system scope from JWT claims', () => {
    const encode = (value: object) =>
      btoa(JSON.stringify(value))
        .replace(/\+/g, '-')
        .replace(/\//g, '_')
        .replace(/=+$/, '');
    const token = `${encode({ alg: 'none' })}.${encode({
      org_id: 'system-org',
      scope: 'system',
    })}.signature`;

    useAuthStore.getState().setSession(token, {
      user_id: 'u1',
      org_id: 'system-org',
      display_role: 'Owner',
      roles: [
        {
          id: 'role-platform-owner',
          key: 'platform_owner',
          name: 'Owner',
          builtin: true,
        },
      ],
      scope: 'organization',
    });

    expect(useAuthStore.getState().ctx).toEqual(
      expect.objectContaining({
        display_role: 'Owner',
        scope: 'system',
      }),
    );
  });
});
