import { expect, type Page } from '@playwright/test';

import { mountMockRoutes, test } from '../fixtures/mockBackend';

// The Functions pages: the list of what a pipeline can call, and the workbench a function is
// written and tried in.

interface FunctionRow {
  id: string;
  name: string;
  language: 'vrl' | 'js';
  source: string;
  params_schema: Record<string, unknown>;
  is_builtin: boolean;
  description: string;
  created_at_micros: number;
  updated_at_micros: number;
}

const REDACT: FunctionRow = {
  id: 'preset-redact',
  name: 'redact-email',
  language: 'vrl',
  source: '.message = replace(to_string(.message) ?? "", r\'[a-z]+@[a-z.]+\', "[redacted-email]")',
  params_schema: {},
  is_builtin: true,
  description: 'Replaces email addresses in message with a placeholder for redaction.',
  created_at_micros: 0,
  updated_at_micros: 0,
};

const NORMALIZE: FunctionRow = {
  id: 'preset-normalize',
  name: 'normalize-logs',
  language: 'vrl',
  source: '. = parse_json!(.message)',
  params_schema: {},
  is_builtin: true,
  description: 'Parses the JSON message and normalizes level to lowercase.',
  created_at_micros: 0,
  updated_at_micros: 0,
};

const OWN: FunctionRow = {
  id: 'fn-mask',
  name: 'mask-cards',
  language: 'js',
  source: "molesignal.set('card', '****');",
  params_schema: {},
  is_builtin: false,
  description: '',
  created_at_micros: 1_760_000_000_000_000,
  updated_at_micros: 1_760_000_000_000_000,
};

interface Served {
  runs: Array<{ source: string }>;
  created: Array<{ name: string; language: string; source: string }>;
}

/** Serves the functions API from a fixed list. A sample run fails when the source says `fail`. */
async function serveFunctions(page: Page, functions: FunctionRow[]): Promise<Served> {
  const served: Served = { runs: [], created: [] };

  await page.route('**/api/v1/functions', (route) => {
    if (route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as Served['created'][number];
      served.created.push(body);
      return route.fulfill({ json: { ...OWN, ...body, id: 'fn-new' } });
    }
    return route.fulfill({ json: functions });
  });
  await page.route('**/api/v1/functions/run', (route) => {
    const body = route.request().postDataJSON() as { source: string; input: Record<string, unknown> };
    served.runs.push({ source: body.source });

    if (body.source.includes('fail')) {
      return route.fulfill({ status: 400, json: { error: 'invalid_argument', message: 'undefined variable `fail`' } });
    }
    return route.fulfill({ json: { output: { ...body.input, environment: 'production' } } });
  });
  for (const item of [...functions, { ...OWN, id: 'fn-new' }]) {
    await page.route(`**/api/v1/functions/${item.id}`, (route) => route.fulfill({ json: item }));
  }

  return served;
}

const head = (page: Page) => page.locator('[data-list-head]');
const views = (page: Page) => page.locator('[data-function-views]');
const rowsShown = (page: Page) => page.locator('[data-function-row]');
const drawer = (page: Page) => page.getByRole('dialog');

test.beforeEach(async ({ page, mockServer }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mountMockRoutes(page, mockServer.port, { theme: 'light' });
});

