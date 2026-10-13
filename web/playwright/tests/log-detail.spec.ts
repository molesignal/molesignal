import { expect, test, type Page } from '@playwright/test';

import { installMockShellSession } from '../fixtures/mockSession';

const field = (name: string, dataType = 'utf8') => ({ name, data_type: dataType, nullable: true, indexed: false });
const STREAM = {
  id: 'application-logs',
  name: 'application_logs',
  stream_type: 'logs',
  schema: {
    fields: [
      { name: '_timestamp', data_type: 'timestamp', nullable: false, indexed: true },
      field('level'),
      field('service.name'),
      field('message'),
      field('trace_id'),
      field('http.method'),
      field('http.route'),
      field('http.status_code', 'int64'),
      field('duration_ms', 'float64'),
      field('k8s.pod.name'),
      field('host.name'),
      field('user.id'),
    ],
  },
  retention: { days: 7 },
  effective_retention: { days: 7 },
  settings: {
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
  },
  created_at_micros: 1,
  updated_at_micros: 1,
};
const LEVELS = ['INFO', 'ERROR', 'WARN', 'INFO', 'DEBUG', 'INFO'];
const FAILURE = 'failed to reserve stock: sku=SKU-48213 requested=4 available=1';

async function serveLogs(page: Page) {
  await installMockShellSession(page);
  // The newest rows of the last minute, 1.37 s apart, so no two share a millisecond.
  const now = Date.now();
  const items = LEVELS.map((level, index) => ({
    _timestamp: (now - 5_000 - index * 1_370) * 1_000,
    level,
    'service.name': index % 2 === 0 ? 'checkout-api' : 'payments',
    message: level === 'ERROR' ? FAILURE : `request ${index} completed`,
    trace_id: `trace-${index}`,
    'http.method': 'GET',
    'http.route': '/api/v1/cart/:id',
    'http.status_code': level === 'ERROR' ? 504 : 200,
    duration_ms: 20 + index,
    'k8s.pod.name': `checkout-api-7d9f8b6c5-p${index}`,
    'host.name': `ip-10-0-0-${index}.internal`,
    'user.id': `u_${index}`,
  }));
  await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [STREAM] }));
  await page.route('**/api/v1/web/logs', (route) =>
    route.fulfill({ json: { items, next_cursor: null, previous_cursor: null, has_more: false } }),
  );
  await page.route('**/api/v1/field_masking/effective/**', (route) =>
    route.fulfill({ json: { fields: [] } }),
  );
}

