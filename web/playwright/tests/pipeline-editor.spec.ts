import type { Locator, Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

interface PipelineBody {
  name: string;
  sourceStream: string;
  targetStream: string;
  cron: string;
  lookbackSecs?: number;
  enabled?: boolean;
  functionSteps: {
    mode?: string;
    sources?: string[];
    sinks?: string[];
    sinkConnectors?: string[];
    sink_tables?: string[];
    alert?: Record<string, unknown>;
    retain_source?: boolean;
    delay_secs?: number;
    retry?: { run_retries: number; delivery_retries: number };
    dead_letter_stream?: string;
    dedupe_field?: string;
    steps: Array<Record<string, unknown>>;
  };
}

interface TestRun {
  source: string;
  input: Record<string, unknown>;
  metadata?: Record<string, unknown> | undefined;
}

const stream = (name: string, type: 'logs' | 'metrics') => ({
  id: `${type}/${name}`,
  name,
  streamType: type,
  schema: { fields: [] },
  createdAt: 1,
  updatedAt: 1,
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
          configJson: {},
          enabled: true,
          createdAt: 1,
          updatedAt: 1,
        },
      ],
    }),
  );
  await page.route('**/api/v1/extend_tables', (route) =>
    route.fulfill({
      json: [
        {
          tableName: 'host_owner',
          description: '',
          keyField: 'host',
          valueFields: [],
          rowCount: 2,
          updatedAt: 1,
          usageLocations: [],
        },
      ],
    }),
  );
  await page.route('**/api/v1/functions', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/v1/functions/run', (route) => {
    const { source, input, metadata } = route.request().postDataJSON() as TestRun;
    runs.push({ source, input, metadata });

    if (source.includes('to_int(.level)')) {
      return route.fulfill({
        status: 400,
        json: { error: 'invalid_argument', message: COMPILE_ERROR },
      });
    }
    // A filter that matches the sample's level aborts it, as the runtime does.
    if (source.includes('abort') && source.includes(`"${String(input.level)}"`)) {
      return route.fulfill({ json: { output: input, dropped: true } });
    }
    // Stands in for the runtime: each script leaves its mark on the event.
    return route.fulfill({
      json: { output: { ...input, ran: [...((input.ran as string[]) ?? []), source.length] }, dropped: false },
    });
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

/**
 * Chooses from one of the flow's menus. The menu hands the focus back to its
 * button a moment after it closes, which would close a popover opened in that
 * moment, so this waits for it.
 */
async function choose(page: Page, menu: string, item: string) {
  const trigger = page.getByRole('button', { name: menu });
  await trigger.click();
  await page.getByRole('menuitem', { name: item, exact: true }).click();
  await expect(trigger).toBeFocused();
}

async function addStep(page: Page, type: string) {
  await choose(page, 'Add step', type);
}

async function addDestination(page: Page, kind: 'Stream' | 'Connector' | 'Extend table') {
  await choose(page, 'Add destination', kind);
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

    // Either side takes more streams, and the destinations take connectors too.
    await expect(page.getByRole('button', { name: 'Add source' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Add destination' })).toBeVisible();
    // How a run reads and what it does when something fails is set beside the schedule.
    const settingsCard = page.locator('[data-pipeline-settings]');
    await expect(settingsCard.getByRole('combobox', { name: 'Wait for late events' })).toHaveText('1 min');
    await expect(settingsCard.getByRole('combobox', { name: 'Retry a failed run' })).toHaveText('3 times');
    await expect(settingsCard.getByRole('combobox', { name: 'Retry a failed delivery' })).toHaveText('3 times');

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
      sourceStream: 'app_logs',
      targetStream: 'app_logs_clean',
      cron: 'every:5m',
      lookbackSecs: 300,
      enabled: true,
    });
    expect(backend.saved[0]!.functionSteps).toMatchObject({
      sources: ['app_logs'],
      sinks: ['app_logs_clean'],
      sinkConnectors: [],
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
    expect(body!.functionSteps.steps).toMatchObject([
      {
        transformName: 'parse-json',
        kind: 'vrl',
        processor: { op: 'parse_json', source: 'message', target: '', removeSource: false },
      },
      {
        transformName: 'mask',
        kind: 'vrl',
        processor: { op: 'mask', field: 'user.email', mode: 'hash' },
      },
    ]);
    expect(body!.functionSteps.steps.map((step) => step.script)).toEqual([
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

    // Routing back into a stream that is read would feed the pipeline its own output.
    await nodeEditor(page).getByRole('textbox', { name: /^Fallback stream/ }).fill('app_logs');
    await expect(flowCard(page).locator('[data-flow-issue="error"]')).toHaveText([
      'Step "route" would route events back into app_logs, a stream this pipeline reads. Choose another stream.',
    ]);
    await nodeEditor(page).getByRole('textbox', { name: /^Fallback stream/ }).fill('default');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    // A script that does not compile: said in words, and nothing after it runs.
    await node(page, 'transform-0').click();
    // Monaco's native input target can sit outside the visible editor. Click the
    // rendered text surface and wait for focus before sending keyboard input.
    const vrlEditor = nodeEditor(page).locator('.monaco-editor');
    await vrlEditor.locator('.view-lines').click();
    await expect(page.getByRole('textbox', { name: 'VRL script', exact: true })).toBeFocused();
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

    await addDestination(page, 'Connector');
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
    expect(backend.saved[0]!.functionSteps).toMatchObject({
      sinks: ['app_logs_clean'],
      sinkConnectors: ['c1'],
    });
  });

  test('works on logs as they arrive in realtime mode', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'route-logs');

    const settings = page.locator('[data-pipeline-settings]');
    await expect(settings.getByText('Run every')).toBeVisible();
    await settings.getByRole('button', { name: 'Realtime', exact: true }).click();

    // No schedule to set and nothing to retry, one stream in and one out.
    await expect(settings.getByText('Run every')).toHaveCount(0);
    await expect(settings.getByRole('combobox', { name: 'Retry a failed run' })).toHaveCount(0);
    await expect(settings.getByRole('combobox', { name: 'Signal type' })).toBeEnabled();
    await expect(page.getByRole('button', { name: /^Add (source|destination)$/ })).toHaveCount(0);

    await node(page, 'source-0').click();
    await nodeEditor(page).getByRole('checkbox', { name: 'Also retain in source stream' }).click();
    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);

    expect(backend.saved[0]).toMatchObject({
      name: 'route-logs',
      cron: '',
      sourceStream: 'app_logs',
      targetStream: 'app_logs_clean',
    });
    expect(backend.saved[0]!.functionSteps).toMatchObject({ mode: 'realtime', retainSource: true });
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

  test('shows a saved pipeline as the same flow: every source, every stream, every connector', async ({
    page,
  }) => {
    const backend = await mountPipelines(page);
    backend.saved.push({
      name: 'archive-logs',
      sourceStream: 'app_logs',
      targetStream: 'clean_logs',
      cron: 'every:15m',
      lookbackSecs: 900,
      enabled: true,
      functionSteps: {
        sources: ['app_logs', 'audit_logs'],
        sinks: ['clean_logs', 'archive_logs'],
        sinkConnectors: ['c1'],
        steps: [{ transformName: 'keep', kind: 'vrl', script: '. = .' }],
      },
    });
    await page.goto('/pipelines/p-1');

    const flow = page.locator('[data-pipeline-flow]').first();
    await expect(flow.getByRole('group', { name: 'Source' }).locator('[data-pipeline-node]')).toHaveText([
      'app_logsStream input',
      'audit_logsStream input',
    ]);
    // The connector goes by its name.
    await expect(flow.getByRole('group', { name: 'Destinations' }).locator('[data-pipeline-node]')).toHaveText([
      'clean_logsStream output',
      'archive_logsStream output',
      'ArchiveConnector delivery',
    ]);
    await expect(flow.getByRole('button')).toHaveCount(0);

    // The editor opens it as it is, with nothing to say about it.
    await page.goto('/pipelines/p-1/edit');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');
  });

  test('reads several streams and writes several, every one of them taking every event', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'fan');

    await page.getByRole('button', { name: 'Add source' }).click();
    await expect(node(page, 'source-1')).toHaveAttribute('aria-pressed', 'true');
    await pickStream(page, 'audit_logs');
    await addDestination(page, 'Stream');
    await expect(node(page, 'sink-1')).toHaveAttribute('aria-pressed', 'true');
    await nameStream(page, 'logs_archive');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    // The sample is said to come from one of the sources, which the steps see as %source_stream.
    await addStep(page, 'Custom VRL');
    await testPanel(page).getByRole('combobox', { name: 'Sample from' }).click();
    await page.getByRole('option', { name: 'From audit_logs' }).click();
    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toContainText('Written to app_logs_clean, logs_archive');
    expect(backend.runs.at(-1)!.metadata).toEqual({ source_stream: 'audit_logs' });

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]).toMatchObject({ sourceStream: 'app_logs', targetStream: 'app_logs_clean' });
    expect(backend.saved[0]!.functionSteps).toMatchObject({
      sources: ['app_logs', 'audit_logs'],
      sinks: ['app_logs_clean', 'logs_archive'],
    });
  });

  test('drops what a filter says, in the test run as when the pipeline runs', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'quiet');

    await addStep(page, 'Filter events');
    await nodeEditor(page).getByRole('button', { name: 'Drop', exact: true }).click();
    await nodeEditor(page).getByRole('textbox', { name: /^Field/ }).fill('level');
    await nodeEditor(page).getByRole('textbox', { name: 'Value', exact: true }).fill('WARN');
    await expect(node(page, 'transform-0')).toHaveText(/Filter events · drop level = WARN$/);
    await addStep(page, 'Custom VRL');

    // The sample's level is WARN: dropped at the filter, so the step after it never runs.
    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'dropped');
    await expect(testResult(page).locator('[data-test-step]')).toHaveText([/dropped$/, /not run$/]);
    await expect(testResult(page)).toContainText('Dropped by step 1');
    await expect(testResult(page)).not.toContainText('Written to');
    expect(backend.runs).toHaveLength(1);

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]!.functionSteps.steps[0]).toMatchObject({
      transformName: 'filter',
      processor: { op: 'filter', action: 'drop', field: 'level', test: 'equals', value: 'WARN' },
    });
    expect(String(backend.saved[0]!.functionSteps.steps[0]!.script)).toContain('abort');
  });

  test('saves how a run waits for late events, retries, keeps dead letters and dedupes', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'careful');

    const settings = page.locator('[data-pipeline-settings]');
    await settings.getByRole('combobox', { name: 'Wait for late events' }).click();
    await page.getByRole('option', { name: '5 min' }).click();
    await settings.getByRole('combobox', { name: 'Retry a failed run' }).click();
    await page.getByRole('option', { name: 'No retry' }).click();
    await settings.locator('[data-stream-combobox]').click();
    await page.getByPlaceholder('Search, or type a new name…').fill('failed_events');
    await page.getByRole('option', { name: 'Use new stream "failed_events"' }).click();
    await settings.getByRole('textbox', { name: 'Dedupe by field' }).fill('trace_id');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]!.functionSteps).toMatchObject({
      delaySecs: 300,
      retry: { runRetries: 0, deliveryRetries: 3 },
      deadLetterStream: 'failed_events',
      dedupeField: 'trace_id',
    });

    // Saved, it opens with them; a dead letter stream can be let go again.
    await expect(settings.locator('[data-stream-combobox]')).toHaveText('failed_events');
    await settings.locator('[data-stream-combobox]').click();
    await page.getByRole('option', { name: 'Keep no failed events' }).click();
    await expect(settings.locator('[data-stream-combobox]')).toHaveText('Keep no failed events');
  });

  test('works on alert events before their incidents open', async ({ page }) => {
    const backend = await mountPipelines(page);
    await page.goto('/pipelines/new');
    await nameField(page).fill('prod alerts');

    const settings = page.locator('[data-pipeline-settings]');
    await settings.getByRole('button', { name: 'Alert events', exact: true }).click();

    // Alert events come in and go back into the incident: no stream, schedule or retry to set.
    await expect(settings.getByText('Run every')).toHaveCount(0);
    await expect(settings.getByRole('combobox', { name: 'Retry a failed run' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: /^Add (source|destination)$/ })).toHaveCount(0);
    await expect(node(page, 'source-0')).toContainText('Every alert event');
    await expect(node(page, 'sink-0')).toContainText('Back into the incident');
    await expect(flowCard(page).locator('[data-flow-no-steps]')).toHaveText(
      'No steps: the incident opens as the alert event came.',
    );

    await node(page, 'source-0').click();
    await nodeEditor(page).getByRole('checkbox', { name: 'Alert rules' }).click();
    await nodeEditor(page).getByRole('checkbox', { name: 'Critical' }).click();
    await nodeEditor(page).getByRole('button', { name: 'Add a label condition' }).click();
    await nodeEditor(page).getByRole('textbox', { name: 'Label', exact: true }).fill('env');
    await nodeEditor(page).getByRole('textbox', { name: 'Value', exact: true }).fill('prod');
    await expect(node(page, 'source-0')).toContainText('Alert rules · Critical · env=prod');

    await addStep(page, 'Set field');
    await nodeEditor(page).getByRole('textbox', { name: /^Field/ }).fill('labels.team');
    await nodeEditor(page).getByRole('textbox', { name: 'Value', exact: true }).fill('payments');
    await addStep(page, 'AI summary');
    await expect(nodeEditor(page).getByRole('textbox', { name: 'Prompt' })).not.toBeEmpty();

    // The steps read where the sample came from; the model is asked only about a real event.
    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'passed');
    await expect(testResult(page)).toContainText('Back into the incident');
    expect(backend.runs).toHaveLength(1);
    expect(backend.runs[0]!.metadata).toEqual({
      rule_id: 'rule-1',
      rule_name: 'Checkout error rate',
      source: 'rule',
      event: 'opened',
    });
    // A sample nobody edited follows the mode: an alert event, not the log it started as.
    expect(backend.runs[0]!.input).toMatchObject({ summary: 'Checkout error rate above 5%', severity: 'error' });

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]).toMatchObject({ name: 'prod alerts', sourceStream: '', targetStream: '', cron: '' });
    expect(backend.saved[0]!.functionSteps).toMatchObject({
      mode: 'alert',
      alert: { sources: ['rule'], severities: ['critical'], labels: { env: 'prod' }, order: 0 },
    });
    expect(backend.saved[0]!.functionSteps.steps.map((step) => step.kind)).toEqual(['vrl', 'ai_summary']);
  });

  test('renames labels by a pattern and keeps only some, without writing VRL', async ({ page }) => {
    const backend = await mountPipelines(page);
    await page.goto('/pipelines/new');
    await nameField(page).fill('tidy labels');
    await page
      .locator('[data-pipeline-settings]')
      .getByRole('button', { name: 'Alert events', exact: true })
      .click();

    await addStep(page, 'Rename by pattern');
    // In an alert event these go through its labels unless told otherwise.
    await expect(nodeEditor(page).getByRole('textbox', { name: /^In/ })).toHaveValue('labels');
    await nodeEditor(page).getByRole('textbox', { name: /^Pattern/ }).fill('^kubernetes_(.+)$');
    await nodeEditor(page).getByRole('textbox', { name: /^Replace with/ }).fill('$1');
    await expect(node(page, 'transform-0')).toContainText('^kubernetes_(.+)$ → $1');

    await addStep(page, 'Keep only these fields');
    await nodeEditor(page).getByRole('textbox', { name: /^Keys to keep/ }).fill('service, env');
    await expect(node(page, 'transform-1')).toContainText('labels: service, env');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'passed');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);

    const steps = backend.saved[0]!.functionSteps.steps;

    expect(steps[0]).toMatchObject({
      kind: 'vrl',
      script:
        "if is_object(.labels) {\n  .labels = map_keys(object!(.labels)) -> |key| { replace(key, r'^kubernetes_(.+)$', \"$1\") }\n}",
      processor: { op: 'rename_regex', within: 'labels' },
    });
    expect(steps[1]).toMatchObject({
      kind: 'vrl',
      script:
        'if is_object(.labels) {\n  .labels = filter(object!(.labels)) -> |key, _value| { includes(["service", "env"], key) }\n}',
      processor: { op: 'keep', within: 'labels', fields: ['service', 'env'] },
    });
  });

  test('runs before a notification, on a copy, for the policies it names', async ({ page }) => {
    const backend = await mountPipelines(page);
    await page.route('**/api/v1/notify/policies', (route) =>
      route.fulfill({
        json: [
          { id: 'policy-oncall', name: 'On-call alerts', eventType: 'alert.triggered' },
          { id: 'policy-resolved', name: 'Recovered', eventType: 'alert.resolved' },
          { id: 'policy-report', name: 'Weekly report', eventType: 'report.ready' },
        ],
      }),
    );
    await page.goto('/pipelines/new');
    await nameField(page).fill('quiet at night');

    const settings = page.locator('[data-pipeline-settings]');
    await settings.getByRole('button', { name: 'Alert events', exact: true }).click();
    await addStep(page, 'AI summary');

    await node(page, 'source-0').click();
    await nodeEditor(page).locator('[data-alert-stage]').getByRole('button', { name: 'Before a notification' }).click();

    // A step that goes outside runs only as an incident opens.
    await expect(flowCard(page).locator('[data-flow-issue="error"]')).toHaveText([
      /goes outside MoleSignal/,
    ]);
    await expect(node(page, 'sink-0')).toContainText('Into the notification');
    await expect(settings).toContainText('when a notification policy is about to send');

    // Only the policies of an alert event are offered, and incidents opened by hand count here.
    const policies = nodeEditor(page).locator('[data-alert-policies]');
    await expect(policies.getByRole('checkbox', { name: 'Weekly report' })).toHaveCount(0);
    await policies.getByRole('checkbox', { name: 'On-call alerts' }).click();
    await nodeEditor(page).getByRole('checkbox', { name: 'Incidents opened by hand' }).click();
    await expect(node(page, 'source-0')).toContainText('1 policy');

    await node(page, 'transform-0').click();
    await page.getByRole('button', { name: 'Remove', exact: true }).click();

    const trigger = page.getByRole('button', { name: 'Add step' });
    await trigger.click();
    await expect(page.getByRole('menuitem', { name: 'AI summary', exact: true })).toHaveCount(0);
    await expect(page.getByRole('menuitem', { name: 'Call an API', exact: true })).toHaveCount(0);
    await expect(page.getByRole('menuitem', { name: 'Add query results', exact: true })).toHaveCount(0);
    await page.getByRole('menuitem', { name: 'Filter events', exact: true }).click();
    await nodeEditor(page).getByRole('textbox', { name: /^Field/ }).fill('labels.env');
    await nodeEditor(page).getByRole('textbox', { name: /^Value/ }).fill('prod');

    // A test run reads what the policy's run would.
    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', /passed|dropped/);
    expect(backend.runs.at(-1)!.metadata).toMatchObject({
      event: 'triggered',
      policy_name: 'On-call alerts',
      source: 'rule',
    });

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]!.functionSteps.alert).toMatchObject({
      stage: 'notify',
      policies: ['policy-oncall'],
      sources: ['manual'],
    });
  });

  test('calls an API and adds query results when the incident opens', async ({ page }) => {
    const backend = await mountPipelines(page);
    await page.route('**/api/v1/connectors', (route) =>
      route.fulfill({
        json: [
          { id: 'c1', name: 'Archive', kind: 's3', configJson: {}, enabled: true, createdAt: 1, updatedAt: 1 },
          { id: 'c2', name: 'CMDB', kind: 'http', configJson: {}, enabled: true, createdAt: 1, updatedAt: 1 },
        ],
      }),
    );
    await page.goto('/pipelines/new');
    await nameField(page).fill('enrich');
    await page
      .locator('[data-pipeline-settings]')
      .getByRole('button', { name: 'Alert events', exact: true })
      .click();

    await addStep(page, 'Call an API');
    await expect(flowCard(page).locator('[data-flow-issue="error"]')).toHaveCount(0);
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'pending');

    // Only an HTTP connector can answer.
    await nodeEditor(page).getByRole('combobox', { name: 'HTTP connector' }).click();
    await expect(page.getByRole('option', { name: 'Archive' })).toHaveCount(0);
    await page.getByRole('option', { name: 'CMDB', exact: true }).click();
    await expect(node(page, 'transform-0')).toContainText('Call an API · CMDB');

    await addStep(page, 'Add query results');
    await pickStream(page, 'app_logs');
    await nodeEditor(page)
      .getByRole('textbox', { name: /^Query/ })
      .fill("SELECT count(*) FROM app_logs WHERE host = '{{labels.host_name}}'");
    await nodeEditor(page).getByRole('textbox', { name: /^Annotation/ }).fill('recent_errors');
    await expect(node(page, 'transform-1')).toContainText('Add query results · app_logs');

    // A test calls nothing outside; it says so, and shows where the rows would go.
    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'passed');
    await expect(testResult(page).locator('[data-test-step="passed"]')).toHaveCount(2);
    await expect(testResult(page)).toContainText('not called in a test');
    await expect(testResult(page).locator('[data-pipeline-test-output]')).toContainText('recent_errors');
    expect(backend.runs).toHaveLength(0);

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]!.functionSteps.steps).toEqual([
      { transformName: 'call-api', kind: 'http_enrich', connectorId: 'c2' },
      {
        transformName: 'query',
        kind: 'query_enrich',
        language: 'sql',
        stream: 'app_logs',
        streamType: 'logs',
        statement: "SELECT count(*) FROM app_logs WHERE host = '{{labels.host_name}}'",
        windowSecs: 300,
        maxRows: 5,
        annotation: 'recent_errors',
      },
    ]);
  });

  test('tries an alert pipeline on a recent incident', async ({ page }) => {
    const backend = await mountPipelines(page);
    const incident = {
      id: 'inc-1',
      orgId: 'org',
      ruleId: 'rule-9',
      escalationPolicyId: 'p',
      status: 'open',
      severity: 'error',
      summary: 'Checkout errors',
      fingerprint: 'fp',
      currentStep: 0,
      currentLoop: 0,
      currentStepStartedAt: 1,
      assignees: [],
      createdAt: 1_761_609_600_000_000,
      labels: { 'alert.source': 'external', env: 'prod' },
      annotations: { runbook: 'https://wiki' },
      traceIds: [],
      hostIds: [],
      affectedServices: ['checkout', 'payments'],
      triggeringQuery: null,
      priority: 'P2',
      urgency: 'high',
    };

    await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) =>
      route.fulfill({ json: [{ ...incident, affectedServices: ['checkout'] }] }),
    );
    await page.route(/\/api\/v1\/alerts\/incidents\/inc-1$/, (route) => route.fulfill({ json: incident }));

    await page.goto('/pipelines/new');
    await nameField(page).fill('tidy');
    await page
      .locator('[data-pipeline-settings]')
      .getByRole('button', { name: 'Alert events', exact: true })
      .click();
    await addStep(page, 'Set field');
    await nodeEditor(page).getByRole('textbox', { name: /^Field/ }).fill('labels.team');
    await nodeEditor(page).getByRole('textbox', { name: 'Value', exact: true }).fill('payments');

    await testPanel(page).getByRole('combobox', { name: 'A recent alert event' }).click();
    await page.getByRole('option', { name: /^Checkout errors/ }).click();
    await expect(testPanel(page)).toContainText('after the pipelines that ran when it opened');

    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'passed');

    expect(backend.runs[0]!.input).toEqual({
      summary: 'Checkout errors',
      severity: 'error',
      priority: 'P2',
      urgency: 'high',
      labels: { 'alert.source': 'external', env: 'prod' },
      annotations: { runbook: 'https://wiki' },
      // The list holds the first of the services; the sample takes them all.
      affected_services: ['checkout', 'payments'],
    });
    expect(backend.runs[0]!.metadata).toMatchObject({ rule_id: 'rule-9', source: 'external', event: 'opened' });
  });

  test('changes profile metadata as it arrives, keeping where the object is', async ({ page }) => {
    const backend = await mountPipelines(page);
    await page.goto('/pipelines/new');
    await nameField(page).fill('tag profiles');

    const settings = page.locator('[data-pipeline-settings]');

    // A scheduled pipeline does not read profiles.
    await settings.getByRole('combobox', { name: 'Signal type' }).click();
    await expect(page.getByRole('option', { name: 'Profiles', exact: true })).toHaveCount(0);
    await page.keyboard.press('Escape');

    await settings.getByRole('button', { name: 'Realtime', exact: true }).click();
    await settings.getByRole('combobox', { name: 'Signal type' }).click();
    await page.getByRole('option', { name: 'Profiles', exact: true }).click();

    // Every profile arrives in one stream, so there is nothing to choose.
    await expect(node(page, 'source-0')).toContainText('default');
    await node(page, 'source-0').click();
    const profileSource = nodeEditor(page).locator('[data-pipeline-profile-source]');
    await expect(profileSource).toContainText('default');
    await expect(profileSource.getByRole('combobox')).toHaveCount(0);

    await expect(node(page, 'sink-0')).toContainText('Changed in place');
    await node(page, 'sink-0').click();
    await expect(nodeEditor(page)).toContainText("Only a profile's metadata row goes through the steps");

    await addStep(page, 'Set field');
    const field = nodeEditor(page).getByRole('textbox', { name: /^Field/ });
    await field.fill('object_key');
    await expect(flowCard(page).locator('[data-flow-issue="error"]')).toHaveCount(1);
    await field.fill('labels.env');
    await nodeEditor(page).getByRole('textbox', { name: 'Value', exact: true }).fill('prod');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]).toMatchObject({ sourceStream: 'default', targetStream: 'default', cron: '' });
    expect(backend.saved[0]!.functionSteps).toMatchObject({ mode: 'realtime', signalType: 'profiles' });
  });

  test('writes a row per event into an extend table beside the stream', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'owners');

    await addDestination(page, 'Extend table');
    await expect(node(page, 'sink-1')).toContainText('Choose an extend table');
    await nodeEditor(page).getByRole('combobox', { name: 'Extend table' }).click();
    await page.getByRole('option', { name: 'host_owner', exact: true }).click();
    await expect(nodeEditor(page)).toContainText('under the value of its host field');
    await expect(node(page, 'sink-1')).toContainText('host_owner');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]!.functionSteps).toMatchObject({
      sinks: ['app_logs_clean'],
      sinkTables: ['host_owner'],
    });
  });

  test('changes metrics in the stream they arrive in, keeping what a sample rests on', async ({ page }) => {
    const backend = await mountPipelines(page);
    await page.goto('/pipelines/new');
    await nameField(page).fill('tidy cpu');

    const settings = page.locator('[data-pipeline-settings]');
    await settings.getByRole('button', { name: 'Realtime', exact: true }).click();
    await settings.getByRole('combobox', { name: 'Signal type' }).click();
    await page.getByRole('option', { name: 'Metrics', exact: true }).click();
    await node(page, 'source-0').click();
    await pickStream(page, 'cpu');

    // The samples go back into their stream: no destination to choose, nowhere to route them.
    await expect(node(page, 'sink-0')).toContainText('cpu');
    await expect(node(page, 'sink-0')).toContainText('Changed in place');
    await expect(page.getByRole('button', { name: 'Add destination' })).toHaveCount(0);
    await node(page, 'sink-0').click();
    await expect(nodeEditor(page)).toContainText('a changed label makes a new series');

    const trigger = page.getByRole('button', { name: 'Add step' });
    await trigger.click();
    await expect(page.getByRole('menuitem', { name: 'Route to stream', exact: true })).toHaveCount(0);
    await page.getByRole('menuitem', { name: 'Remove fields', exact: true }).click();
    await expect(trigger).toBeFocused();

    const fields = nodeEditor(page).getByRole('textbox', { name: /^Fields/ });
    await fields.fill('pod, le');
    await expect(flowCard(page).locator('[data-flow-issue="error"]')).toHaveText([
      /changes le, which a realtime pipeline on this stream keeps as it arrives\.$/,
    ]);
    await fields.fill('pod');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    // A test run says when a step would change what the sample rests on: that sample would fail.
    await page.route('**/api/v1/functions/run', (route) => {
      const { input } = route.request().postDataJSON() as TestRun;
      return route.fulfill({ json: { output: { ...input, metric_name: 'renamed' }, dropped: false } });
    });
    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'failed');
    await expect(testResult(page)).toContainText('Changes metric_name, which this pipeline keeps as it arrives');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]).toMatchObject({ sourceStream: 'cpu', targetStream: 'cpu', cron: '' });
    expect(backend.saved[0]!.functionSteps).toMatchObject({
      mode: 'realtime',
      signalType: 'metrics',
      sinks: ['cpu'],
      retainSource: false,
    });
  });

  test('changes RUM views in their custom parts as they arrive, and never drops one', async ({ page }) => {
    const backend = await mountPipelines(page);
    await page.route('**/api/v1/streams', (route) =>
      route.request().method() === 'GET'
        ? route.fulfill({ json: [stream('app_logs', 'logs'), stream('rum_views', 'logs'), stream('rum_actions', 'logs')] })
        : route.fallback(),
    );
    await page.goto('/pipelines/new');
    await nameField(page).fill('tidy views');

    const settings = page.locator('[data-pipeline-settings]');
    await settings.getByRole('button', { name: 'Realtime', exact: true }).click();
    await node(page, 'source-0').click();
    await pickStream(page, 'rum_views');

    // The views go back into their stream, changed only in what the application fills itself.
    await expect(node(page, 'sink-0')).toContainText('rum_views');
    await expect(node(page, 'sink-0')).toContainText('Changed in place');
    await node(page, 'sink-0').click();
    await expect(nodeEditor(page)).toContainText('views and vitals never are');

    await addStep(page, 'Set field');
    const field = nodeEditor(page).getByRole('textbox', { name: /^Field/ });
    await field.fill('view.name');
    await nodeEditor(page).getByRole('textbox', { name: 'Value', exact: true }).fill('checkout');
    await expect(flowCard(page).locator('[data-flow-issue="error"]')).toHaveText([
      /changes view\.name, which a realtime pipeline/,
    ]);
    await field.fill('context.team');
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    // A filter drops events, which a pipeline on views never does.
    await addStep(page, 'Filter events');
    await expect(flowCard(page).locator('[data-flow-issue="error"]')).toContainText([
      /can drop events, but a pipeline on rum_views keeps every row/,
    ]);
    await nodeEditor(page).getByRole('button', { name: 'Remove', exact: true }).click();
    await expect(flowStatus(page)).toHaveAttribute('data-flow-status', 'valid');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]).toMatchObject({ sourceStream: 'rum_views', targetStream: 'rum_views', cron: '' });
    expect(backend.saved[0]!.functionSteps).toMatchObject({
      mode: 'realtime',
      signalType: 'logs',
      sinks: ['rum_views'],
      retainSource: false,
    });
  });

  test('says whose access a pipeline reads _audit with', async ({ page }) => {
    // Three pages in one test.
    test.slow();
    const backend = await mountPipelines(page);
    await page.route('**/api/v1/streams', (route) =>
      route.request().method() === 'GET'
        ? route.fulfill({ json: [stream('app_logs', 'logs'), stream('_audit', 'logs')] })
        : route.fallback(),
    );

    // A new pipeline reads as whoever saves it.
    await page.goto('/pipelines/new');
    await pickStream(page, '_audit');
    const note = page.locator('[data-audit-reader]');
    await expect(note).toContainText('Reads _audit as');
    await expect(note).toContainText('(you)');
    await expect(note).not.toContainText('After you save');
    await expect(note).toContainText('If this person loses audit.read');

    // A saved one reads as whoever saved it, until someone who holds the permission saves it.
    backend.saved.push({
      name: 'audit-copy',
      sourceStream: '_audit',
      targetStream: 'audit_copy',
      cron: 'every:15m',
      lookbackSecs: 900,
      enabled: true,
      functionSteps: { steps: [] },
      auditReaderId: 'u-ada',
      auditReader: { id: 'u-ada', displayName: 'Ada', email: 'ada@example.com' },
    } as PipelineBody);
    await page.goto('/pipelines/p-1/edit');
    await node(page, 'source-0').click();
    await expect(note).toContainText('Ada');
    await expect(note).toContainText('After you save, it reads with your access.');
    await expect(note.getByText('Ada', { exact: true })).toHaveAttribute('title', 'ada@example.com');

    await page.goto('/pipelines/p-1');
    await expect(page.getByText('Reads _audit as')).toBeVisible();
    await expect(page.getByText('Ada', { exact: true })).toBeVisible();
  });

  test('lists what each run read and where its events ended, under the retry that is due', async ({ page }) => {
    const now = Date.parse(FROZEN_NOW_ISO) * 1000;
    await page.route('**/api/v1/scheduled_pipelines/p-1', (route) =>
      route.fulfill({
        json: {
          id: 'p-1',
          name: 'checkout-logs',
          sourceStream: 'app_logs',
          targetStream: 'clean_logs',
          cron: 'every:5m',
          lookbackSecs: 300,
          enabled: true,
          retry: {
            source: 'nginx_logs',
            attempt: 2,
            nextAt: now + 120_000_000,
            error: 'read: unavailable: storage unavailable',
          },
          functionSteps: { sources: ['app_logs', 'nginx_logs'], sinks: ['clean_logs'], steps: [] },
        },
      }),
    );
    await page.goto('/pipelines/p-1/history');

    const header = page.getByTestId('page-header');
    await expect(header.getByRole('heading', { level: 1 })).toHaveText('Run history · checkout-logs');
    await expect(page.locator('[data-pipeline-retry]')).toHaveText(
      'Reading or writing a stretch of nginx_logs failed; attempt 2 is in 2 minutes.read: unavailable: storage unavailable',
    );

    // The mock's two runs: one that went through with some events dropped or failed, one retried.
    const rows = page.getByRole('row');
    await expect(rows.nth(1)).toContainText('18,420 read · 18,200 written');
    await expect(rows.nth(1)).toContainText('214 dropped · 6 failed');
    await expect(rows.nth(1)).toContainText('6 event(s) failed');
    await expect(rows.nth(2)).toContainText('Attempt 2');
    await expect(rows.nth(2)).toContainText('`logs`: read: unavailable: storage unavailable');

    await header.getByRole('button', { name: 'Edit pipeline' }).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
  });

  test('counts matching logs into a metric while the logs go on', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'warn-metrics');

    // Steps that keep count need events as they arrive: a scheduled pipeline does not offer them.
    await page.getByRole('button', { name: 'Add step' }).click();
    await expect(page.getByRole('menuitem', { name: 'Generate metric', exact: true })).toHaveCount(0);
    await page.keyboard.press('Escape');

    await page.locator('[data-pipeline-settings]').getByRole('button', { name: 'Realtime', exact: true }).click();
    await addStep(page, 'Generate metric');

    const editor = nodeEditor(page);
    await editor.getByRole('button', { name: 'Add a test' }).click();
    await editor.locator('[data-counting-condition]').getByRole('textbox', { name: 'Field' }).fill('level');
    await editor.locator('[data-counting-condition]').getByRole('textbox', { name: 'Value' }).fill('WARN');
    await editor.getByRole('textbox', { name: /^Metric name/ }).fill('app_warnings_total');
    await editor.getByRole('textbox', { name: /^Group by/ }).fill('appname');
    await expect(node(page, 'transform-0')).toHaveText(/Generate metric · app_warnings_total · by appname$/);
    // The query that reads it, to copy into a dashboard or an alert rule.
    await expect(editor.locator('code')).toHaveText('sum by (appname) (increase(app_warnings_total[5m]))');

    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page)).toHaveAttribute('data-pipeline-test-result', 'passed');
    await expect(testResult(page).locator('[data-counting-note]')).toHaveText(
      'Adds 1 to app_warnings_total{appname="checkout"}.',
    );
    // The event goes on as it came: a metric step changes nothing.
    await expect(testResult(page)).toContainText('Written to app_logs_clean');
    expect(backend.runs).toHaveLength(0);

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]!.functionSteps.steps).toEqual([
      {
        transformName: 'metric',
        kind: 'generate_metric',
        conditions: [{ field: 'level', test: 'equals', value: 'WARN' }],
        metric: {
          name: 'app_warnings_total',
          type: 'count',
          groupBy: ['appname'],
          stream: 'app_logs',
          intervalSecs: 60,
        },
      },
    ]);

    // Saved, it opens as the same form.
    await node(page, 'transform-0').click();
    await expect(nodeEditor(page).getByRole('textbox', { name: /^Metric name/ })).toHaveValue('app_warnings_total');
  });

  test('holds back logs past a daily quota and a rate, sending the rest to a cheaper stream', async ({ page }) => {
    const backend = await mountPipelines(page);
    await startPipeline(page, 'budgeted');
    await page.locator('[data-pipeline-settings]').getByRole('button', { name: 'Realtime', exact: true }).click();

    await addStep(page, 'Quota');
    let editor = nodeEditor(page);
    await editor.getByRole('textbox', { name: /^Daily limit \(events\)/ }).fill('1000000');
    await editor.getByRole('textbox', { name: /^Split by/ }).fill('appname');
    await editor.getByRole('button', { name: 'Add a bucket' }).click();
    await editor.locator('[data-quota-override]').getByRole('textbox', { name: 'appname' }).fill('checkout');
    await editor.locator('[data-quota-override]').getByRole('textbox', { name: 'Limit (events)' }).fill('5000000');
    await editor.getByRole('button', { name: 'Send to another stream' }).click();
    await nameStream(page, 'app_logs_cold');
    await expect(node(page, 'transform-0')).toHaveText(/Quota · 1000000 events a day · by appname$/);

    await addStep(page, 'Throttle');
    editor = nodeEditor(page);
    await editor.getByRole('textbox', { name: /^Events per window/ }).fill('100');
    await editor.getByRole('textbox', { name: /^Per group of/ }).fill('kubernetes.pod_name');
    // A throttle drops what goes past it or sends it elsewhere; it never lets it through.
    await expect(editor.getByRole('button', { name: 'Let through' })).toHaveCount(0);
    await expect(node(page, 'transform-1')).toHaveText(/Throttle · 100 per 60 s · by kubernetes\.pod_name$/);

    await page.getByRole('button', { name: 'Run test' }).click();
    await expect(testResult(page).locator('[data-counting-note]')).toHaveText([
      'Counts in appname=checkout (5,000,000 events a day); past the limit it is sent to app_logs_cold.',
      'Counts in kubernetes.pod_name=checkout-7d9f8 (100 per 60 s); past the limit it is dropped.',
    ]);
    await expect(testResult(page)).toContainText('Written to app_logs_clean');

    await save(page).click();
    await expect(page).toHaveURL(/\/pipelines\/p-1\/edit$/);
    expect(backend.saved[0]!.functionSteps.steps).toEqual([
      {
        transformName: 'quota',
        kind: 'quota',
        conditions: [],
        quota: {
          unit: 'events',
          limit: 1_000_000,
          partitionBy: ['appname'],
          overrides: [{ values: { appname: 'checkout' }, limit: 5_000_000 }],
          action: 'route',
          target: 'app_logs_cold',
        },
      },
      {
        transformName: 'throttle',
        kind: 'throttle',
        conditions: [],
        throttle: { limit: 100, windowSecs: 60, groupBy: ['kubernetes.pod_name'], action: 'drop' },
      },
    ]);
  });

  test('shows what its quotas let through today on the pipeline page', async ({ page }) => {
    await page.route('**/api/v1/scheduled_pipelines/p-9', (route) =>
      route.fulfill({
        json: {
          id: 'p-9',
          name: 'budgeted',
          sourceStream: 'app_logs',
          targetStream: 'app_logs',
          cron: '',
          lookbackSecs: 300,
          enabled: true,
          functionSteps: {
            mode: 'realtime',
            signalType: 'logs',
            steps: [
              {
                transformName: 'daily',
                kind: 'quota',
                quota: {
                  unit: 'events',
                  limit: 1000,
                  partitionBy: ['service'],
                  overrides: [{ values: { service: 'checkout' }, limit: 5000 }],
                  action: 'drop',
                },
              },
              {
                transformName: 'burst',
                kind: 'throttle',
                throttle: { limit: 10, windowSecs: 1, action: 'drop' },
              },
            ],
          },
        },
      }),
    );
    await page.route('**/api/v1/scheduled_pipelines/p-9/runs**', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/v1/scheduled_pipelines/p-9/limits', (route) =>
      route.fulfill({
        json: {
          dayStart: 0,
          buckets: [
            { step: 'daily', bucket: '["cart"]', admittedEvents: 1000, admittedBytes: 51200, overEvents: 240, overBytes: 12288 },
            { step: 'daily', bucket: '["checkout"]', admittedEvents: 1200, admittedBytes: 61440, overEvents: 0, overBytes: 0 },
          ],
        },
      }),
    );
    await page.goto('/pipelines/p-9');

    const section = page.locator('[data-pipeline-section]').filter({ hasText: 'Limits today' });
    const rows = section.getByRole('row');
    await expect(rows.nth(1)).toContainText('service=cart');
    await expect(rows.nth(1)).toContainText('1,000 of 1,000');
    await expect(rows.nth(1)).toContainText('240 · Drop');
    await expect(rows.nth(2)).toContainText('1,200 of 5,000');
    // A throttle that has counted nothing today is still listed.
    await expect(rows.nth(3)).toContainText('burst');
    await expect(rows.nth(3)).toContainText('Whole step');
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
              defaultHomeRoute: '/home',
              timeFormat: 'iso_24h',
              dateFormat: 'yyyy_mm_dd_dash',
              timezone: '',
              keyboardShortcutsEnabled: true,
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
