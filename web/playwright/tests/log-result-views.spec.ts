import type { Page } from '@playwright/test';

import { explorerRead, explorerReadBody } from '../fixtures/explorerReads';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

const front = (page: Page) => page.locator('[data-query-tab-panel][data-active="true"]');
const views = (page: Page) => front(page).locator('[data-log-result-views]');
const view = (page: Page, name: string) => views(page).getByRole('button', { name, exact: true });

interface AskedStatement {
  statement: string;
  timeRange: { start: number; end: number };
  stream: { name: string; streamType: string };
}

/** Answers a statement of statistics by its shape, and records what was asked. */
async function serveStatistics(page: Page): Promise<AskedStatement[]> {
  const asked: AskedStatement[] = [];

  await page.route(explorerRead('query'), async (route) => {
    const body = explorerReadBody<AskedStatement>(route.request());
    asked.push(body);

    if (/AS "time"/.test(body.statement)) {
      const start = Math.floor(body.timeRange.start / 60_000_000) * 60_000_000;
      const rows = Array.from({ length: 60 }, (_, index) => [start + index * 60_000_000, 4 + (index % 7)]);

      return route.fulfill({ json: { columns: ['time', 'count'], rows, scannedRows: 354, tookMs: 9 } });
    }
    if (/GROUP BY/.test(body.statement)) {
      return route.fulfill({
        json: {
          columns: ['level', 'count'],
          rows: [['INFO', 212], ['ERROR', 71], ['WARN', 44], ['DEBUG', 27]],
          scannedRows: 354,
          tookMs: 7,
        },
      });
    }

    return route.fulfill({ json: { columns: ['count'], rows: [[354]], scannedRows: 354, tookMs: 3 } });
  });

  return asked;
}

