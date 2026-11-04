import axios, { AxiosError } from 'axios';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ search: vi.fn() }));

vi.mock('@/api/rum', () => ({ search: mocks.search }));

import { recentReceipt, sendTestEvents, testEvents } from './rumIntake';

const REQUEST = { token: 'mspat-secret', applicationId: 'checkout-web', platform: 'browser', service: 'rum' } as const;

const post = vi.spyOn(axios, 'post');

beforeEach(() => {
  post.mockReset();
  mocks.search.mockReset();
});

describe('the test events of the datasource guide', () => {
  it('are one visit of a synthetic session: a view and the error it ran into', () => {
    const [view, error] = testEvents(REQUEST, 1_760_000_000_000) as Array<Record<string, any>>;

    expect([view?.type, error?.type]).toEqual(['view', 'error']);
    for (const event of [view, error]) {
      expect(event).toMatchObject({
        date: 1_760_000_000_000,
        source: 'browser',
        application: { id: 'checkout-web' },
        session: { type: 'synthetics', start: 1_760_000_000_000 },
      });
    }
    expect(view?.session.id).toBe(error?.session.id);
    expect(view?.view.id).toBe(error?.view.id);

    // What the contract requires of each: a view is told by its url, its revision and the time spent.
    expect(view?.view).toMatchObject({ revision: 1, url: '/datasource-test' });
    expect(Number.isInteger(view?.view.time_spent)).toBe(true);
    expect(error?.error).toMatchObject({ source: 'custom', type: 'DatasourceTestError', handling: 'handled' });
    expect(error?.error.id).toEqual(expect.any(String));
    expect(error?.error.message).not.toBe('');
  });

  it('are sent to the one entry of the intake with the token of the application', async () => {
    post.mockResolvedValue({ data: { accepted: 1, rejected: [{ index: 1, reason: 'error.id is missing' }] } });

    // The guide counts what was refused; the reasons stay with it.
    await expect(sendTestEvents(REQUEST)).resolves.toEqual({
      accepted: 1,
      rejected: 1,
      errors: [{ index: 1, reason: 'error.id is missing' }],
    });

    const [url, body, config] = post.mock.calls[0]!;
    expect(url).toBe('/api/v1/rum/events');
    expect(body).toHaveLength(2);
    expect(config).toEqual({ headers: { Authorization: 'Bearer mspat-secret' } });
  });
});

describe('what an application has sent', () => {
  const page = (...micros: number[]) => ({
    items: micros.map((timestamp) => ({ timestamp })),
    hasMore: false,
    nextCursor: null,
    previousCursor: null,
  });

  it('is its views and errors of the last hour, counted, with the last of them', async () => {
    mocks.search.mockImplementation(async ({ type }: { type: string }) =>
      type === 'views' ? page(900, 800) : page(700),
    );

    const receipt = await recentReceipt({ applicationId: 'checkout-web' });

    expect(receipt).toEqual({ rows: 3, lastReceivedAt: 900, streams: ['rum_views', 'rum_errors'] });
    expect(mocks.search).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'views',
        filters: [
          { field: 'application.id', op: '=', value: 'checkout-web' },
          { field: 'session.type', op: 'not_in', value: '[""]' },
        ],
      }),
    );
  });

  it('names only the streams that had rows, and is nothing where there are none', async () => {
    mocks.search.mockImplementation(async ({ type }: { type: string }) => (type === 'errors' ? page(5) : page()));

    expect(await recentReceipt({ applicationId: 'a' })).toEqual({
      rows: 1,
      lastReceivedAt: 5,
      streams: ['rum_errors'],
    });

    mocks.search.mockResolvedValue(page());

    expect(await recentReceipt({ applicationId: 'a' })).toEqual({ rows: 0, lastReceivedAt: null, streams: [] });
  });

  it('is nothing for a stream that was never written, and fails for any other trouble', async () => {
    const missing = new AxiosError('missing', '404', undefined, undefined, {
      status: 404,
      data: { error: 'not_found', message: 'stream `rum_views` does not exist' },
    } as never);
    mocks.search.mockRejectedValue(missing);

    expect(await recentReceipt({ applicationId: 'a' })).toEqual({ rows: 0, lastReceivedAt: null, streams: [] });

    mocks.search.mockRejectedValue(new Error('boom'));

    await expect(recentReceipt({ applicationId: 'a' })).rejects.toThrow('boom');
  });
});
