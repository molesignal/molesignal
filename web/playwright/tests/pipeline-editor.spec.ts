import type { Locator, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface PipelineBody {
  name: string;
  source_stream: string;
  target_stream: string;
  cron: string;
  lookback_secs?: number;
  enabled?: boolean;
  function_steps: {
    mode?: string;
    sources?: string[];
    sinks?: string[];
    sink_connectors?: string[];
    retain_source?: boolean;
    steps: Array<Record<string, unknown>>;
  };
}

interface TestRun {
  source: string;
  input: Record<string, unknown>;
}

const stream = (name: string, type: 'logs' | 'metrics') => ({
  id: `${type}/${name}`,
  name,
  stream_type: type,
  schema: { fields: [] },
  created_at_micros: 1,
  updated_at_micros: 1,
});

// What the backend answers for a script that does not compile, as it words it.
const COMPILE_ERROR =
  'invalid argument: vrl compile: DiagnosticList([Diagnostic { severity: Error, code: 103, message: "unhandled fallible assignment", labels: [Label { message: "this expression is fallible", primary: true, span: Span { start: 5, end: 19 } }], notes: [] }])';

/**
 * The workspace a pipeline is built in: three log streams, a metric stream and
 * a connector. Saves and test runs are kept so a test can read what was sent.
 */
async function mountPipelines(page: Page) {
  const saved: PipelineBody[] = [];
  const runs: TestRun[] = [];

  await page.route('**/api/v1/streams', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: [
            stream('app_logs', 'logs'),
            stream('audit_logs', 'logs'),
            stream('clean_logs', 'logs'),
            stream('cpu', 'metrics'),
          ],
        })
      : route.fallback(),
  );
  await page.route('**/api/v1/connectors', (route) =>
    route.fulfill({
      json: [
        {
          id: 'c1',
          name: 'Archive',
          kind: 's3',
          config_json: {},
          enabled: true,
          created_at_micros: 1,
          updated_at_micros: 1,
        },
      ],
    }),
  );
  await page.route('**/api/v1/functions', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/v1/functions/run', (route) => {
    const { source, input } = route.request().postDataJSON() as TestRun;
    runs.push({ source, input });

    if (source.includes('to_int(.level)')) {
      return route.fulfill({
        status: 400,
        json: { error: 'invalid_argument', message: COMPILE_ERROR },
      });
    }
    // Stands in for the runtime: each script leaves its mark on the event.
    return route.fulfill({ json: { output: { ...input, ran: [...((input.ran as string[]) ?? []), source.length] } } });
  });
  await page.route('**/api/v1/scheduled_pipelines', (route) => {
    if (route.request().method() !== 'POST') return route.fallback();

    saved.push(route.request().postDataJSON() as PipelineBody);
    return route.fulfill({ json: { id: 'p-1', ...saved.at(-1) } });
  });
  await page.route('**/api/v1/scheduled_pipelines/p-1', (route) => {
    const last = saved.at(-1);

    if (route.request().method() === 'PUT') {
      saved.push(route.request().postDataJSON() as PipelineBody);
      return route.fulfill({ json: { id: 'p-1', ...saved.at(-1) } });
    }
    return last
      ? route.fulfill({ json: { id: 'p-1', ...last } })
      : route.fulfill({ status: 404, json: { error: 'not_found', message: 'not found' } });
  });
  await page.route('**/api/v1/scheduled_pipelines/p-1/runs**', (route) => route.fulfill({ json: [] }));

  return { saved, runs };
}

const flowCard = (page: Page) => page.locator('[data-pipeline-flow-card]');
const node = (page: Page, id: string) => page.locator(`[data-pipeline-node="${id}"]`);
const nodeEditor = (page: Page) => page.locator('[data-pipeline-node-editor]');
const testPanel = (page: Page) => page.locator('[data-pipeline-test]');
const testResult = (page: Page) => page.locator('[data-pipeline-test-result]');
const flowStatus = (page: Page) => page.locator('[data-flow-status]');
const nameField = (page: Page) => page.getByRole('textbox', { name: /^(Name|名称)$/ });
const save = (page: Page) => page.getByRole('button', { name: 'Create pipeline' });
const streamPicker = (page: Page) => nodeEditor(page).locator('[data-stream-combobox]');

async function addStep(page: Page, type: string) {
  await page.getByRole('button', { name: 'Add step' }).click();
  await page.getByRole('menuitem', { name: type, exact: true }).click();
}

/** Picks one of the workspace's streams for the node that is open. */
async function pickStream(page: Page, name: string) {
  await streamPicker(page).click();
  await page.getByRole('option', { name, exact: true }).click();
}