test.describe('the list of functions', () => {
  test('says what it shows at its own head: a search and the three views, counted', async ({ page }) => {
    await serveFunctions(page, [OWN, REDACT, NORMALIZE]);
    await page.goto('/functions');

    await expect(head(page).locator('[data-function-search]')).toBeVisible();
    // In this order, each with how many it holds.
    await expect(views(page).getByRole('button')).toHaveCount(3);
    await expect(views(page).getByRole('button').nth(0)).toHaveAccessibleName(/^All\s*3$/);
    await expect(views(page).getByRole('button').nth(1)).toHaveAccessibleName(/^Yours\s*1$/);
    await expect(views(page).getByRole('button').nth(2)).toHaveAccessibleName(/^Presets\s*2$/);
    await expect(rowsShown(page)).toHaveCount(3);

    // One surface under the title: no card of its own for the search.
    await expect(page.locator('[data-page-body] > *')).toHaveCount(1);
  });

  test('a view keeps its functions, and a preset is found by what it does', async ({ page }) => {
    await serveFunctions(page, [OWN, REDACT, NORMALIZE]);
    await page.goto('/functions');

    await views(page).getByRole('button', { name: /Yours/ }).click();
    await expect(rowsShown(page)).toHaveCount(1);
    await expect(rowsShown(page)).toContainText('mask-cards');

    await views(page).getByRole('button', { name: /All/ }).click();
    await head(page).locator('[data-function-search]').fill('placeholder');
    await expect(rowsShown(page)).toHaveCount(1);
    await expect(rowsShown(page)).toContainText('redact-email');

    // Nothing matches: the head stays, and the search is taken back where it was made.
    await head(page).locator('[data-function-search]').fill('nothing-like-this');
    await expect(page.getByText('No function matches')).toBeVisible();
    await page.getByRole('button', { name: 'Clear search' }).click();
    await expect(rowsShown(page)).toHaveCount(3);
  });

  test('a preset is told from a function of the workspace in its row', async ({ page }) => {
    await serveFunctions(page, [OWN, REDACT]);
    await page.goto('/functions');

    const preset = page.locator('tbody tr').filter({ hasText: 'redact-email' });
    await expect(preset).toContainText('Built-in');
    await expect(preset).toContainText(REDACT.description);
    await expect(preset).toContainText('VRL');
    await expect(preset).toContainText('Ships with the product');
    await expect(preset.getByRole('button', { name: 'Start from this' })).toBeVisible();

    const own = page.locator('tbody tr').filter({ hasText: 'mask-cards' });
    await expect(own).toContainText('JavaScript');
    await expect(own).not.toContainText('Built-in');
    await expect(own.getByRole('button')).toHaveCount(0);
  });

  test('with none of its own, the workspace is pointed to the presets, and New function is offered once', async ({
    page,
  }) => {
    await serveFunctions(page, [REDACT, NORMALIZE]);
    await page.goto('/functions');

    await views(page).getByRole('button', { name: /Yours/ }).click();
    await expect(page.getByText('No functions of your own yet')).toBeVisible();
    await expect(page.getByRole('button', { name: 'New function' })).toHaveCount(1);

    await page.getByRole('button', { name: 'See the presets' }).click();
    await expect(rowsShown(page)).toHaveCount(2);
    await expect(views(page).getByRole('button', { name: /Presets/ })).toHaveAttribute('aria-pressed', 'true');
  });
});

test.describe('a preset', () => {
  test('is read in a drawer, and a function can start from it', async ({ page }) => {
    await serveFunctions(page, [OWN, REDACT]);
    await page.goto('/functions');

    await page.locator('tbody tr').filter({ hasText: 'redact-email' }).click();
    await expect(drawer(page)).toContainText('redact-email');
    await expect(drawer(page)).toContainText('Preset · VRL');
    await expect(drawer(page)).toContainText(REDACT.description);
    await expect(drawer(page).locator('pre')).toContainText('[redacted-email]');

    await drawer(page).getByRole('button', { name: 'Start from this' }).click();
    await expect(page).toHaveURL(/\/functions\/new\?from=preset-redact$/);

    // A copy, under a name of its own, said to be one.
    await expect(page.getByLabel('Name')).toHaveValue('redact-email-copy');
    await expect(page.locator('[data-page-title-row]')).toContainText('Started as a copy of redact-email');
    await expect(page.locator('[data-function-source]')).toContainText('[redacted-email]');
  });

  test('starts a function from its row without opening the drawer', async ({ page }) => {
    await serveFunctions(page, [REDACT]);
    await page.goto('/functions');

    await page.locator("[data-function-start-from='redact-email']").click();

    await expect(page).toHaveURL(/\/functions\/new\?from=preset-redact$/);
    await expect(drawer(page)).toHaveCount(0);
  });

  test('the row button is its own with the keyboard too', async ({ page }) => {
    await serveFunctions(page, [REDACT]);
    await page.goto('/functions');

    await page.locator("[data-function-start-from='redact-email']").focus();
    await page.keyboard.press('Enter');

    await expect(page).toHaveURL(/\/functions\/new\?from=preset-redact$/);
  });

  test('a copy does not take the name of a function the workspace has', async ({ page }) => {
    await serveFunctions(page, [{ ...OWN, id: 'fn-copy', name: 'redact-email-copy' }, REDACT]);
    await page.goto('/functions');

    await page.locator("[data-function-start-from='redact-email']").click();

    await expect(page.getByLabel('Name')).toHaveValue('redact-email-copy-2');
  });
});

