import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { normalizeRole, useAuthStore } from './index';
import { tabSessionSharing } from './tabSession';

describe('auth store', () => {
  beforeEach(() => {
    useAuthStore.getState().logout();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('preserves database-defined display roles', () => {
    expect(normalizeRole('  SRE Operator  ')).toBe('SRE Operator');
    expect(normalizeRole('custom_auditor')).toBe('custom_auditor');
  });

  it('stores database role metadata when saving a session', () => {
    useAuthStore.getState().setSession({
      userId: 'u1',
      orgId: 'org1',
      displayRole: 'SRE Operator',
      roles: [
        { id: 'role-1', key: 'sre_operator', name: 'SRE Operator', builtin: false },
      ],
    });

    expect(useAuthStore.getState().ctx?.displayRole).toBe('SRE Operator');
    expect(useAuthStore.getState().ctx?.roles[0]?.id).toBe('role-1');
  });

  it('keeps the scope the server answered with', () => {
    useAuthStore.getState().setSession({
      userId: 'u1',
      orgId: 'system-org',
      displayRole: 'Owner',
      roles: [{ id: 'role-platform-owner', key: 'platform_owner', name: 'Owner', builtin: true }],
      scope: 'system',
    });

    expect(useAuthStore.getState().ctx).toEqual(
      expect.objectContaining({ displayRole: 'Owner', scope: 'system' }),
    );
  });

  it('tells the other tabs which session this one now shows', () => {
    const announce = vi.spyOn(tabSessionSharing, 'announceSession').mockImplementation(() => {});

    useAuthStore.getState().setSession({
      userId: 'u1',
      orgId: 'org2',
      displayRole: 'Owner',
      roles: [],
    });

    expect(announce).toHaveBeenCalledWith('u1:org2');
  });

  it('keeps nothing but the displayed identity', () => {
    useAuthStore.getState().setSession({
      userId: 'u1',
      orgId: 'org1',
      displayRole: 'Owner',
      roles: [],
    });

    const stored = sessionStorage.getItem('molesignal-auth') ?? localStorage.getItem('molesignal-auth');
    expect(Object.keys(JSON.parse(stored ?? '{}').state)).toEqual(['ctx']);
  });

  describe('logout', () => {
    const signedIn = () =>
      useAuthStore.getState().setSession({
        userId: 'u1',
        orgId: 'org1',
        displayRole: 'Owner',
        roles: [],
      });

    it('ends only this tab\'s session by default', () => {
      const announce = vi.spyOn(tabSessionSharing, 'announceSignOut');
      signedIn();

      useAuthStore.getState().logout();

      expect(useAuthStore.getState().ctx).toBeNull();
      expect(announce).not.toHaveBeenCalled();
    });

    it('tells the other tabs when the user signs out on purpose', () => {
      const announce = vi.spyOn(tabSessionSharing, 'announceSignOut').mockImplementation(() => {});
      signedIn();

      useAuthStore.getState().logout({ allTabs: true });

      expect(useAuthStore.getState().ctx).toBeNull();
      expect(announce).toHaveBeenCalledTimes(1);
    });
  });
});
