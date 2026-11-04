import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const SETTINGS = {
  description: null,
  indexRules: [],
  retentionFilter: null,
  keepConditions: [],
  maxQueryRangeHours: null,
  flattenLevel: null,
  useStreamStatsForPartitioning: false,
  storeOriginalData: false,
  enableDistinctValues: true,
  queryable: true,
};

const STREAMS = [
  {
    id: 'logs-id',
    name: '_molesignal',
    streamType: 'logs',
    schema: {
      fields: [
        { name: 'body', dataType: 'utf8', nullable: true, indexed: false },
        { name: 'line', dataType: 'int64', nullable: true, indexed: false },
      ],
    },
    retention: { days: 7 },
    effectiveRetention: { days: 7 },
    settings: SETTINGS,
    createdAt: 1,
    updatedAt: 1,
  },
  {
    id: 'metrics-id',
    name: '_molesignal',
    streamType: 'metrics',
    schema: {
      fields: [
        { name: 'metric_name', dataType: 'utf8', nullable: true, indexed: false },
        { name: 'value', dataType: 'float64', nullable: true, indexed: false },
      ],
    },
    retention: { days: 7 },
    effectiveRetention: { days: 7 },
    settings: SETTINGS,
    createdAt: 1,
    updatedAt: 1,
  },
  {
    id: 'traces-id',
    name: '_molesignal',
    streamType: 'traces',
    schema: {
      fields: [
        { name: 'trace_id', dataType: 'utf8', nullable: true, indexed: true },
        { name: 'duration_ns', dataType: 'int64', nullable: true, indexed: false },
      ],
    },
    retention: { days: 7 },
    effectiveRetention: { days: 7 },
    settings: SETTINGS,
    createdAt: 1,
    updatedAt: 1,
  },
] as const;

test.describe('stream detail signal variants', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/streams**', async (route) => {
      const pathname = new URL(route.request().url()).pathname;
      if (pathname === '/api/v1/streams/runtime') {
        await route.fulfill({
          json: {
            generatedAt: Date.now() * 1000,
            windowStart: 0,
            windowEnd: Date.now() * 1000,
            windowSecs: 86_400,
            streams: STREAMS.map((stream) => ({
              id: stream.id,
              name: stream.name,
              streamType: stream.streamType,
              status: 'healthy',
              rows: 1,
              storedBytes: 10,
              currentStoredBytes: 10,
              firstReceivedAt: 1,
              lastReceivedAt: Date.now() * 1000,
              statsAvailable: true,
              buckets: [],
            })),
          },
        });
        return;
      }

      const id = pathname.match(/^\/api\/v1\/streams\/([^/]+)$/)?.[1];
      if (id) {
        const stream = STREAMS.find((item) => item.id === id);
        await route.fulfill(stream ? { json: stream } : { status: 404, json: {} });
        return;
      }

      await route.fulfill({ json: STREAMS });
    });
  });

  test('keeps list and detail signal types aligned and renders logical field types', async ({
    page,
  }) => {
    await page.goto('/streams/logs-id');

    await expect(page.getByTitle('Current type: Logs')).toBeVisible();
    await expect(page.getByRole('link', { name: 'Switch to the Metrics stream' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Switch to the Traces stream' })).toBeVisible();

    await page.getByRole('tab', { name: 'Fields & indexes' }).click();
    await expect(page.getByTitle('Storage type: utf8')).toContainText('String');
    await expect(page.getByTitle('Storage type: int64')).toContainText('Integer');

    await page.getByRole('link', { name: 'Switch to the Metrics stream' }).click();
    await expect(page).toHaveURL('/streams/metrics-id');
    await expect(page.getByTitle('Current type: Metrics')).toBeVisible();
    await expect(page.getByTitle('Storage type: float64')).toContainText('Decimal');
  });
});
