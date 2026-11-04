import { afterEach, describe, expect, it, vi } from 'vitest';

import { http } from '@/lib/http';

import { getSigninTest, listAssignableRoles, startSigninTest, testDirectorySignin } from './sso';

describe('SSO test sign-in API', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('starts a test of a provider, whose id goes into the address safely', async () => {
    const request = vi.spyOn(http, 'post').mockResolvedValue({
      data: { id: 't-1', authorizationUrl: 'https://idp.example/authorize' },
    } as never);

    await expect(startSigninTest('a/b')).resolves.toEqual({
      id: 't-1',
      authorizationUrl: 'https://idp.example/authorize',
    });
    expect(request).toHaveBeenCalledWith('/sso/providers/a%2Fb/tests');
  });

  it('sends a directory account in the body, never in the address', async () => {
    const request = vi.spyOn(http, 'post').mockResolvedValue({ data: { id: 't-1' } } as never);

    await testDirectorySignin('dir', { username: 'alice', password: 'secret' });

    expect(request).toHaveBeenCalledWith('/sso/providers/dir/tests/ldap', {
      username: 'alice',
      password: 'secret',
    });
  });

  it('reads one test', async () => {
    const request = vi.spyOn(http, 'get').mockResolvedValue({ data: { id: 't-1' } } as never);

    await getSigninTest('t-1');

    expect(request).toHaveBeenCalledWith('/sso/tests/t-1');
  });
});

describe('SSO assignable roles API', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('reads roles from the SSO-scoped endpoint', async () => {
    const request = vi.spyOn(http, 'get').mockResolvedValue({
      data: [
        { id: 'role-owner', name: 'Owner' },
        { id: 'role-viewer', name: 'Viewer' },
      ],
    } as never);

    await expect(listAssignableRoles()).resolves.toEqual([
      { id: 'role-owner', name: 'Owner' },
      { id: 'role-viewer', name: 'Viewer' },
    ]);
    expect(request).toHaveBeenCalledOnce();
    expect(request).toHaveBeenCalledWith('/sso/providers/roles');
  });
});
