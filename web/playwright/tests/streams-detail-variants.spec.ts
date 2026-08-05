import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const SETTINGS = {
  description: null,
  index_rules: [],
  retention_filter: null,
  keep_conditions: [],
  max_query_range_hours: null,
  flatten_level: null,
  use_stream_stats_for_partitioning: false,
  store_original_data: false,
  enable_distinct_values: true,
  queryable: true,
};

const STREAMS = [
  {
    id: 'logs-id',
    name: '_molesignal',
    stream_type: 'logs',
    schema: {
      fields: [
        { name: 'body', data_type: 'utf8', nullable: true, indexed: false },
        { name: 'line', data_type: 'int64', nullable: true, indexed: false },
      ],
    },
    retention: { days: 7 },
    effective_retention: { days: 7 },
    settings: SETTINGS,
    created_at_micros: 1,
    updated_at_micros: 1,
  },
  {
    id: 'metrics-id',
    name: '_molesignal',
    stream_type: 'metrics',
    schema: {
      fields: [
        { name: 'metric_name', data_type: 'utf8', nullable: true, indexed: false },
        { name: 'value', data_type: 'float64', nullable: true, indexed: false },
      ],
    },
    retention: { days: 7 },
    effective_retention: { days: 7 },
    settings: SETTINGS,
    created_at_micros: 1,
    updated_at_micros: 1,
  },
  {
    id: 'traces-id',
    name: '_molesignal',
    stream_type: 'traces',
    schema: {
      fields: [
        { name: 'trace_id', data_type: 'utf8', nullable: true, indexed: true },
        { name: 'duration_ns', data_type: 'int64', nullable: true, indexed: false },
      ],
    },
    retention: { days: 7 },
    effective_retention: { days: 7 },
    settings: SETTINGS,
    created_at_micros: 1,
    updated_at_micros: 1,
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
            generated_at_micros: Date.now() * 1000,
            window_start_micros: 0,
            window_end_micros: Date.now() * 1000,
            window_secs: 86_400,
            streams: STREAMS.map((stream) => ({
              id: stream.id,
              name: stream.name,
              stream_type: stream.stream_type,
              status: 'healthy',
              rows: 1,
              stored_bytes: 10,
              current_stored_bytes: 10,
              first_received_at_micros: 1,
              last_received_at_micros: Date.now() * 1000,
              stats_available: true,
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

    await page.getByRole('button', { name: 'Fields & indexes' }).click();
    await expect(page.getByTitle('Storage type: utf8')).toContainText('String');
    await expect(page.getByTitle('Storage type: int64')).toContainText('Integer');

    await page.getByRole('link', { name: 'Switch to the Metrics stream' }).click();
    await expect(page).toHaveURL('/streams/metrics-id');
    await expect(page.getByTitle('Current type: Metrics')).toBeVisible();
    await expect(page.getByTitle('Storage type: float64')).toContainText('Decimal');
  });
});