/** Names a stream the workspace does not have yet for the node that is open. */
async function nameStream(page: Page, name: string) {
  await streamPicker(page).click();
  await page.getByPlaceholder('Search, or type a new name…').fill(name);
  await page.getByRole('option', { name: `Use new stream "${name}"` }).click();
}

/** A new pipeline with the two things it has to be told: reads app_logs, writes app_logs_clean. */
async function startPipeline(page: Page, name: string) {
  await page.goto('/pipelines/new');
  await nameField(page).fill(name);
  await pickStream(page, 'app_logs');
  await node(page, 'sink-0').click();
  await nameStream(page, 'app_logs_clean');
}

async function box(locator: Locator) {
  const rect = await locator.boundingBox();
  if (!rect) throw new Error('not laid out');

  return {
    left: Math.round(rect.x),
    right: Math.round(rect.x + rect.width),
    top: Math.round(rect.y),
    bottom: Math.round(rect.y + rect.height),
  };
}

test.describe('Pipeline editor', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is one page: what it is, the flow, the picked node beside a test run', async ({ page }) => {
    await mountPipelines(page);
    await page.goto('/pipelines/new');
    await expect(flowCard(page)).toBeVisible();

    // The title row carries the page's two actions and nothing else.
    const header = page.getByTestId('page-header');
    await expect(header.getByRole('heading', { level: 1 })).toHaveText('New pipeline');
    await expect(header.getByRole('button')).toHaveText(['Cancel', 'Create pipeline']);

    // A new pipeline is a source and a destination to choose, with the source open. Nothing is
    // made up for it, and nothing is called a mistake before a save is tried.
    await expect(page.locator('[data-pipeline-node]')).toHaveText([
      'Choose a streamStream input',
      'Choose a streamStream output',
    ]);
    await expect(page.locator('[data-flow-issue]')).toHaveCount(0);
    await expect(node(page, 'source-0')).toHaveAttribute('aria-pressed', 'true');
    await expect(flowCard(page).getByRole('group', { name: 'Processing' })).toContainText(
      'No steps: events are written as they are read.',
    );
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'pending');
    await expect(flowStatus(page)).toHaveText('2 left to fill in');
    await expect(page.locator('[aria-invalid="true"]')).toHaveCount(0);

    // A pipeline reads one stream and writes one: neither lane grows, and no control promises
    // what nothing does.
    await expect(page.getByRole('button', { name: /Add source|Add destination/ })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Add connector' })).toBeVisible();
    await expect(page.locator('[data-pipeline-settings]')).not.toContainText(/retry/i);

    // The cards stand on one pair of edges; the node and the test run share a row.
    const [settings, flow, editor, panel] = await Promise.all([
      box(page.locator('[data-pipeline-settings]')),
      box(flowCard(page)),
      box(nodeEditor(page)),
      box(testPanel(page)),
    ]);
    expect([flow.left, editor.left]).toEqual([settings.left, settings.left]);
    expect([flow.right, panel.right]).toEqual([settings.right, settings.right]);
    expect(panel.top).toBe(editor.top);
    expect(panel.left - editor.right).toBe(12);
    expect(flow.top - settings.bottom).toBe(12);
    expect(editor.top - flow.bottom).toBe(12);
  });

  test('reads a stream the workspace has, and may write to one it does not have yet', async ({
    page,
  }) => {
    const backend = await mountPipelines(page);
    await page.goto('/pipelines/new');
    await nameField(page).fill('copy-logs');

    // The source is one of the log streams; a metric stream is not on offer, nor a new name.
    await streamPicker(page).click();
    await expect(page.getByRole('option')).toHaveText(['app_logs', 'audit_logs', 'clean_logs']);
    await page.getByPlaceholder('Search streams…').fill('brand_new');
    await expect(page.getByRole('option')).toHaveCount(0);
    await page.getByPlaceholder('Search streams…').fill('app');
    await page.getByRole('option', { name: 'app_logs' }).click();
    await expect(node(page, 'source-0')).toContainText('app_logs');

    await node(page, 'sink-0').click();
    await nameStream(page, 'app_logs_clean');
    await expect(node(page, 'sink-0')).toContainText('app_logs_clean');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    // With no steps it is a copy: the test run gives the event back as it went in.
    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toContainText('This pipeline has no steps');
    await expect(testResult(page)).toContainText('Written to app_logs_clean');
    expect(backend.runs).toHaveLength(0);

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]).toMatchObject({
      name: 'copy-logs',
      source_stream: 'app_logs',
      target_stream: 'app_logs_clean',
      cron: 'every:5m',
      lookback_secs: 300,
      enabled: true,
    });
    expect(backend.saved[0]!.function_steps).toMatchObject({
      sources: ['app_logs'],
      sinks: ['app_logs_clean'],
      sink_connectors: [],
      steps: [],
    });
  });

  test('builds a chain of processors, tries it on a sample, and saves it', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'checkout-logs');

    await addStep(page, 'Parse JSON');
    await addStep(page, 'Mask field');
    await nodeEditor(page).getByRole('textbox', { name: /^Field/ }).fill('user.email');
    await nodeEditor(page).getByRole('button', { name: 'SHA-256 hash' }).click();
    await expect(page.locator('[data-pipeline-node^="transform-"]')).toHaveText([
      /^1parse-jsonParse JSON · message$/,
      /^2maskMask field · user\.email$/,
    ]);

    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'passed');
    await expect(testResult(page).locator('[data-test-step]')).toHaveText(['1parse-json', '2mask']);
    await expect(testResult(page)).toContainText('Written to app_logs_clean');

    // Each step was given what the one before left, as the generated VRL.
    expect(backend.runs).toHaveLength(2);
    expect(backend.runs[0]!.source).toContain('parse_json');
    expect(backend.runs[0]!.input).toMatchObject({ appname: 'checkout' });
    expect(backend.runs[1]!.source).toContain('sha2');
    expect(backend.runs[1]!.input.ran).toHaveLength(1);

    // The event as the first step left it is one click away.
    await testResult(page).getByRole('button', { name: /parse-json$/ }).click();
    await expect(testResult(page)).toContainText('After step 1');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);

    const [body] = backend.saved;
    expect(body!.function_steps.steps).toMatchObject([
      {
        transform_name: 'parse-json',
        kind: 'vrl',
        processor: { op: 'parse_json', source: 'message', target: '', removeSource: false },
      },
      {
        transform_name: 'mask',
        kind: 'vrl',
        processor: { op: 'mask', field: 'user.email', mode: 'hash' },
      },
    ]);
    expect(body!.function_steps.steps.map((step) => step.script)).toEqual([
      backend.runs[0]!.source,
      backend.runs[1]!.source,
    ]);

    // Saved, it opens again as the processors it was built from, not as their VRL.
    await expect(page.getByTestId('page-header').getByRole('heading', { level: 1 })).toHaveText(
      'Edit pipeline · checkout-logs',
    );
    await node(page, 'transform-1').click();
    await expect(nodeEditor(page).getByRole('combobox', { name: 'Processing type' })).toHaveText('Mask field');
    await expect(nodeEditor(page).getByRole('textbox', { name: /^Field/ })).toHaveValue('user.email');
    await expect(nodeEditor(page).getByRole('button', { name: 'SHA-256 hash' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  test('holds the save back until everything is filled in, and leads to what is missing', async ({
    page,
  }) => {
    const backend = await mountPipelines(page);
    await page.goto('/pipelines/new');
    await nameField(page).fill('tidy');
    await addStep(page, 'Rename field');

    // Left to fill in: marked quietly, not as errors.
    await expect(flowStatus(page)).toHaveText('3 left to fill in');
    await expect(node(page, 'transform-0').locator('[data-flow-node-issue]')).toHaveAttribute(
      'data-flow-node-issue',
      'pending',
    );
    await save(page).click();

    const issues = flowCard(page).getByRole('alert');
    await expect(issues).toContainText('Fix these before saving');
    await expect(issues.locator('[data-flow-issue="error"]')).toHaveText([
      'Choose the stream to read.',
      'Choose the stream to write to.',
      'Step "rename" still has an empty field.',
    ]);
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'error');
    await expect(node(page, 'transform-0')).toHaveAttribute('aria-invalid', 'true');
    await expect(page).toHaveURL(/\/pipelines\/new$/);
    expect(backend.saved).toHaveLength(0);

    await issues.getByRole('button', { name: 'Choose the stream to read.' }).click();
    await pickStream(page, 'app_logs');
    await issues.getByRole('button', { name: 'Choose the stream to write to.' }).click();
    await pickStream(page, 'clean_logs');
    await issues.getByRole('button', { name: 'Step "rename" still has an empty field.' }).click();
    await expect(node(page, 'transform-0')).toHaveAttribute('aria-pressed', 'true');
    await nodeEditor(page).getByRole('textbox', { name: /^Field/ }).fill('msg');
    await nodeEditor(page).getByRole('textbox', { name: 'New name' }).fill('message');

    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');
    await expect(flowCard(page).getByRole('alert')).toHaveCount(0);
    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved).toHaveLength(1);
  });

  test('names the stream a route sends the event to, and stops at a step that fails', async ({
    page,
  }) => {
    await mountPipelines(page);
    await startPipeline(page, 'by-app');

    await addStep(page, 'Custom VRL');
    await addStep(page, 'Route to stream');
    await nodeEditor(page).getByRole('textbox', { name: /^Field name/ }).fill('appname');
    await nodeEditor(page).getByRole('textbox', { name: /^Stream prefix/ }).fill('app_');
    // The route decides the stream, so the destination stands for whatever it picks.
    await expect(node(page, 'sink-0')).toContainText('Chosen by the routing step');

    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page).locator('[data-test-step]').last()).toContainText('→ app_checkout');
    await expect(testResult(page)).toContainText('Written to app_checkout');

    // Routing back into the stream that is read would fail every run: held against the step.
    await nodeEditor(page).getByRole('textbox', { name: /^Fallback stream/ }).fill('app_logs');
    await expect(flowCard(page).locator('[data-flow-issue="error"]')).toHaveText([
      'Step "route" would route events back into app_logs, the stream this pipeline reads. Every run would fail.',
    ]);
    await nodeEditor(page).getByRole('textbox', { name: /^Fallback stream/ }).fill('default');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    // A script that does not compile: said in words, and nothing after it runs.
    await node(page, 'transform-0').click();
    await page.getByLabel('VRL script').click({ force: true });
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.insertText('.x = to_int(.level)');
    await expect(testResult(page)).toHaveAttribute('data-stale', 'true');

    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'failed');
    await expect(testResult(page).locator('[data-test-step]')).toHaveText([
      /Not valid VRL: unhandled fallible assignment: this expression is fallible$/,
      /not run$/,
    ]);
    await expect(testResult(page).locator('[data-pipeline-test-output]')).toHaveCount(0);
  });

  test('delivers to connectors beside the stream', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'archive-logs');

    await page.getByRole('button', { name: 'Add connector' }).click();
    await expect(node(page, 'sink-1')).toContainText('Choose a connector');
    await expect(flowStatus(page)).toHaveText('1 left to fill in');
    await expect(node(page, 'sink-1').locator('[data-flow-node-issue]')).toHaveAttribute(
      'data-flow-node-issue',
      'pending',
    );

    await nodeEditor(page).getByRole('combobox', { name: /^Connector/ }).click();
    await page.getByRole('option', { name: 'Archive (s3)' }).click();
    await expect(node(page, 'sink-1')).toHaveText('ArchiveConnector delivery');

    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toContainText('Written to app_logs_clean, Archive');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]!.function_steps).toMatchObject({
      sinks: ['app_logs_clean'],
      sink_connectors: ['c1'],
    });
  });

  test('works on logs as they arrive in realtime mode', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'route-logs');

    const settings = page.locator('[data-pipeline-settings]');
    await expect(settings.getByText('Run every')).toBeVisible();
    await settings.getByRole('button', { name: 'Realtime', exact: true }).click();

    // No schedule to set, one stream in and one out, logs only.
    await expect(settings.getByText('Run every')).toHaveCount(0);
    await expect(settings.getByRole('combobox', { name: 'Signal type' })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Add connector' })).toHaveCount(0);

    await node(page, 'source-0').click();
    await nodeEditor(page).getByRole('checkbox', { name: 'Also retain in source stream' }).click();
    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);

    expect(backend.saved[0]).toMatchObject({
      name: 'route-logs',
      cron: '',
      source_stream: 'app_logs',
      target_stream: 'app_logs_clean',
    });
    expect(backend.saved[0]!.function_steps).toMatchObject({ mode: 'realtime', retain_source: true });
  });

  test('keeps a long chain inside its lane and the picked step in view', async ({ page }) => {
    await mountPipelines(page);
    await page.goto('/pipelines/new');
    const types = ['Parse JSON', 'Rename field', 'Remove fields', 'Set field', 'Mask field', 'Change case', 'Convert type', 'Custom VRL'];
    for (const type of types) await addStep(page, type);

    await expect(page.locator('[data-pipeline-node^="transform-"]')).toHaveCount(8);
    await expect(node(page, 'transform-7')).toHaveAttribute('aria-pressed', 'true');

    // The page keeps its width; the chain scrolls inside the lane, whose near edge fades to say
    // there are steps before it.
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    const lane = page.getByRole('group', { name: 'Processing' });
    expect(await lane.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);
    const mask = () => lane.evaluate((element) => getComputedStyle(element).maskImage);
    await expect.poll(mask).toContain('rgb(0, 0, 0) 28px');
    // The last step is in clear view: nothing comes after it, so the far edge does not fade.
    expect(await mask()).not.toContain('100% - 28px');

    const [laneBox, last, destination] = await Promise.all([
      box(lane),
      box(node(page, 'transform-7')),
      box(node(page, 'sink-0')),
    ]);
    expect(last.left).toBeGreaterThanOrEqual(laneBox.left);
    expect(last.right).toBeLessThanOrEqual(laneBox.right);
    // The destination stays where it is, at the end of the row.
    expect(destination.left).toBeGreaterThan(laneBox.right);

    // After a save is held back, an issue leads to its step, wherever the lane was scrolled to.
    await nameField(page).fill('long-chain');
    await save(page).click();
    await page.getByRole('button', { name: 'Step "rename" still has an empty field.' }).click();
    await expect(node(page, 'transform-1')).toBeInViewport();
    await expect(node(page, 'transform-1')).toHaveAttribute('aria-pressed', 'true');
  });

  test('shows a saved pipeline as the same flow, and says what in it nothing reads or writes', async ({
    page,
  }) => {
    const backend = await mountPipelines(page);
    backend.saved.push({
      name: 'archive-logs',
      source_stream: 'app_logs',
      target_stream: 'clean_logs',
      cron: 'every:15m',
      lookback_secs: 900,
      enabled: true,
      function_steps: {
        // As an earlier editor could save it: two sources, though only the first was ever read.
        sources: ['app_logs', 'audit_logs'],
        sinks: ['clean_logs'],
        sink_connectors: ['c1'],
        steps: [{ transform_name: 'keep', kind: 'vrl', script: '. = .' }],
      },
    });
    await page.goto('/pipelines/p-1');

    const flow = page.locator('[data-pipeline-flow]').first();
    await expect(flow.getByRole('group', { name: 'Source' }).locator('[data-pipeline-node]')).toHaveText([
      'app_logsStream input',
      'audit_logsSaved, never read',
    ]);
    // The connector goes by its name.
    await expect(flow.getByRole('group', { name: 'Destinations' }).locator('[data-pipeline-node]')).toHaveText([
      'clean_logsStream output',
      'ArchiveConnector delivery',
    ]);
    await expect(flow.getByRole('button')).toHaveCount(0);

    // The editor says it too, and lets the name go.
    await page.goto('/pipelines/p-1/edit');
    await expect(flowStatus(page)).toHaveText('1 note');
    await page.getByRole('button', { name: /audit_logs is never read/ }).click();
    await nodeEditor(page).getByRole('button', { name: 'Remove' }).click();
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');
  });

  test('reads in Chinese when the interface does', async ({ page }) => {
    await mountPipelines(page);
    await page.route('**/api/v1/me/preferences', (route) =>
      route.request().method() === 'GET'
        ? route.fulfill({
            json: {
              theme: 'system',
              density: 'normal',
              language: 'zh-cn',
              default_home_route: '/home',
              time_format: 'iso_24h',
              date_format: 'yyyy_mm_dd_dash',
              timezone: '',
              keyboard_shortcuts_enabled: true,
            },
          })
        : route.fallback(),
    );
    await page.goto('/pipelines/new');

    await expect(page.getByTestId('page-header').getByRole('heading', { level: 1 })).toHaveText('新建流水线');
    await expect(flowStatus(page)).toHaveText('还有 2 处待填写');
    await page.getByRole('button', { name: '添加步骤' }).click();
    await page.getByRole('menuitem', { name: '字段脱敏', exact: true }).click();
    await expect(node(page, 'transform-0')).toContainText('字段脱敏');
    // What is left to fill in is listed once a save is tried.
    await nameField(page).fill('脱敏');
    await page.getByRole('button', { name: '创建流水线' }).click();
    await expect(flowCard(page).locator('[data-flow-issue]')).toHaveText([
      '请选择要读取的数据流。',
      '请选择要写入的数据流。',
      '步骤「mask」还有未填写的字段。',
    ]);

    // Nothing is left as a key, and nothing but names and code is left in English.
    const text = await page.locator('#main').innerText();
    expect(text).not.toMatch(/\b(?:settings|flow|steps|editor|processors|test|graph|streams)\.[a-z_.]+\b/);
    expect(text).not.toMatch(/Add step|Run test|Sample event|Remove|Step \d|Choose a stream/);
  });
});
