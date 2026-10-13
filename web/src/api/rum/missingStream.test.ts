import { AxiosError, type AxiosResponse } from 'axios';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ get: vi.fn() }));

vi.mock('@/lib/http', async (importOriginal) => ({
  ...((await importOriginal()) as Record<string, unknown>),
  http: { get: mocks.get },
}));

import {
  apiPerformance,
  errorRateSeries,
  getSession,
  listErrors,
  listSessions,
  webVitalsSeries,
} from '@/api/rum';

import { listApplicationSummaries } from './applications';
import { getOverview, getOverviewInsights } from './overview';

/** What the backend answers while no session has created the RUM streams. */
function missingStream(stream: string): AxiosError {
  return new AxiosError(
    'Request failed with status code 404',
    AxiosError.ERR_BAD_REQUEST,
    undefined,
    undefined,
    {
      status: 404,
      data: {
        message: `not found: stream \`${stream}\` of type \`builtin.logs\` does not exist`,
      },
    } as AxiosResponse,
  );
}

const RANGE = { org_id: 'org-1', from_micros: 10, to_micros: 20 };
const EMPTY_PAGE = {
  items: [],
  next_cursor: null,
  previous_cursor: null,
  has_more: false,
};

const READS: Array<[string, string, () => Promise<unknown>, unknown]> = [
  [
    'the overview',
    'rum_sessions',
    () => getOverview(RANGE),
    {
      metrics: {
        users: 0,
        sessions: 0,
        errorFreeRate: 0,
        lcpP75: 0,
        inpP75: 0,
        clsP75: 0,
      },
      browserDevices: [],
      regions: [],
      facets: {
        applications: [],
        environments: [],
        versions: [],
        countries: [],
        devices: [],
      },
    },
  ],
  [
    'the overview analysis',
    'rum_sessions',
    () => getOverviewInsights(RANGE),
    {
      trend: [],
      satisfaction: { good: 0, needsImprovement: 0, poor: 0, total: 0 },
      slowPages: [],
      frequentErrors: [],
    },
  ],
  ['applications', 'rum_sessions', () => listApplicationSummaries(RANGE), []],
  ['sessions', 'rum_sessions', () => listSessions(RANGE), EMPTY_PAGE],
  ['errors', 'rum_errors', () => listErrors(RANGE), EMPTY_PAGE],
  ['Web Vitals', 'rum_actions', () => webVitalsSeries(RANGE), []],
  ['API performance', 'rum_actions', () => apiPerformance(RANGE), []],
  ['the error rate', 'rum_errors', () => errorRateSeries(RANGE), []],
];

describe('RUM reads before the first session', () => {
  beforeEach(() => mocks.get.mockReset());

  it.each(READS)('answers %s as empty', async (_name, stream, read, empty) => {
    mocks.get.mockRejectedValueOnce(missingStream(stream));

    await expect(read()).resolves.toEqual(empty);
  });

  it.each(READS)('still reports any other failure of %s', async (_name, _stream, read) => {
    const failure = new AxiosError(
      'Request failed with status code 500',
      AxiosError.ERR_BAD_RESPONSE,
      undefined,
      undefined,
      { status: 500, data: { message: 'internal error' } } as AxiosResponse,
    );
    mocks.get.mockRejectedValueOnce(failure);

    await expect(read()).rejects.toBe(failure);
  });

  it('keeps a single session a "not found"', async () => {
    const failure = missingStream('rum_sessions');
    mocks.get.mockRejectedValueOnce(failure);

    await expect(
      getSession({ ...RANGE, session_id: 'session-1' }),
    ).rejects.toBe(failure);
  });
});
