import { afterEach, describe, expect, it, vi } from 'vitest';

import { http } from '@/lib/http';

import { listAssignableRoles } from './sso';

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