test.describe('the workbench of a function', () => {
  test('saving stands in the title row, and says what it waits for', async ({ page }) => {
    const served = await serveFunctions(page, [REDACT]);
    await page.goto('/functions/new');

    const titleRow = page.locator('[data-page-title-row]');
    const save = titleRow.locator('[data-function-save]');
    await expect(titleRow.getByRole('button', { name: 'Cancel' })).toBeVisible();
    await expect(save).toBeDisabled();
    // Nothing to save at the foot of the page any more.
    await expect(page.locator('[data-function-actions]')).toHaveCount(0);

    await save.hover({ force: true });
    await expect(page.getByRole('tooltip')).toContainText('Give the function a name first.');

    await page.getByLabel('Name').fill('to-production');
    await expect(save).toBeEnabled();
    await save.click();

    await expect.poll(() => served.created.length).toBe(1);
    expect(served.created[0]).toMatchObject({ name: 'to-production', language: 'vrl' });
    await expect(page).toHaveURL(/\/functions\/fn-new$/);
  });

  test('stands as three cards: what it is, its source, its test', async ({ page }) => {
    await serveFunctions(page, []);
    await page.goto('/functions/new');

    const [definition, source, testCard] = await Promise.all([
      page.locator('[data-function-definition]').boundingBox(),
      page.locator('[data-function-source]').boundingBox(),
      page.locator('[data-function-test]').boundingBox(),
    ]);

    // The definition over the two, the two side by side, a card's gap between each.
    expect(source!.y - (definition!.y + definition!.height)).toBeCloseTo(12, 0);
    expect(testCard!.x - (source!.x + source!.width)).toBeCloseTo(12, 0);
    expect(Math.abs(source!.y - testCard!.y)).toBeLessThanOrEqual(0.5);
    // As tall as each other: the page ends on one line.
    expect(Math.abs(source!.height - testCard!.height)).toBeLessThanOrEqual(1);
  });

  test('a sample run is offered once and says how it went once', async ({ page }) => {
    const served = await serveFunctions(page, []);
    await page.goto('/functions/new');

    await expect(page.locator('[data-function-run]')).toHaveCount(1);
    await expect(page.locator("[data-function-test] [data-function-status='pending']")).toHaveText('Not run');

    await page.locator('[data-function-run]').click();

    await expect(page.locator("[data-function-test] [data-function-status='valid']")).toContainText('Succeeded');
    await expect(page.locator("[data-function-source] [data-function-status='valid']")).toHaveText('Validation passed');
    await expect(page.locator('[data-function-test]')).toContainText('"environment": "production"');
    expect(served.runs).toHaveLength(1);
    // The card said it: no notice over the page says it again.
    await expect(page.locator('[data-sonner-toast]')).toHaveCount(0);
  });

  test('a run that fails says why in the test card, and nowhere else', async ({ page }) => {
    await serveFunctions(page, [{ ...OWN, source: 'fail' }]);
    await page.goto(`/functions/${OWN.id}`);

    await expect(page.locator('[data-page-title-row]')).toContainText('Edit function · mask-cards');
    await page.locator('[data-function-run]').click();

    await expect(page.locator('[data-function-run-error]')).toContainText('undefined variable `fail`');
    await expect(page.locator("[data-function-source] [data-function-status='invalid']")).toHaveText('Validation failed');
    await expect(page.locator('[data-function-validation-detail]')).toHaveCount(0);
    await expect(page.locator('[data-sonner-toast]')).toHaveCount(0);
  });

  test('a function of the workspace can be deleted from the title row', async ({ page }) => {
    await serveFunctions(page, [OWN]);
    await page.goto(`/functions/${OWN.id}`);

    await page.locator('[data-page-title-row] [data-function-delete]').click();

    await expect(page.getByRole('alertdialog').or(page.getByRole('dialog'))).toContainText('Delete this function?');
  });
});