test.describe('Views of the log results', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test('the results are looked at four ways, from one row above them', async ({ page }) => {
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');

    // A view is named once to a reader; its label is laid out twice, so that it keeps its width when chosen.
    await expect(views(page).getByRole('button')).toHaveCount(4);
    for (const name of ['Logs', 'Statistics', 'AI inspection', 'Patterns']) {
      await expect(view(page, name)).toBeVisible();
    }
    await expect(view(page, 'Logs')).toHaveAttribute('aria-pressed', 'true');
    // The switch shares the row of the summary: the rows lose no line to it.
    const [switchBox, summaryBox] = await Promise.all([
      views(page).boundingBox(),
      front(page).locator('[data-log-result-summary]').boundingBox(),
    ]);
    expect(switchBox!.y).toBeGreaterThanOrEqual(summaryBox!.y);
    expect(switchBox!.y + switchBox!.height).toBeLessThanOrEqual(summaryBox!.y + summaryBox!.height);
    expect(summaryBox!.height).toBeLessThanOrEqual(48);

    // What is of the list alone goes with the list.
    await view(page, 'Statistics').click();
    const actions = front(page).locator('[data-log-result-actions]');
    await expect(actions.getByRole('button', { name: 'Columns' })).toHaveCount(0);
    await expect(front(page).locator('[data-explorer-module="histogram"]')).toHaveCount(0);
    await expect(front(page).getByRole('navigation', { name: 'Log results pagination' })).toHaveCount(0);

    await view(page, 'Logs').click();
    await expect(actions.getByRole('button', { name: 'Columns' })).toBeVisible();
    await expect(front(page).locator('[data-explorer-module="histogram"]')).toBeVisible();
  });

  test('statistics are worked out of everything the field query matches', async ({ page }) => {
    const asked = await serveStatistics(page);
    await page.goto(`/logs?q=${encodeURIComponent("level = 'ERROR' AND timeout")}`);
    await expectExplorerResults(page, 'logs');
    await view(page, 'Statistics').click();

    const stats = front(page).locator('[data-log-stats="built"]');
    await expect(stats).toBeVisible();
    await expect.poll(() => asked.length).toBeGreaterThan(0);

    // The logs counted by their level, over the conditions the list selects by and its time range.
    expect(asked.at(-1)!.statement).toBe(
      [
        'SELECT CAST("level" AS VARCHAR) AS "level", COUNT(*) AS "count"',
        'FROM "application_logs"',
        `WHERE "level" = 'ERROR'`,
        `  AND "message" LIKE '%timeout%'`,
        'GROUP BY 1',
        'ORDER BY 2 DESC',
        'LIMIT 20',
      ].join('\n'),
    );
    expect(asked.at(-1)).toMatchObject({ stream: { name: 'application_logs', streamType: 'logs' } });
    expect(asked.at(-1)!.timeRange.end - asked.at(-1)!.timeRange.start).toBe(3_600_000_000);

    // Drawn as bars that stand on an axis, over the rows they are drawn from.
    const bars = stats.locator('[data-testid="bar-chart-bar"]');
    await expect(bars).toHaveCount(4);
    for (const box of await bars.evaluateAll((items) => items.map((item) => item.getBoundingClientRect().width))) {
      expect(box).toBeLessThanOrEqual(56);
    }
    const stage = (await stats.locator('[data-log-stats-chart-stage]').boundingBox())!;
    expect(stage.height).toBeLessThanOrEqual(320);
    await expect(stats.locator('[data-log-stats-sheet] tbody tr')).toHaveCount(4);
    await expect(stats.locator('[data-log-stats-sheet] tbody tr').first()).toHaveText('INFO212');

    // Another grouping is another statement: over time, in stretches of a round length.
    await stats.locator('[data-log-stats-control="group"]').click();
    await page.getByRole('option', { name: 'Time', exact: true }).click();
    await expect
      .poll(() => asked.at(-1)!.statement)
      .toContain('(CAST(_timestamp AS BIGINT) / 60000000) * 60000000 AS "time", COUNT(*) AS "count"');
    await expect(stats.locator('[data-log-stats-control="limit"]')).toHaveCount(0);
    await expect(stats.locator('[data-log-stats-chart]')).toHaveCount(3);
  });

  test('statistics are saved as a dashboard in one step', async ({ page }) => {
    await serveStatistics(page);
    const created: Array<{ model: Record<string, unknown> }> = [];
    await page.route('**/api/v1/dashboards', async (route) => {
      if (route.request().method() !== 'POST') return route.fallback();

      const body = route.request().postDataJSON() as { model: Record<string, unknown> };
      created.push(body);

      return route.fulfill({
        json: { id: 'dash-new', orgId: 'default', uid: 'u', title: body.model.title, tags: [], model: body.model, version: 1, createdAt: 1, updatedAt: 1 },
      });
    });
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    await view(page, 'Statistics').click();
    const stats = front(page).locator('[data-log-stats="built"]');
    await expect(stats.locator('[data-testid="bar-chart-bar"]')).toHaveCount(4);

    await stats.locator('[data-log-stats-create-dashboard]').click();
    const dialog = page.getByRole('dialog', { name: 'Create a dashboard' });
    await expect(dialog.getByLabel('Dashboard name')).toHaveValue('application_logs log statistics');
    await dialog.getByLabel('Panel title').fill('Logs by level');
    await dialog.getByRole('button', { name: 'Create', exact: true }).click();

    await expect.poll(() => created.length).toBe(1);
    // The dashboard holds the panel that was on the screen: the statement, the stream, the chart.
    expect(created[0]!.model).toMatchObject({
      engine: 'molesignal-dashboard',
      title: 'application_logs log statistics',
      timeSettings: { defaultFrom: 'now-1h', defaultTo: 'now' },
      elements: [
        {
          kind: 'panel',
          title: 'Logs by level',
          visualization: { type: 'bar_chart' },
          queries: [
            {
              dataSourceType: 'logs',
              query: { language: 'sql', stream: 'application_logs', streamType: 'logs' },
            },
          ],
        },
      ],
    });
    await expect(dialog).toHaveCount(0);

    // It is a click away, and the explorer stays where it was.
    const toast = page.getByText('Dashboard “application_logs log statistics” created');
    await expect(toast).toBeVisible();
    await expect(page).toHaveURL(/\/logs/);
    await page.getByRole('button', { name: 'Open dashboard' }).click();
    await expect(page).toHaveURL(/\/dashboards\/dash-new$/);
  });

  test('a statement that lists rows is offered one that works something out', async ({ page }) => {
    const asked = await serveStatistics(page);
    await page.goto(`/logs?sql=${encodeURIComponent('SELECT * FROM "application_logs" LIMIT 100')}`);
    await expect(front(page).locator('[data-log-result-row="logs"]').first()).toBeVisible();
    await view(page, 'Statistics').click();

    const samples = front(page).locator('[data-log-stats="samples"]');
    await expect(samples).toContainText('This statement has no statistics');
    await expect(samples.locator('[data-log-stats-sample]')).toHaveCount(2);

    // A sample goes into the editor as the reader's own statement, and is run.
    const before = asked.length;
    await samples.locator('[data-log-stats-sample]').first().click();
    await expect.poll(() => asked.length).toBeGreaterThan(before);
    expect(asked.at(-1)!.statement).toContain('CAST("level" AS VARCHAR) AS "level", COUNT(*) AS "count"');
    // Its result is what is drawn: nothing is asked a second time for the chart.
    const written = front(page).locator('[data-log-stats="written"]');
    await expect(written.locator('[data-testid="bar-chart-bar"]')).toHaveCount(4);
    await expect(written).toContainText('Drawn from the result of the SQL statement.');
  });
});

