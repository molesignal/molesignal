import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const EMPTY_RED = {
  request_count: 0,
  error_count: 0,
  error_rate: 0,
  duration_sum_micros: 0,
  duration_average_micros: 0,
  p50_micros: 0,
  p95_micros: 0,
  p99_micros: 0,
  latency_partial: false,
  exemplars: [],
};

const EMPTY_APM_OVERVIEW = {
  meta: {
    range: { from: 1, to: 2 },
    resolution: 'minute',
    data_quality: { partial: false, gaps: [], overflow_dimensions: [] },
    activation_boundary: false,
  },
  red: EMPTY_RED,
  trend: [],
  service_health: { healthy: 0, warning: 0, critical: 0, no_traffic: 0 },
  services: [],
  top_transactions: [],
  top_dependencies: [],
  top_errors: [],
  recent_versions: [],
};

const EMPTY_RUM_OVERVIEW = {
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
};

const MODULES = [
  {
    path: '/logs',
    canvas: '[data-workspace="logs"]',
    surface: '[data-query-workbench-appearance="surface"]',
  },
  {
    path: '/metrics',
    canvas: '[data-testid="metrics-page"]',
    surface: '[data-metrics-query-surface="true"]',
  },
  {
    path: '/apm/overview',
    canvas: '[data-surface-workbench-page="apm"]',
    surface: '[data-apm-navigation="surface"]',
  },
  {
    path: '/rum/overview',
    canvas: '[data-surface-workbench-page="rum"]',
    surface: '[data-rum-navigation="surface"]',
  },
  {
    path: '/profiles',
    canvas: '[data-page-appearance="surface"]',
    surface: '[data-page-appearance="surface"] section',
  },
] as const;