test.describe('Logs list and detail', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveLogs(page);
    await page.goto('/logs');
    await expect(page.locator('[data-log-result-row="logs"]')).toHaveCount(LEVELS.length);
  });

  test('lists a log as a line that can be scanned: when, how bad, where, what', async ({ page }) => {
    const rows = page.locator('[data-log-result-row="logs"]');

    // To the millisecond, which is what orders two logs of one second; the day only where it differs.
    const times = await rows.locator('[data-log-field="_timestamp"]').allInnerTexts();
    for (const time of times) expect(time).toMatch(/^(\d{2}[-/]\d{2} )?\d{2}:\d{2}:\d{2}\.\d{3}$/);
    expect(new Set(times).size).toBe(times.length);

    // A stream with a message opens with the fields that matter, and the message has the room.
    await expect(page.locator('[data-log-result-columns="logs"] > *')).toHaveText([
      '_timestamp',
      'level',
      'service.name',
      'message',
      'trace_id',
    ]);
    const message = await rows.first().locator('[data-log-field="message"]').boundingBox();
    expect(message!.width).toBeGreaterThan(400);

    // The level runs down the row's edge, loud only where something went wrong, and is a word
    // in the row, not a chip.
    await expect(rows.nth(1)).toHaveAttribute('data-log-level', 'error');
    const edge = (index: number) => rows.nth(index).evaluate((row) => getComputedStyle(row).borderLeftColor);
    const [info, error, warn] = await Promise.all([edge(0), edge(1), edge(2)]);
    expect(new Set([info, error, warn]).size).toBe(3);
    const word = rows.nth(1).locator('[data-log-field="level"] span');
    await expect(word).toHaveText('ERROR');
    await expect(word).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');

    // A field every loaded record carries says no count; the number of rows is said once, above.
    const panel = page.locator('aside[data-variant="utility"]');
    await expect(panel.locator('[data-log-field-row="level"] [data-log-field-count]')).toHaveCount(0);
  });

  test('lists the fields that share the first part of their names under it', async ({ page }) => {
    const panel = page.locator('aside[data-variant="utility"]').first();
    const http = panel.locator('[data-field-group="http"]');
    const toggle = http.getByRole('button', { name: 'http: 3 fields' });

    // Three fields of `http` are one entry that says how many they are, closed until asked.
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(http.locator('[data-field-group-count]')).toHaveText('3');
    await expect(panel.locator('[data-log-field-row="http.method"]')).toHaveCount(0);
    // A first part only one field has says nothing: that field keeps its whole name and its own card.
    await expect(panel.locator('[data-log-field-row="k8s.pod.name"]')).toBeVisible();
    await expect(panel.locator('[data-field-group="k8s"]')).toHaveCount(0);
    await expect(panel.locator('[data-log-field-row="k8s.pod.name"]')).not.toHaveCSS('box-shadow', 'none');
    // The list is divided by what a field is to the reader: on display, kept in an index, or neither.
    // The fields on display are the columns of the list, in their order: they stay a plain list.
    const sections = panel.locator('[data-side-panel-section]');
    await expect(sections.getByRole('button', { name: /^(Displayed|Indexed|Other) fields/ })).toHaveText([
      'Displayed fields4',
      'Indexed fields1',
      'Other fields7',
    ]);
    await expect(sections.nth(0).locator('[data-log-field-row="service.name"]')).toBeVisible();
    await expect(sections.nth(1).locator('[data-log-field-row="_timestamp"]')).toBeVisible();
    await expect(sections.nth(2).locator('[data-field-group="http"]')).toBeVisible();
    // A part folds away under its heading.
    await sections.nth(0).getByRole('button', { name: /^Displayed fields/ }).click();
    await expect(panel.locator('[data-log-field-row="service.name"]')).toHaveCount(0);
    await sections.nth(0).getByRole('button', { name: /^Displayed fields/ }).click();

    // Open, its fields are inside its card by the rest of their names, each a row on a ground of its own.
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    const fields = http.locator('[data-field-group-fields="http"]');
    await expect(fields.locator('[data-log-field-row]')).toHaveCount(3);
    const status = fields.locator('[data-log-field-row="http.status_code"]');
    await expect(status.getByText('status_code', { exact: true })).toHaveAttribute('title', 'http.status_code');
    const [groupBox, statusBox] = await Promise.all([http.boundingBox(), status.boundingBox()]);
    expect(statusBox!.x).toBeGreaterThan(groupBox!.x);
    expect(statusBox!.x + statusBox!.width).toBeLessThan(groupBox!.x + groupBox!.width);
    const ground = (locator: typeof http) =>
      locator.evaluate((element) => getComputedStyle(element).backgroundColor);
    expect(await ground(status)).not.toBe(await ground(http));
    // The group is one small card lifted off the list; the fields in it are rows, not cards.
    await expect(http).not.toHaveCSS('box-shadow', 'none');
    await expect(status).toHaveCSS('box-shadow', 'none');

    // A field under a group opens on its values like any other, inside the group.
    await status.getByRole('button').first().click();
    const values = fields.locator('[data-log-field-values="http.status_code"]');
    await expect(values.locator('[data-field-value="200"]')).toBeVisible();
    await expect(values.locator('[data-field-value="504"]')).toBeVisible();

    // What can be asked of it shows while the pointer is on its row: here, to put it on display.
    await status.hover();
    await status.getByRole('button', { name: 'Show http.status_code' }).click();
    await expect(page.locator('[data-log-result-columns] [data-log-field="http.status_code"]')).toBeVisible();
    await expect(http.getByRole('button', { name: 'http: 2 fields' })).toBeVisible();

    // A search cuts the list to what matches: one match is a field of its own again, by its whole name.
    await panel.getByRole('textbox', { name: 'Filter fields' }).fill('route');
    await expect(panel.locator('[data-field-group]')).toHaveCount(0);
    await expect(panel.locator('[data-log-field-row="http.route"]')).toContainText('http.route');
  });

  test('offers pages of a hundred rows to five thousand, in a list as wide as its numbers', async ({ page }) => {
    const size = page.locator('[data-page-size]');

    // In the type of the labels beside it, and no wider than the number and its arrow.
    await expect(size).toHaveText('100');
    await expect(size).toHaveCSS('font-size', '12px');
    expect((await size.boundingBox())!.width).toBeLessThan(64);

    await size.click();
    const options = page.getByRole('option');
    await expect(options).toHaveText(['100', '200', '500', '1,000', '2,000', '5,000']);
    await expect(options.first()).toHaveCSS('font-size', '12px');

    // The list hugs its numbers: no tick column before them, no empty half after them.
    const list = (await page.getByRole('listbox').boundingBox())!;
    const widest = (await options.last().boundingBox())!;
    expect(list.width).toBeLessThan(72);
    expect(widest.width).toBeGreaterThan(list.width - 12);
    // The numbers line up on their last digit.
    const ends = await options.evaluateAll((items) =>
      items.map((item) => item.querySelector(':scope > span:last-child')!.getBoundingClientRect().right),
    );
    expect(Math.max(...ends) - Math.min(...ends)).toBeLessThan(1);
    // The chosen one is told by its fill and weight.
    await expect(options.first()).toHaveAttribute('data-state', 'checked');
    await expect(options.first()).toHaveCSS('font-weight', '600');
  });

  test('opens a log in a drawer that says each thing once', async ({ page }) => {
    await page.locator('[data-log-result-row="logs"]').nth(1).click();
    const drawer = page.getByRole('complementary', { name: 'Log detail drawer' });
    await expect(drawer).toBeVisible();

    // What it is: level and moment in the head, then what it says, in full.
    await expect(drawer.locator('[data-log-detail-moment]')).toHaveText(
      /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}$/,
    );
    await expect(drawer.getByText(FAILURE)).toHaveCount(1);
    await expect(drawer.locator('[data-log-detail-message]')).toHaveText(FAILURE);
    await expect(drawer.getByLabel('2 of 6')).toHaveText('2 / 6');

    // The rest of the record, in one of three views; there is no page that repeats the head.
    const views = drawer.getByRole('group', { name: 'Views of the log' });
    await expect(views.getByRole('button')).toHaveCount(3);
    for (const name of [/^Fields/, 'Raw JSON', 'Context']) {
      await expect(views.getByRole('button', { name })).toBeVisible();
    }
    const fields = drawer.locator('[data-log-detail-field]');
    await expect(fields).toHaveCount(10);
    await expect(drawer.locator('[data-log-detail-field="message"]')).toHaveCount(0);
    await expect(drawer.locator('[data-log-detail-field="_timestamp"]')).toHaveCount(0);

    // What can be asked of a value comes with the pointer, inside the drawer, on the row.
    const status = drawer.locator('[data-log-detail-field="http.status_code"]');
    await status.hover();
    const include = status.getByRole('button', { name: 'Add include filter' });
    await expect(include).toBeVisible();
    const [drawerBox, rowBox, lastAction] = await Promise.all([
      drawer.boundingBox(),
      status.boundingBox(),
      status.getByRole('button', { name: 'Show as column' }).boundingBox(),
    ]);
    expect(lastAction!.x + lastAction!.width).toBeLessThanOrEqual(drawerBox!.x + drawerBox!.width);
    expect(lastAction!.y).toBeGreaterThanOrEqual(rowBox!.y);
    // Nothing in the drawer is wider than the drawer.
    await expect
      .poll(() =>
        drawer.locator('[data-log-detail-scroll]').evaluate((element) => element.scrollWidth <= element.clientWidth),
      )
      .toBe(true);

    await views.getByRole('button', { name: 'Raw JSON' }).click();
    await expect(drawer.getByText('message:')).toBeVisible();
    await views.getByRole('button', { name: 'Context' }).click();
    await expect(drawer.locator('[aria-current="true"]')).toContainText(FAILURE);
  });

  test('groups the logs by what they say, from the Patterns view of the results', async ({ page }) => {
    const patternsMode = page.locator('[data-log-result-views]').getByRole('button', { name: 'Patterns' });
    await expect(patternsMode).toHaveAttribute('aria-pressed', 'false');
    await patternsMode.click();
    await expect(patternsMode).toHaveAttribute('aria-pressed', 'true');

    // Each kind of message once, in place of the logs one by one.
    const patterns = page.locator('[data-log-pattern]');
    await expect(patterns).toHaveCount(2);
    await expect(page.locator('[data-log-result-row="logs"]')).toHaveCount(0);
    // Columns and row height are of the list: they are not offered where there is none.
    const actions = page.locator('[data-log-result-actions]');
    await expect(actions.getByRole('button', { name: 'Columns' })).toHaveCount(0);
    await expect(actions.getByRole('button', { name: 'Hide histogram' })).toBeVisible();

    // Five of the six logs say the same thing with a number of their own: the count once, and as a length.
    const requests = patterns.first();
    await expect(requests.locator('[data-log-pattern-text]')).toHaveText('request * completed');
    await expect(requests.locator('[data-log-pattern-share]')).toHaveAttribute('style', /width: 83\.3/);
    await expect(requests).not.toContainText('%');
    // Its logs are of three levels, the loudest first; the failure is of one, and says only that.
    await expect(requests.locator('[data-log-pattern-levels]')).toHaveText('WARN1INFO3DEBUG1');
    await expect(patterns.nth(1)).toHaveAttribute('data-log-level', 'error');
    await expect(patterns.nth(1).locator('[data-log-pattern-levels]')).toHaveText('ERROR');
    // Where the list has its pages stands what the patterns were read from: here, all there is.
    await expect(page.locator('[data-log-patterns-basis]')).toHaveText('Every event that matches');
    await expect(page.getByRole('navigation', { name: 'Log results pagination' })).toHaveCount(0);

    // Opening a pattern shows its logs, and one of them opens in the drawer.
    await requests.locator('[data-log-pattern-text]').click();
    const examples = requests.locator('[data-log-pattern-examples]').getByRole('button');
    await expect(examples).toHaveCount(5);
    await examples.nth(1).click();
    const drawer = page.getByRole('complementary', { name: 'Log detail drawer' });
    await expect(drawer.locator('[data-log-detail-message]')).toHaveText('request 2 completed');
    await expect(examples.nth(1)).toHaveAttribute('aria-pressed', 'true');
    // Escape gives the keyboard back to the log the drawer was opened from.
    await page.keyboard.press('Escape');
    await expect(drawer).toHaveCount(0);
    await expect(examples.nth(1)).toBeFocused();

    // Asking for a pattern writes the words all of its logs have into the query.
    const failure = patterns.nth(1);
    await failure.hover();
    await failure.getByRole('button', { name: /^Show only logs like failed to reserve stock/ }).click();
    await expect(page.locator('[data-query-workbench-appearance="surface"]')).toContainText(
      "message contains 'failed to reserve stock: sku='",
    );

    // And back to the logs one by one.
    await page.locator('[data-log-result-views]').getByRole('button', { name: 'Logs' }).click();
    await expect(page.locator('[data-log-result-row="logs"]')).toHaveCount(LEVELS.length);
    await expect(actions.getByRole('button', { name: 'Columns' })).toBeVisible();
    await expect(page.getByRole('navigation', { name: 'Log results pagination' })).toBeVisible();
  });

  test('steps through logs by clicking them, and gives the keyboard back on Escape', async ({ page }) => {
    const rows = page.locator('[data-log-result-row="logs"]');
    await rows.nth(1).click();
    const drawer = page.getByRole('complementary', { name: 'Log detail drawer' });
    await expect(drawer.getByLabel('2 of 6')).toBeVisible();

    // Another log in the list is one click away: the drawer shows it, and stays open.
    const fourth = (await rows.nth(3).boundingBox())!;
    await page.mouse.click(fourth.x + 60, fourth.y + fourth.height / 2);
    await expect(drawer.getByLabel('4 of 6')).toBeVisible();
    await expect(rows.nth(3)).toHaveAttribute('aria-pressed', 'true');

    // So are the ones on either side, from the head.
    await drawer.getByRole('button', { name: 'Next' }).click();
    await expect(drawer.getByLabel('5 of 6')).toBeVisible();
    await drawer.getByRole('button', { name: 'Previous' }).click();
    await drawer.getByRole('button', { name: 'Previous' }).click();
    await expect(drawer.getByLabel('3 of 6')).toBeVisible();

    // Escape closes it, and the keyboard is where it was: on the log that was open.
    await page.keyboard.press('Escape');
    await expect(drawer).toHaveCount(0);
    await expect(rows.nth(2)).toBeFocused();

    // A click anywhere that is not a log closes it too.
    await rows.nth(0).click();
    await expect(drawer).toBeVisible();
    await page.locator('[data-log-result-summary]').click({ position: { x: 12, y: 12 }, force: true });
    await expect(drawer).toHaveCount(0);
  });
});


