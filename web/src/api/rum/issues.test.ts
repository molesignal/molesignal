import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ get: vi.fn(), patch: vi.fn() }));

vi.mock('@/lib/http', async (importOriginal) => ({
  ...((await importOriginal()) as Record<string, unknown>),
  http: { get: mocks.get, patch: mocks.patch },
}));

import { getIssue, listIssues, SETTABLE_STATUSES, updateIssue } from './issues';

describe('the issues client', () => {
  beforeEach(() => {
    mocks.get.mockReset();
    mocks.patch.mockReset();
  });

  it('says the filters of a list as the JSON the server reads, and leaves out those it has none of', async () => {
    mocks.get.mockResolvedValue({ data: { issues: [], total: 0 } });

    await listIssues({ application: 'shop-web', status: 'unresolved', limit: 50, filters: [{ field: 'version', op: '=', value: '2.4.1' }] });
    await listIssues({ application: 'shop-web', filters: [] });

    expect(mocks.get).toHaveBeenNthCalledWith(1, '/rum/issues', {
      params: {
        application: 'shop-web',
        status: 'unresolved',
        limit: 50,
        filters: '[{"field":"version","op":"=","value":"2.4.1"}]',
      },
    });
    expect(mocks.get).toHaveBeenNthCalledWith(2, '/rum/issues', { params: { application: 'shop-web' } });
  });

  it('passes the signal of a read on, and takes the issue by its id as one word of the address', async () => {
    mocks.get.mockResolvedValue({ data: { id: 'a/b' } });
    const signal = new AbortController().signal;

    await getIssue('a/b', { from: 1, to: 2 }, signal);

    expect(mocks.get).toHaveBeenCalledWith('/rum/issues/a%2Fb', { params: { from: 1, to: 2 }, signal });
  });

  it('changes an issue with a patch that names only what changes', async () => {
    mocks.patch.mockResolvedValue({ data: { id: 'abc', status: 'resolved' } });

    const issue = await updateIssue('abc', { status: 'resolved', assigneeId: null });

    expect(mocks.patch).toHaveBeenCalledWith('/rum/issues/abc', { status: 'resolved', assigneeId: null });
    expect(issue.status).toBe('resolved');
  });

  it('lets a person set every state but the one the rules set', () => {
    expect(SETTABLE_STATUSES).toEqual(['open', 'investigating', 'resolved', 'ignored']);
  });
});