test.describe('AI inspection of the log results', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test('needs a model that is set up and switched on', async ({ page }) => {
    await page.route('**/api/v1/agent/settings/model-providers', (route) =>
      route.fulfill({
        json: [{ id: 'p1', provider: 'openai', name: 'OpenAI', defaultModel: 'gpt-4o', enabled: false, timeoutMs: 30_000, keySet: true, createdAt: 1, updatedAt: 1 }],
      }),
    );
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    await view(page, 'AI inspection').click();

    const missing = front(page).getByTestId('log-inspection-no-model');
    await expect(missing).toContainText('Set up and enable an AI model first');
    await expect(front(page).locator('[data-log-inspection-start]')).toHaveCount(0);
    // Whoever can set one up is taken to where that is done.
    await missing.getByRole('link', { name: 'Set up an AI model' }).click();
    await expect(page).toHaveURL(/\/agent\/settings\/models$/);
  });

  test('inspects the logs the query matches, and keeps the report while the reader looks elsewhere', async ({ page }) => {
    const sent: Array<Record<string, unknown>> = [];
    const answer = JSON.stringify({
      summary: 'Errors rose sharply between 09:36 and 09:42.',
      evidence: [{ label: '71 ERROR logs from payment-gateway', kind: 'logs' }],
      likely_causes: ['The acquirer timed out'],
      limitations: [],
      suggested_next_steps: ['Check the acquirer status page'],
      related_links: [],
      confidence: 'medium',
    });
    // The message is queued as a run; what the run does is read from its numbered events.
    await page.route('**/api/v1/agent/chat/*/messages', async (route) => {
      if (route.request().method() !== 'POST') return route.fallback();

      sent.push(route.request().postDataJSON() as Record<string, unknown>);

      return route.fulfill({ status: 202, json: { run: { id: 'inspection-run' }, userMessageId: 'question' } });
    });
    await page.route('**/api/v1/agent/runs/inspection-run/events?after=*', (route) =>
      route.fulfill({
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
        body: [
          { event: 'tool_start', data: { id: 't1', name: 'query_logs', arguments: {} } },
          { event: 'tool_end', data: { id: 't1', name: 'query_logs', result: '{}', truncated: false, isError: false } },
          { event: 'text', data: { text: answer } },
          { event: 'done', data: { status: 'succeeded', content: answer, promptTokens: 10, completionTokens: 12, finishReason: 'stop' } },
        ]
          .map(({ event, data }, index) => `id: ${index + 1}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`)
          .join(''),
      }),
    );
    await page.goto(`/logs?q=${encodeURIComponent("level = 'ERROR'")}`);
    await expectExplorerResults(page, 'logs');
    await view(page, 'AI inspection').click();

    const form = front(page).locator('[data-log-inspection="form"]');
    await expect(form.locator('[data-log-inspection-target]')).toContainText('application_logs');
    await expect(form.locator('[data-log-inspection-target]')).toContainText("level = 'ERROR'");
    // What happens to the logs is said before the button is pressed.
    await expect(form).toContainText('sends query results of these logs to the AI model');
    await form.getByLabel('What to attend to').fill('only payment-gateway');
    await form.locator('[data-log-inspection-start]').click();

    const report = front(page).locator('[data-log-inspection="done"]');
    await expect(report.locator('[data-log-inspection-report]')).toContainText('Errors rose sharply between 09:36 and 09:42.');
    await expect(report.locator('[data-log-inspection-report]')).toContainText('71 ERROR logs from payment-gateway');
    await expect(report.locator('[data-log-inspection-report]')).toContainText('Check the acquirer status page');
    await expect(report.locator('[data-log-inspection-tools]')).toHaveText('Called query_logs');

    // The model is asked about this stream, this range and this query, and to read only.
    expect(sent).toHaveLength(1);
    expect(sent[0]).toMatchObject({ executionPolicy: 'read_only', streamHints: ['application_logs'] });
    expect(String(sent[0]!.content)).toContain('application_logs');
    expect(String(sent[0]!.content)).toContain("level = 'ERROR'");
    expect(String(sent[0]!.content)).toContain('only payment-gateway');
    const range = sent[0]!.timeRange as { startTime: number; endTime: number };
    expect(range.endTime - range.startTime).toBe(3_600_000_000);

    // The report is the page's, not the view's: it is there on the way back.
    await view(page, 'Logs').click();
    await expect(front(page).locator('[data-log-result-row="logs"]').first()).toBeVisible();
    await view(page, 'AI inspection').click();
    await expect(report.locator('[data-log-inspection-report]')).toContainText('Errors rose sharply');
    await report.getByRole('button', { name: 'Inspect again' }).click();
    await expect(form).toBeVisible();
  });
});