test.describe('Log patterns beyond a page of the list', () => {
  const TOTAL = 1_240;

  test('reads the query further back than the page, and further when asked', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await installMockShellSession(page);
    const now = Date.now();
    // One log a second, newest first; every tenth is a failure.
    const items = Array.from({ length: TOTAL }, (_, index) => ({
      _timestamp: (now - 5_000 - index * 1_000) * 1_000,
      level: index % 10 === 0 ? 'ERROR' : 'INFO',
      'service.name': 'checkout-api',
      message:
        index % 10 === 0
          ? `failed to reserve stock: sku=SKU-${index} requested=4 available=1`
          : `request ${index} completed`,
    }));
    const asked: number[] = [];
    await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [STREAM] }));
    // The list endpoint as it pages: `limit` logs from the start, then from where the cursor says.
    await page.route('**/api/v1/web/logs', (route) => {
      const body = route.request().postDataJSON() as { cursor?: string; limit?: number };
      const offset = body.cursor ? Number(body.cursor) : 0;
      const limit = body.limit ?? 20;
      const next = offset + limit < TOTAL ? String(offset + limit) : null;
      asked.push(limit);

      return route.fulfill({
        json: {
          items: items.slice(offset, offset + limit),
          next_cursor: next,
          previous_cursor: null,
          has_more: next !== null,
        },
      });
    });
    await page.route('**/api/v1/field_masking/effective/**', (route) => route.fulfill({ json: { fields: [] } }));
    await page.goto('/logs');
    await expect(page.locator('[data-log-result-row="logs"]')).toHaveCount(100);

    await page.locator('[data-log-result-views]').getByRole('button', { name: 'Patterns' }).click();
    const patterns = page.locator('[data-log-pattern]');
    const summary = page.locator('[data-log-result-summary]');
    const basis = page.locator('[data-log-patterns-basis]');
    const readOlder = basis.getByRole('button', { name: 'Read 500 older' });

    // A page of the list is few to say what kinds there are: the latest five hundred are read.
    await expect(readOlder).toBeVisible();
    await expect(summary).toContainText('500 events');
    await expect(patterns).toHaveCount(2);
    await expect(patterns.nth(0)).toContainText('450');
    await expect(patterns.nth(1)).toContainText('50');
    await expect(basis).toContainText('The latest events that match');
    await expect(basis).toContainText('back to ');

    await readOlder.click();
    await expect(summary).toContainText('1,000 events');
    await expect(patterns.nth(0)).toContainText('900');

    // 1,240 match: reading on comes to the end of them, and there is nothing older to offer.
    await readOlder.click();
    await expect(basis).toHaveText('Every event that matches');
    await expect(summary).toContainText('1,240 events');
    await expect(patterns.nth(1)).toContainText('124');
    // The list asked for its page; the patterns for a step at a time, each in one answer.
    expect(asked).toEqual([100, 500, 500, 500]);

    // A log under a pattern is one of the logs read, wherever the list's page ended.
    const failures = patterns.nth(1);
    await failures.locator('[data-log-pattern-text]').click();
    await failures.locator('[data-log-pattern-example]').nth(5).click();
    const drawer = page.getByRole('complementary', { name: 'Log detail drawer' });
    await expect(drawer.locator('[data-log-detail-message]')).toHaveText(
      'failed to reserve stock: sku=SKU-50 requested=4 available=1',
    );
    await expect(drawer.getByLabel('51 of 1240')).toBeVisible();
    await page.keyboard.press('Escape');

    // The list still has the page it had.
    await page.locator('[data-log-result-views]').getByRole('button', { name: 'Logs' }).click();
    await expect(page.locator('[data-log-result-row="logs"]')).toHaveCount(100);
    await expect(summary).toContainText('100 events');
  });
});