const REMAINING_MODULES = [
  { path: '/dashboards', surface: '[data-page-body] > :last-child' },
  { path: '/alerts', surface: '[data-page-body] > :last-child' },
  { path: '/synthetics', surface: '[data-page-body] > section' },
  { path: '/status-pages', surface: '[data-page-body] > :last-child' },
  { path: '/agent', surface: '[data-agent-content-surface]' },
  { path: '/datasource', surface: '[data-datasource-filter-surface]' },
  { path: '/streams', surface: '[data-page-body] > :last-child' },
  { path: '/pipelines', surface: '[data-page-body] > :last-child' },
  { path: '/functions', surface: '[data-page-body] > :last-child' },
  { path: '/extend-tables', surface: '[data-page-body] > :last-child' },
  { path: '/reports', surface: '[data-reports-workspace]' },
  { path: '/iam/users', surface: '[data-management-content]' },
  { path: '/settings/general', surface: '[data-management-content]' },
] as const;

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route(/\/api\/v1\/apm\/overview(?:\?.*)?$/, (route) =>
    route.fulfill({ json: EMPTY_APM_OVERVIEW }),
  );
  await page.route(/\/api\/v1\/rum\/overview(?:\?.*)?$/, (route) =>
    route.fulfill({ json: EMPTY_RUM_OVERVIEW }),
  );
  await page.route(/\/api\/v1\/synthetics\/monitors(?:\?.*)?$/, (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route(/\/api\/v1\/synthetics\/locations(?:\?.*)?$/, (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route(/\/api\/v1\/extend_tables(?:\?.*)?$/, (route) =>
    route.fulfill({ json: [] }),
  );
});

test('uses the Trace canvas and borderless Surface hierarchy across analysis modules', async ({
  page,
}) => {
  for (const module of MODULES) {
    await page.goto(module.path);

    const shell = page.locator('[data-shell-layout="surface-workbench"]');
    const canvas = page.locator(module.canvas).first();
    const header = page.getByTestId('page-header');
    const surface = page.locator(module.surface).first();

    await expect(shell).toBeVisible();
    await expect(canvas).toBeVisible();
    await expect(header).toBeVisible();
    await expect(surface).toBeVisible();
    await expect(page.getByRole('banner')).toHaveCSS('border-bottom-width', '0px');
    await expect(page.getByTestId('primary-sidebar')).toHaveCSS(
      'border-right-width',
      '0px',
    );
    await expect(header).toHaveCSS('border-bottom-width', '0px');
    await expect(header).toHaveCSS('margin-top', '12px');
    await expect(header).toHaveCSS('margin-bottom', '12px');
    await expect(surface).toHaveCSS('border-top-width', '0px');
    await expect(surface).toHaveCSS('border-right-width', '0px');
    await expect(surface).toHaveCSS('border-bottom-width', '0px');
    await expect(surface).toHaveCSS('border-left-width', '0px');

    const [canvasBackground, surfaceBackground] = await Promise.all([
      canvas.evaluate((element) => getComputedStyle(element).backgroundColor),
      surface.evaluate((element) => getComputedStyle(element).backgroundColor),
    ]);
    expect(surfaceBackground).not.toBe(canvasBackground);
  }
});

test('extends the Trace Surface hierarchy to the remaining product modules', async ({
  page,
}) => {
  for (const module of REMAINING_MODULES) {
    await page.goto(module.path);

    const shell = page.locator('[data-shell-layout="surface-workbench"]');
    const canvas = page.locator('[data-page-appearance="surface"]').first();
    const header = page.getByTestId('page-header');
    const surface = page.locator(module.surface).first();

    await expect(shell).toBeVisible();
    await expect(canvas).toBeVisible();
    await expect(header).toBeVisible();
    await expect(surface).toBeVisible();
    await expect(page.getByRole('banner')).toHaveCSS('border-bottom-width', '0px');
    await expect(page.getByTestId('primary-sidebar')).toHaveCSS(
      'border-right-width',
      '0px',
    );
    await expect(header).toHaveCSS('border-bottom-width', '0px');
    await expect(header).toHaveCSS('margin-top', '12px');
    await expect(header).toHaveCSS('margin-bottom', '12px');

    for (const side of ['top', 'right', 'bottom', 'left'] as const) {
      await expect(surface).toHaveCSS(`border-${side}-width`, '0px');
    }
    await expect(surface).toHaveCSS('border-radius', '8px');

    const [canvasBackground, surfaceBackground, surfaceShadow] =
      await Promise.all([
        canvas.evaluate((element) => getComputedStyle(element).backgroundColor),
        surface.evaluate((element) => getComputedStyle(element).backgroundColor),
        surface.evaluate((element) => getComputedStyle(element).boxShadow),
      ]);
    expect(surfaceBackground).not.toBe(canvasBackground);
    expect(surfaceShadow).not.toBe('none');
  }
});

test('keeps module tabs underlined and Explorer columns separated by canvas gutters', async ({
  page,
}) => {
  await page.goto('/logs');

  const workspace = page.locator('[data-log-workspace-layout="surface-grid"]');
  const fieldPanel = workspace.locator('aside[data-variant="utility"]');
  const results = workspace.locator('[data-workspace-pane="log-results"]');
  expect(
    await workspace.evaluate((element) => getComputedStyle(element).gap),
  ).toBe('8px');
  await expect(fieldPanel).toHaveCSS('border-right-width', '0px');
  await expect(results).toHaveCSS('border-left-width', '0px');

  for (const [path, selector] of [
    ['/apm/overview', '[data-apm-navigation="surface"]'],
    ['/rum/overview', '[data-rum-navigation="surface"]'],
  ] as const) {
    await page.goto(path);
    const navigation = page.locator(selector);
    const active = navigation.locator('[aria-current="page"]').first();
    await expect(active).toBeVisible();
    await expect(active).toHaveCSS('border-bottom-width', '0px');
    await expect(active).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await expect(active).toHaveCSS('border-radius', '8px');
    await expect(navigation).toHaveCSS('border-radius', '12px');
    await expect(navigation).toHaveCSS('overflow-x', 'hidden');
    const underline = await active.evaluate((element) => {
      const style = getComputedStyle(element, '::after');
      return {
        backgroundColor: style.backgroundColor,
        height: style.height,
      };
    });
    expect(underline.height).toBe('3px');
    expect(underline.backgroundColor).not.toBe('rgba(0, 0, 0, 0)');
    expect(
      await navigation.evaluate((element) => getComputedStyle(element).boxShadow),
    ).not.toBe('none');
  }
});

test('keeps module and nested secondary menus at the shared height', async ({
  page,
}) => {
  const menus = [
    {
      path: '/apm/overview',
      selectors: ['[data-apm-navigation="surface"] > nav'],
    },
    {
      path: '/rum/performance/overview',
      selectors: [
        '[data-rum-navigation="surface"] > nav',
        '[data-rum-subnavigation="performance"]',
      ],
    },
    {
      path: '/reports',
      selectors: ['[data-reports-navigation]'],
    },
  ] as const;

  let sharedHeight: number | undefined;
  for (const group of menus) {
    await page.goto(group.path);
    for (const selector of group.selectors) {
      const menu = page.locator(selector);
      await expect(menu).toBeVisible();
      const height = await menu.evaluate(
        (element) => element.getBoundingClientRect().height,
      );
      sharedHeight ??= height;
      expect(height).toBeCloseTo(sharedHeight, 3);
    }
  }
});

test('keeps shared and native selectors borderless', async ({ page }) => {
  await page.goto('/rum/overview');

  const sharedSelectors = page.locator('[data-ui="select-trigger"]');
  await expect(sharedSelectors.first()).toBeVisible();
  expect(await sharedSelectors.count()).toBeGreaterThan(0);
  for (const selector of await sharedSelectors.all()) {
    await expect(selector).toHaveCSS('border-top-width', '0px');
    await expect(selector).toHaveCSS('border-right-width', '0px');
    await expect(selector).toHaveCSS('border-bottom-width', '0px');
    await expect(selector).toHaveCSS('border-left-width', '0px');
  }

  const firstSelector = sharedSelectors.first();
  await firstSelector.click();
  await expect(firstSelector).toHaveAttribute('data-state', 'open');
  await expect(firstSelector).toHaveCSS('border-top-width', '0px');

  await page.goto('/synthetics/overview');
  const nativeSelector = page.locator('select').first();
  await expect(nativeSelector).toHaveCSS('border-top-width', '0px');
  await expect(nativeSelector).toHaveCSS('border-right-width', '0px');
  await expect(nativeSelector).toHaveCSS('border-bottom-width', '0px');
  await expect(nativeSelector).toHaveCSS('border-left-width', '0px');
});

test('keeps global and workbench search controls borderless while focused', async ({
  page,
}) => {
  await page.goto('/rum/sessions');

  const controls = [
    page.getByTestId('command-palette-trigger'),
    page.getByPlaceholder(
      'Search users, pages, browsers, versions, or sessions…',
    ),
  ];

  for (const control of controls) {
    await expect(control).toBeVisible();
    for (const side of ['top', 'right', 'bottom', 'left'] as const) {
      await expect(control).toHaveCSS(`border-${side}-width`, '0px');
    }
    await control.focus();
    await expect(control).toHaveCSS('border-top-width', '0px');
    await expect(control).toHaveCSS('box-shadow', 'none');
  }
});

test('keeps every text input surface borderless', async ({ page }) => {
  await page.goto('/apm/overview');

  const inputs = page.locator(
    '[data-surface-workbench-page="apm"] input:not([type="checkbox"]):not([type="radio"]):not([type="range"]):not([type="file"]):not([type="color"]):not([type="button"]):not([type="submit"]):not([type="reset"]):not([type="image"]):not([type="hidden"])',
  );
  expect(await inputs.count()).toBeGreaterThan(0);

  for (const input of await inputs.all()) {
    for (const side of ['top', 'right', 'bottom', 'left'] as const) {
      await expect(input).toHaveCSS(`border-${side}-width`, '0px');
    }
    await expect(input).toHaveCSS('box-shadow', 'none');
  }

  const firstInput = inputs.first();
  await firstInput.focus();
  await expect(firstInput).toHaveCSS('border-top-width', '0px');
  await expect(firstInput).toHaveCSS('box-shadow', 'none');
});

test('keeps compound and read-only input surfaces borderless', async ({
  page,
}) => {
  await page.goto('/datasource/recommended/kubernetes');

  const searchInput = page.getByPlaceholder(
    'Search sources, platforms, or integration methods…',
  );
  await expect(searchInput).toBeVisible();

  const surfaces = page.locator(
    '[data-ui="input-control"], [data-ui="read-only-control"]',
  );
  expect(await surfaces.count()).toBeGreaterThanOrEqual(3);

  for (const surface of await surfaces.all()) {
    for (const side of ['top', 'right', 'bottom', 'left'] as const) {
      await expect(surface).toHaveCSS(`border-${side}-width`, '0px');
    }
    await expect(surface).toHaveCSS('box-shadow', 'none');
  }

  await searchInput.focus();
  const searchSurface = searchInput.locator('..');
  await expect(searchSurface).toHaveCSS('border-top-width', '0px');
  await expect(searchSurface).toHaveCSS('box-shadow', 'none');
});

test('aligns calendar weekdays and dates without a redundant Time heading', async ({
  page,
}) => {
  await page.goto('/rum/overview');

  const timeRange = page.locator('[data-time-range-trigger="true"]');
  await expect(timeRange).toBeVisible();
  await timeRange.click();

  const absolutePicker = page
    .locator('[data-time-picker-surface="workbench"]')
    .getByRole('button', { name: /Open date and time picker:/ })
    .first();
  await expect(absolutePicker).toBeVisible();
  await absolutePicker.click();

  const dateTimePicker = page.locator(
    '[data-slot="date-time-picker-content"]',
  );
  await expect(dateTimePicker).toBeVisible();

  const weekdayCells = dateTimePicker.locator('.rdp-weekday');
  const firstWeekCells = dateTimePicker
    .locator('.rdp-week')
    .first()
    .locator('.rdp-day');
  await expect(weekdayCells).toHaveCount(7);
  await expect(firstWeekCells).toHaveCount(7);

  const weekdayCenters = await weekdayCells.evaluateAll((elements) =>
    elements.map((element) => {
      const rect = element.getBoundingClientRect();
      return rect.x + rect.width / 2;
    }),
  );
  const dateCenters = await firstWeekCells.evaluateAll((elements) =>
    elements.map((element) => {
      const rect = element.getBoundingClientRect();
      return rect.x + rect.width / 2;
    }),
  );
  for (let index = 0; index < 7; index += 1) {
    expect(
      Math.abs(weekdayCenters[index]! - dateCenters[index]!),
    ).toBeLessThan(0.5);
  }

  const typography = await dateTimePicker.evaluate((element) => {
    const weekday = element.querySelector('.rdp-weekday');
    const day = element.querySelector('.rdp-day_button');
    if (!weekday || !day) return null;
    const weekdayStyle = getComputedStyle(weekday);
    const dayStyle = getComputedStyle(day);
    return {
      weekdayFamily: weekdayStyle.fontFamily,
      weekdaySize: weekdayStyle.fontSize,
      dayFamily: dayStyle.fontFamily,
      daySize: dayStyle.fontSize,
    };
  });
  expect(typography).not.toBeNull();
  expect(typography?.weekdayFamily).toBe(typography?.dayFamily);
  expect(typography?.weekdaySize).toBe(typography?.daySize);

  await expect(
    dateTimePicker
      .locator('[data-date-time-picker-section="time"]')
      .getByText('Time', { exact: true }),
  ).toHaveCount(0);
});
