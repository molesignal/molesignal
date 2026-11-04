import { describe, expect, it, vi } from 'vitest';

import * as queryApi from '@/api/query';

import { recentEvent } from './recentEvent';

vi.mock('@/api/query', () => ({ runQuery: vi.fn() }));
vi.mock('@/api/missingStream', () => ({ whenStreamExists: (read: () => unknown) => read() }));

const answer = (columns: string[], row: unknown[]) =>
  vi.mocked(queryApi.runQuery).mockResolvedValue({ columns, rows: [row] } as never);

describe('recentEvent', () => {
  it('leaves out the columns the event has no value for', async () => {
    answer(['message', 'host'], ['hello', null]);

    await expect(recentEvent('org', 'app_logs', 'logs')).resolves.toEqual({ message: 'hello' });
  });

  it('gives a profile its labels as a step sees them: an object, not the text they are stored as', async () => {
    answer(['service', 'labels'], ['checkout', '{"pod":"web-1"}']);

    await expect(recentEvent('org', 'default', 'profiles')).resolves.toEqual({
      service: 'checkout',
      labels: { pod: 'web-1' },
    });
  });

  it('keeps a text field of another signal as it is', async () => {
    answer(['labels'], ['{"pod":"web-1"}']);

    await expect(recentEvent('org', 'app_logs', 'logs')).resolves.toEqual({ labels: '{"pod":"web-1"}' });
  });
});
