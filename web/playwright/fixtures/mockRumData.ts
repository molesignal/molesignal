/**
 * The RUM data of the mock organization: ten visits to two applications and what each one did,
 * as the explorer reads it. A visit leaves its views, the actions its user took, the requests the
 * page made, the errors it ran into and the long tasks it blocked on; the session row is derived
 * from them, so its counts are the counts of the rows that exist.
 *
 * Every row is a flat object under the dotted names of the backend's columns. Durations are in
 * the nanoseconds an SDK sends. All times are relative to the frozen clock of the mock session.
 */
import type { RumRow, RumType } from '../../src/api/rum';

const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;

/** A time in the nanoseconds the SDKs send, from milliseconds. */
const ns = (milliseconds: number) => Math.round(milliseconds * 1_000_000);

export const MOCK_RUM = {
  session: {
    /** A recorded visit that ended in a failed order request. */
    checkout: '3f6c9a52-1b7e-4d08-a5c3-90e2d41f7b68',
    /** Rage clicks on the pay button, three errors and a payment provider that timed out. */
    frustrated: '9d41e7b0-62fa-4c3d-8e15-b7a09c52d3f4',
    /** A visitor who never signed in. */
    anonymous: 'c7a5d2f1-0e83-4b96-9a4c-1d6e8f3b2a07',
    /** A run of a synthetic check, which the figures of real users leave out. */
    synthetic: 'e91c4a70-2b5d-4f18-a6c3-7d0f8b2e9a14',
  },
  /** The check that ran it, and the run. */
  check: { id: 'chk-checkout', result: 'task-7c1d', location: 'loc-eu-west' },
  /** The trace the failed order request carried. */
  trace: '4bf92f3577b34da6a3ce929d0e0e4736',
  span: '00f067aa0ba902b7',
  applications: { web: 'storefront', app: 'storefront-app' },
} as const;

interface PageSpec {
  name: string;
  group: string;
  loading: number;
  lcp: number;
  inp: number;
  cls: number;
  requests: number;
  taps: readonly string[];
  /** What the browser names as the element that set each level; the file of the largest paint, where it is one. */
  elements: { lcp: string; lcpFile: string | null; inp: string; cls: string };
}

const PAGES: Record<string, PageSpec> = {
  '/': { name: 'Home', group: '/', loading: 820, lcp: 1050, inp: 70, cls: 0.01, requests: 7, taps: ['Shop now', 'Search'], elements: { lcp: 'img.hero', lcpFile: 'https://cdn.example.com/img/hero.webp', inp: 'button.shop-now', cls: 'div.banner' } },
  '/products': { name: 'Products', group: '/products', loading: 1380, lcp: 2150, inp: 120, cls: 0.04, requests: 11, taps: ['Filter: Shoes', 'Sort by price', 'Add to cart'], elements: { lcp: 'img.product-card', lcpFile: 'https://cdn.example.com/img/sku-104.jpg', inp: 'button.filter', cls: 'div.filters' } },
  '/products/sku-104': { name: 'Product', group: '/products/:sku', loading: 1950, lcp: 2900, inp: 180, cls: 0.06, requests: 12, taps: ['Size 42', 'Add to cart'], elements: { lcp: 'img.product-main', lcpFile: 'https://cdn.example.com/img/sku-104.jpg', inp: 'button.add-to-cart', cls: 'img.product-main' } },
  '/cart': { name: 'Cart', group: '/cart', loading: 940, lcp: 1300, inp: 90, cls: 0.02, requests: 8, taps: ['Update quantity', 'Checkout'], elements: { lcp: 'h1.page-title', lcpFile: null, inp: 'button.checkout', cls: 'div.promo' } },
  '/checkout': { name: 'Checkout', group: '/checkout', loading: 2240, lcp: 3150, inp: 410, cls: 0.14, requests: 14, taps: ['Apply coupon', 'Pay now'], elements: { lcp: 'div.order-summary', lcpFile: null, inp: 'button.pay-now', cls: 'div.promo-banner' } },
  '/search': { name: 'Search', group: '/search', loading: 1100, lcp: 1700, inp: 110, cls: 0.03, requests: 9, taps: ['Open filters'], elements: { lcp: 'img.result-thumb', lcpFile: 'https://cdn.example.com/img/sku-104.jpg', inp: 'button.open-filters', cls: 'div.results' } },
  '/pricing': { name: 'Pricing', group: '/pricing', loading: 760, lcp: 980, inp: 60, cls: 0.0, requests: 6, taps: ['Compare plans'], elements: { lcp: 'h1.hero-title', lcpFile: null, inp: 'button.compare', cls: 'div.plans' } },
  '/blog/launch': { name: 'Launch post', group: '/blog/:slug', loading: 1020, lcp: 1500, inp: 80, cls: 0.02, requests: 6, taps: [], elements: { lcp: 'img.cover', lcpFile: 'https://cdn.example.com/img/hero.webp', inp: 'a.share', cls: 'img.cover' } },
  '/orders': { name: 'Orders', group: '/orders', loading: 1260, lcp: 1900, inp: 150, cls: 0.05, requests: 10, taps: ['Open order'], elements: { lcp: 'table.orders', lcpFile: null, inp: 'button.open-order', cls: 'table.orders' } },
  '/orders/9921': { name: 'Order', group: '/orders/:id', loading: 1180, lcp: 1600, inp: 100, cls: 0.03, requests: 8, taps: ['Download invoice'], elements: { lcp: 'h1.order-title', lcpFile: null, inp: 'button.download-invoice', cls: 'div.invoice' } },
};

/** The scripts the long tasks of a Chromium browser are put down to: where they came from, what they ran and how they were called. */
const SCRIPTS = {
  validate: { url: 'https://cdn.example.com/assets/checkout.9c2d.js', function: 'validateAddress', invoker: 'event-listener' },
  render: { url: 'https://cdn.example.com/assets/app.4f3a.js', function: 'renderTotals', invoker: 'event-listener' },
  hydrate: { url: 'https://cdn.example.com/assets/app.4f3a.js', function: 'hydrate', invoker: 'module-script' },
  vendor: { url: 'https://cdn.example.com/assets/vendor.77de.js', function: '', invoker: 'classic-script' },
  tag: { url: 'https://www.googletagmanager.com/gtm.js', function: '', invoker: 'classic-script' },
} as const;

type Script = (typeof SCRIPTS)[keyof typeof SCRIPTS];

/** Requests a page makes, in the order it makes them. */
const REQUESTS = [
  { type: 'document', method: 'GET', host: 'shop.example.com', path: '{page}', status: 200, ms: 180, size: 18_400 },
  { type: 'css', method: 'GET', host: 'cdn.example.com', path: '/assets/app.91cc.css', status: 200, ms: 60, size: 42_100 },
  { type: 'js', method: 'GET', host: 'cdn.example.com', path: '/assets/app.4f3a.js', status: 200, ms: 140, size: 312_000 },
  { type: 'font', method: 'GET', host: 'cdn.example.com', path: '/fonts/inter.woff2', status: 200, ms: 90, size: 48_000 },
  { type: 'fetch', method: 'GET', host: 'api.example.com', path: '/api/products', status: 200, ms: 210, size: 9_800 },
  { type: 'image', method: 'GET', host: 'cdn.example.com', path: '/img/hero.webp', status: 200, ms: 320, size: 121_000 },
  { type: 'fetch', method: 'GET', host: 'api.example.com', path: '/api/cart', status: 200, ms: 120, size: 1_900 },
  { type: 'xhr', method: 'POST', host: 'api.example.com', path: '/api/events', status: 202, ms: 45, size: 0 },
  { type: 'fetch', method: 'GET', host: 'api.example.com', path: '/api/recommendations', status: 200, ms: 380, size: 6_400 },
  { type: 'image', method: 'GET', host: 'cdn.example.com', path: '/img/sku-104.jpg', status: 200, ms: 260, size: 88_000 },
  { type: 'fetch', method: 'GET', host: 'api.example.com', path: '/api/user', status: 200, ms: 95, size: 700 },
  { type: 'js', method: 'GET', host: 'cdn.example.com', path: '/assets/vendor.77de.js', status: 200, ms: 210, size: 540_000 },
  { type: 'fetch', method: 'GET', host: 'api.example.com', path: '/api/inventory', status: 200, ms: 150, size: 2_300 },
  { type: 'image', method: 'GET', host: 'cdn.example.com', path: '/img/logo.svg', status: 200, ms: 40, size: 3_100 },
] as const;

interface Failure {
  page: string;
  method: string;
  host: string;
  path: string;
  status: number;
  ms: number;
  /** The request carried a trace id of the backend. */
  trace?: boolean;
}

interface Problem {
  page: string;
  message: string;
  type: string;
  source: 'source' | 'network' | 'custom';
  handling: 'handled' | 'unhandled';
}

/** What an SDK read as frustration on an element of a page: clicks again and again, a click nothing came of, a click that ended in an error. */
interface Sign {
  page: string;
  /** The text of the element; none where it has none. */
  name: string | null;
  /** How a browser tells the element; an app has none. */
  selector: string | null;
  kinds: readonly string[];
  /** How many actions in a row showed it. */
  times?: number;
}

interface Visit {
  id: string;
  app: string;
  /** Minutes ago the visit began, and how long it lasted. */
  ago: number;
  lasted: number;
  active?: true;
  user: readonly [id: string, name: string, email: string] | null;
  browser: readonly [name: string, version: string] | null;
  os: readonly [name: string, version: string];
  device: readonly [type: string, model: string, brand: string];
  place: readonly [iso: string, country: string, city: string, continent: string, latitude: number, longitude: number];
  ip: string;
  release: string;
  pages: readonly string[];
  replay?: true;
  rage?: true;
  /** A run of a check, not a person. */
  synthetic?: true;
  signs?: readonly Sign[];
  failures?: readonly Failure[];
  problems?: readonly Problem[];
}

const API = 'api.example.com';

const ORDER_FAILED: Failure = { page: '/checkout', method: 'POST', host: API, path: '/api/orders', status: 500, ms: 482, trace: true };
const PAYMENT_TIMEOUT: Failure = { page: '/checkout', method: 'POST', host: 'payments.example.com', path: '/v1/intents', status: 504, ms: 3_000 };

const VISITS: readonly Visit[] = [
  {
    id: MOCK_RUM.session.checkout, app: 'storefront', ago: 47, lasted: 12,
    user: ['u-1042', 'Mei Lin', 'mei.lin@example.com'], browser: ['Chrome', '126.0'], os: ['macOS', '14.5'],
    device: ['desktop', 'MacBook Pro', 'Apple'], place: ['CN', 'China', 'Shanghai', 'AS', 31.23, 121.47],
    ip: '203.0.113.24', release: '2.4.0', pages: ['/products', '/cart', '/checkout'], replay: true,
    signs: [
      { page: '/checkout', name: 'Apply coupon', selector: 'button[data-testid="apply-coupon"]', kinds: ['dead_click'] },
      { page: '/checkout', name: 'Pay now', selector: 'button[data-testid="pay-now"]', kinds: ['error_click'] },
    ],
    failures: [ORDER_FAILED],
    problems: [{ page: '/checkout', message: 'Failed to fetch /api/orders: 500 Internal Server Error', type: 'ServerError', source: 'network', handling: 'handled' }],
  },
  {
    id: 'a2e4c8b6-7d15-4f39-b0a8-5c3e91d7f204', app: 'storefront', ago: 31, lasted: 4,
    user: ['u-2210', 'Anna Weber', 'anna.weber@example.com'], browser: ['Safari', '17.5'], os: ['iOS', '17.5'],
    device: ['mobile', 'iPhone 15', 'Apple'], place: ['DE', 'Germany', 'Berlin', 'EU', 52.52, 13.4],
    ip: '198.51.100.77', release: '2.4.0', pages: ['/', '/cart'],
    signs: [
      { page: '/', name: null, selector: 'div.hero-banner', kinds: ['rage_click'], times: 3 },
    ],
  },
  {
    id: MOCK_RUM.session.anonymous, app: 'storefront', ago: 22, lasted: 1,
    user: null, browser: ['Firefox', '127.0'], os: ['Linux', '6.8'],
    device: ['desktop', '', ''], place: ['US', 'United States', 'Seattle', 'NA', 47.61, -122.33],
    ip: '192.0.2.18', release: '2.4.0', pages: ['/blog/launch'],
  },
  {
    id: MOCK_RUM.session.frustrated, app: 'storefront', ago: 18, lasted: 9,
    user: ['u-3307', 'Carlos Silva', 'carlos.silva@example.com'], browser: ['Chrome', '126.0'], os: ['Windows', '11'],
    device: ['desktop', '', ''], place: ['BR', 'Brazil', 'São Paulo', 'SA', -23.55, -46.63],
    ip: '203.0.113.201', release: '2.3.9', pages: ['/products/sku-104', '/cart', '/checkout'], replay: true, rage: true,
    signs: [
      { page: '/products/sku-104', name: 'Size 42', selector: 'button[data-testid="size-42"]', kinds: ['dead_click'] },
    ],
    failures: [PAYMENT_TIMEOUT],
    problems: [
      { page: '/checkout', message: "Cannot read properties of undefined (reading 'price')", type: 'TypeError', source: 'source', handling: 'unhandled' },
      { page: '/checkout', message: 'Payment provider timeout', type: 'GatewayTimeout', source: 'network', handling: 'handled' },
      { page: '/checkout', message: "Cannot read properties of undefined (reading 'price')", type: 'TypeError', source: 'source', handling: 'unhandled' },
    ],
  },
  {
    id: 'e91b3d70-4a2c-4856-8f1e-62d0a7c5b389', app: 'storefront-app', ago: 12, lasted: 6,
    user: ['u-4120', 'Yuki Tanaka', 'yuki.tanaka@example.com'], browser: null, os: ['Android', '14'],
    device: ['mobile', 'Pixel 8', 'Google'], place: ['JP', 'Japan', 'Tokyo', 'AS', 35.68, 139.69],
    ip: '198.51.100.9', release: '1.8.2', pages: ['/', '/search'],
    signs: [
      { page: '/', name: 'Shop now', selector: null, kinds: ['error_tap'] },
      { page: '/search', name: 'Search', selector: null, kinds: ['rage_tap'], times: 3 },
    ],
  },
  {
    id: '58f0a6c3-9b24-4e71-a3d5-0c7e1b94f862', app: 'storefront', ago: 8, lasted: 2,
    user: null, browser: ['Edge', '126.0'], os: ['Windows', '10'],
    device: ['desktop', '', ''], place: ['GB', 'United Kingdom', 'London', 'EU', 51.51, -0.13],
    ip: '203.0.113.88', release: '2.4.0', pages: ['/pricing'],
  },
  {
    id: 'b4d79e25-1c60-4a38-92f7-8e03a5d1c6b4', app: 'storefront', ago: 4, lasted: 3.5, active: true,
    user: ['u-5502', 'Priya Nair', 'priya.nair@example.com'], browser: ['Chrome', '126.0'], os: ['macOS', '14.5'],
    device: ['desktop', 'MacBook Air', 'Apple'], place: ['IN', 'India', 'Bengaluru', 'AS', 12.97, 77.59],
    ip: '192.0.2.150', release: '2.4.0', pages: ['/orders', '/orders/9921'],
  },
  {
    id: '07c3e815-d9a4-4b2f-8160-f5a29b7e4d31', app: 'storefront', ago: 190, lasted: 8,
    user: ['u-1042', 'Mei Lin', 'mei.lin@example.com'], browser: ['Chrome', '126.0'], os: ['macOS', '14.5'],
    device: ['desktop', 'MacBook Pro', 'Apple'], place: ['CN', 'China', 'Shanghai', 'AS', 31.23, 121.47],
    ip: '203.0.113.24', release: '2.4.0', pages: ['/products', '/products/sku-104', '/cart'],
    signs: [
      { page: '/cart', name: 'Checkout', selector: 'button[data-testid="checkout"]', kinds: ['dead_click'] },
    ],
  },
  {
    id: '6aa1f4d2-3e87-4c09-b5d6-91c8e2073f5a', app: 'storefront', ago: 540, lasted: 2,
    user: null, browser: ['Safari', '17.4'], os: ['macOS', '14.4'],
    device: ['desktop', '', ''], place: ['FR', 'France', 'Paris', 'EU', 48.86, 2.35],
    ip: '198.51.100.140', release: '2.3.9', pages: ['/', '/search'],
  },
  {
    id: 'd2f8b0a9-5c71-4e46-a18d-3b6f94e07c15', app: 'storefront', ago: 1230, lasted: 5,
    user: ['u-2210', 'Anna Weber', 'anna.weber@example.com'], browser: ['Safari', '17.5'], os: ['iOS', '17.5'],
    device: ['mobile', 'iPhone 15', 'Apple'], place: ['DE', 'Germany', 'Berlin', 'EU', 52.52, 13.4],
    ip: '198.51.100.77', release: '2.3.9', pages: ['/orders', '/orders/9921'],
    signs: [
      { page: '/orders', name: 'Open order', selector: 'button[data-testid="open-order"]', kinds: ['rage_click', 'dead_click'] },
    ],
  },
  {
    id: MOCK_RUM.session.synthetic, app: 'storefront', ago: 6, lasted: 1, synthetic: true,
    user: null, browser: ['Chrome', '126.0'], os: ['Linux', '6.8'],
    device: ['desktop', '', ''], place: ['DE', 'Germany', 'Frankfurt', 'EU', 50.11, 8.68],
    ip: '192.0.2.200', release: '2.4.0', pages: ['/checkout'],
  },
];

/** Ids that look like the ones SDKs make and are the same from run to run. */
function uuid(kind: number, visit: number, index: number): string {
  const hex = (value: number, width: number) => value.toString(16).padStart(width, '0').slice(-width);

  return [
    hex(0x5a000000 + kind * 0x10000 + visit * 0x100 + index, 8),
    hex(0x7c10 + kind, 4),
    `4${hex(visit * 16 + index, 3)}`,
    `8${hex(0x2f1 + index, 3)}`,
    hex(0xa00000000000 + kind * 0x100000 + visit * 0x1000 + index, 12),
  ].join('-');
}

/** The script a long task of a page is put down to: the checkout runs its own, a product page renders and loads tags. */
function scriptOf(path: string, k: number): Script {
  if (path === '/checkout') return k % 4 === 3 ? SCRIPTS.hydrate : SCRIPTS.validate;
  if (path === '/products/sku-104') return k % 2 === 0 ? SCRIPTS.render : k % 4 === 1 ? SCRIPTS.vendor : SCRIPTS.tag;

  return SCRIPTS.hydrate;
}

/**
 * The id of the issue an error belongs to: sixteen hexadecimal digits, the same for errors of one
 * application that have the same type and message, as the server's are.
 */
export function issueIdOf(application: string, type: string, message: string): string {
  const hash = (seed: number) => {
    let value = seed;
    for (const char of `${application}\u0000${type}\u0000${message}`) value = (value * 31 + char.charCodeAt(0)) >>> 0;

    return value.toString(16).padStart(8, '0');
  };

  return `${hash(7)}${hash(11)}`;
}

const empty = (): Record<RumType, RumRow[]> => ({
  sessions: [],
  views: [],
  actions: [],
  errors: [],
  resources: [],
  long_tasks: [],
  vitals: [],
  logs: [],
});

/** What a visit says about itself on every event it sends. */
function dimensions(visit: Visit, start: number): Record<string, unknown> {
  const web = visit.browser !== null;

  return {
    'application.id': visit.app,
    service: web ? 'storefront-web' : 'storefront-mobile',
    env: 'prod',
    version: visit.release,
    source: web ? 'browser' : 'flutter',
    'session.id': visit.id,
    'session.type': visit.synthetic ? 'synthetics' : 'user',
    ...(visit.synthetic
      ? { 'synthetics.test_id': MOCK_RUM.check.id, 'synthetics.result_id': MOCK_RUM.check.result, 'synthetics.location_id': MOCK_RUM.check.location }
      : {}),
    'session.has_replay': visit.replay === true,
    'session.start': Math.round(start / 1_000),
    ...(visit.user
      ? { 'usr.id': visit.user[0], 'usr.name': visit.user[1], 'usr.email': visit.user[2] }
      : { 'usr.anonymous_id': `anon-${visit.id.slice(0, 8)}` }),
    ...(visit.browser
      ? { 'browser.name': visit.browser[0], 'browser.version': visit.browser[1], 'browser.version_major': visit.browser[1].split('.')[0] }
      : {}),
    'os.name': visit.os[0],
    'os.version': visit.os[1],
    'device.type': visit.device[0],
    ...(visit.device[1] ? { 'device.model': visit.device[1], 'device.brand': visit.device[2] } : {}),
    'geo.country_iso_code': visit.place[0],
    'geo.country': visit.place[1],
    'geo.city': visit.place[2],
    'geo.continent_code': visit.place[3],
    'geo.latitude': visit.place[4],
    'geo.longitude': visit.place[5],
    'network.client.ip': visit.ip,
    'sdk.name': web ? 'browser-rum' : 'flutter-rum',
    'sdk.version': web ? '1.0.0' : '0.9.0',
    'sdk.session_sample_rate': 100,
    ...(web
      ? {
          'display.viewport.width': visit.device[0] === 'mobile' ? 390 : 1280,
          'display.viewport.height': visit.device[0] === 'mobile' ? 844 : 720,
        }
      : {}),
    ...(web ? { feature_flags: flagsOf(visit) } : {}),
  };
}

/** The flags a visit evaluated: half of the browsers have the new checkout. */
function flagsOf(visit: Visit): Record<string, unknown> {
  const treated = visit.rage === true || visit.problems !== undefined;

  return { checkout_v2: treated ? 'treatment' : 'control', new_nav: visit.release === '2.4.0' };
}

/** Where a named element stands in the page the mock records. */
const ELEMENT_PATHS: Record<string, string> = { 'Pay now': 'body>button', Checkout: 'body>h1' };

/**
 * What a Chromium browser says about the levels of a view: the element each was set by and, of the
 * two that are times, the parts they are made of. The other browsers report none of it.
 */
function attribution(spec: PageSpec): Record<string, unknown> {
  const rest = spec.lcp - spec.loading * 0.2;

  return {
    'view.performance.lcp.target_selector': spec.elements.lcp,
    ...(spec.elements.lcpFile ? { 'view.performance.lcp.resource_url': spec.elements.lcpFile } : {}),
    'view.performance.lcp.sub_parts.load_delay': ns(rest * 0.2),
    'view.performance.lcp.sub_parts.load_time': ns(rest * 0.3),
    'view.performance.lcp.sub_parts.render_delay': ns(rest * 0.5),
    'view.performance.inp.target_selector': spec.elements.inp,
    'view.performance.inp.sub_parts.input_delay': ns(spec.inp * 0.2),
    'view.performance.inp.sub_parts.processing_duration': ns(spec.inp * 0.55),
    'view.performance.inp.sub_parts.presentation_delay': ns(spec.inp * 0.25),
    'view.performance.cls.target_selector': spec.elements.cls,
  };
}

/** The rows of the mock organization, newest first within each type. */
/** What the figures of an application are made of: the rows of the sessions people ran. */
export function mockRumData(now: number): Record<RumType, RumRow[]> {
  return Object.fromEntries(
    Object.entries(mockRumEveryRow(now)).map(([type, rows]) => [type, rows.filter((row) => row['session.type'] === 'user')]),
  ) as Record<RumType, RumRow[]>;
}

/** Every row there is, those of a run of a synthetic check too. */
export function mockRumEveryRow(now: number): Record<RumType, RumRow[]> {
  const data = empty();

  VISITS.forEach((visit, n) => {
    const start = now - visit.ago * MINUTE;
    const length = visit.lasted * MINUTE;
    const end = start + length;
    const common = dimensions(visit, start);
    const slice = length / visit.pages.length;
    const own = empty();

    visit.pages.forEach((path, i) => {
      const spec = PAGES[path] as PageSpec;
      const at = start + Math.round(i * slice);
      const view = uuid(1, n, i);
      const context = {
        'view.id': view,
        'view.url': `https://shop.example.com${path}`,
        'view.url_host': 'shop.example.com',
        'view.url_path': path,
        'view.url_path_group': spec.group,
        'view.name': spec.name,
      };

      // What the visitor does comes after the page has loaded, and what the last action asks of
      // the API (a call that fails) comes right after it.
      const taps = spec.taps.flatMap((name) => (visit.rage === true && name === 'Pay now' ? [name, name, name] : [name]));
      const afterTaps = at + 3 * SECOND + taps.length * 4 * SECOND;
      const loads = Array.from({ length: spec.requests }, (_, k) => REQUESTS[(k + i) % REQUESTS.length] as (typeof REQUESTS)[number]).map(
        (request, k) => ({
          ...request,
          path: request.path === '{page}' ? path : request.path,
          trace: false,
          at: at + 80_000 + k * 190_000,
        }),
      );
      const calls = (visit.failures ?? [])
        .filter((failure) => failure.page === path)
        .map((failure, k) => ({ ...failure, type: 'fetch', size: 0, trace: failure.trace === true, at: afterTaps + k * 250_000 }));

      [...loads, ...calls].forEach((request, k) => {
        const ms = request.ms * (1 + ((k * 7) % 5) / 10);
        const traced = request.trace;

        own.resources.push({
          timestamp: request.at,
          ...common,
          ...context,
          'resource.id': uuid(4, n, i * 20 + k),
          'resource.type': request.type,
          'resource.method': request.method,
          'resource.url': `https://${request.host}${request.path}`,
          'resource.url_host': request.host,
          'resource.url_path': request.path,
          'resource.url_path_group': request.path,
          'resource.status_code': request.status,
          'resource.duration': ns(ms),
          'resource.size': request.size,
          'resource.transfer_size': Math.round(request.size * 0.8),
          'resource.protocol': 'h2',
          // A style and the first script block the first paint; the files of the static hosts come from the cache every third time.
          ...(request.type === 'css' || (request.type === 'js' && k === 2)
            ? { 'resource.render_blocking_status': 'blocking' }
            : request.type === 'js'
              ? { 'resource.render_blocking_status': 'non-blocking' }
              : {}),
          ...(['css', 'js', 'font', 'image'].includes(request.type) ? { 'resource.delivery_type': (k + n) % 3 === 0 ? 'cache' : 'other' } : {}),
          'resource.dns.duration': ns(Math.min(12, ms / 10)),
          'resource.connect.duration': ns(Math.min(30, ms / 6)),
          'resource.ssl.duration': ns(Math.min(18, ms / 10)),
          'resource.first_byte.duration': ns(ms * 0.6),
          'resource.download.duration': ns(ms * 0.25),
          ...(traced ? { 'resource.trace_id': MOCK_RUM.trace, 'resource.span_id': MOCK_RUM.span } : {}),
        });
      });

      // The actions, and the signs of a frustrated visitor among them.
      taps.forEach((name, k) => {
        const raging = visit.rage === true && name === 'Pay now';
        const first = k === 0 || taps[k - 1] !== name;
        const frustrated = raging && !first;

        own.actions.push({
          timestamp: at + 3 * SECOND + k * (raging ? 300_000 : 4 * SECOND),
          ...common,
          ...context,
          'action.id': uuid(2, n, i * 20 + k),
          'action.type': visit.browser ? 'click' : 'tap',
          'action.target.name': name,
          'action.target.selector': `button[data-testid="${name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}"]`,
          ...(visit.browser
            ? {
                'action.target.path': ELEMENT_PATHS[name] ?? `body>main>a:nth-of-type(${(k % 3) + 1})`,
                'action.target.width': 200,
                'action.target.height': 40,
                'action.target.offset.x': 40 + (k % 4) * 40,
                'action.target.offset.y': 10 + (k % 3) * 10,
                'action.page_position.x': 96 + (k % 4) * 40,
                'action.page_position.y': 118 + (k % 3) * 400,
              }
            : {}),
          'action.loading_time': ns(60 + k * 40),
          ...(frustrated ? { 'action.frustration.type': ['rage_click'], 'action.frustration.count': 1 } : { 'action.frustration.count': 0 }),
        });
      });

      // The signs an SDK read on this page come after the taps, each action a little after the one before it.
      (visit.signs ?? [])
        .filter((sign) => sign.page === path)
        .forEach((sign, j) => {
          for (let k = 0; k < (sign.times ?? 1); k += 1) {
            own.actions.push({
              timestamp: afterTaps - 2 * SECOND + j * 1_500_000 + k * 300_000,
              ...common,
              ...context,
              'action.id': uuid(2, n, i * 20 + 10 + j * 4 + k),
              'action.type': visit.browser ? 'click' : 'tap',
              ...(sign.name ? { 'action.target.name': sign.name } : {}),
              ...(sign.selector ? { 'action.target.selector': sign.selector } : {}),
              'action.loading_time': ns(40),
              'action.frustration.type': [...sign.kinds],
              'action.frustration.count': sign.kinds.length,
              'action.error.count': sign.kinds.some((kind) => kind.startsWith('error_')) ? 1 : 0,
            });
          }
        });

      (visit.problems ?? [])
        .filter((problem) => problem.page === path)
        .forEach((problem, k) => {
          own.errors.push({
            timestamp: afterTaps + 600_000 + k * 2 * SECOND,
            ...common,
            ...context,
            'error.id': uuid(3, n, i * 20 + k),
            'error.message': problem.message,
            'error.type': problem.type,
            'error.source': problem.source,
            'error.handling': problem.handling,
            'error.issue_id': issueIdOf(visit.app, problem.type, problem.message),
            'error.is_crash': false,
            'error.stack':
              problem.source === 'source'
                ? `${problem.type}: ${problem.message}\n    at renderTotals (https://cdn.example.com/assets/app.4f3a.js:1:20431)\n    at Object.onClick (https://cdn.example.com/assets/app.4f3a.js:1:19877)`
                : `${problem.type}: ${problem.message}`,
            ...(problem.source === 'network'
              ? { 'error.resource.method': 'POST', 'error.resource.status_code': 500, 'error.resource.url': `https://${API}/api/orders` }
              : {}),
            // A source map restored the frames of the errors thrown in the application's own code.
            ...(problem.source === 'source'
              ? {
                  'error.frames': [
                    {
                      file: 'https://cdn.example.com/assets/app.4f3a.js',
                      function: 'a',
                      line: 1,
                      column: 20431,
                      original_file: 'src/checkout/Totals.tsx',
                      original_function: 'renderTotals',
                      original_line: 88,
                      original_column: 14,
                    },
                    {
                      file: 'https://cdn.example.com/assets/app.4f3a.js',
                      function: 'b',
                      line: 1,
                      column: 19877,
                      original_file: 'node_modules/react-dom/cjs/react-dom.production.min.js',
                      original_function: 'onClick',
                      original_line: 12,
                    },
                  ],
                  'error.symbolication': { status: 'completed', translated_frames: 2, attempted_frames: 2, truncated: false },
                }
              : {}),
          });
        });

      // A heavy page blocks the main thread; a middling one now and then. A Chromium browser says
      // which script ran the longest, the others only that something did.
      const blocked = spec.lcp > 2_500 ? 24 : spec.lcp > 1_500 ? 3 : 0;
      const frames = visit.browser?.[0] === 'Chrome' || visit.browser?.[0] === 'Edge';

      for (let k = 0; k < blocked; k += 1) {
        const ms = 60 + ((n * 7 + i * 13 + k * 29) % 260);
        const start = 300 + Math.round((k * Math.max(spec.loading, 1_000) * 0.9) / blocked);
        const script = scriptOf(path, k);

        own.long_tasks.push({
          timestamp: at + start * 1_000,
          ...common,
          ...context,
          'long_task.id': uuid(5, n, i * 40 + k),
          'long_task.entry_type': frames ? 'long-animation-frame' : 'long-task',
          'long_task.duration': ns(ms),
          'long_task.start_time': ns(start),
          'long_task.blocking_duration': ns(ms - 50),
          'long_task.is_frozen_frame': false,
          ...(frames
            ? {
                'long_task.scripts': [
                  { duration: ns(ms * 0.9), source_url: script.url, source_function_name: script.function, invoker_type: script.invoker },
                  { duration: ns(ms * 0.1), source_url: SCRIPTS.tag.url, source_function_name: '', invoker_type: 'classic-script' },
                ],
                'long_task.top_script.source_url': script.url,
                ...(script.function ? { 'long_task.top_script.function': script.function } : {}),
                'long_task.top_script.invoker_type': script.invoker,
              }
            : {}),
        });
      }

      const active = visit.active === true && i === visit.pages.length - 1;
      const chromium = visit.browser?.[0] === 'Chrome' || visit.browser?.[0] === 'Edge';
      const inView = (rows: RumRow[]) => rows.filter((row) => row['view.id'] === view).length;

      own.views.push({
        timestamp: at,
        ...common,
        ...context,
        'view.revision': 3 + inView(own.actions),
        'view.is_active': active,
        'view.loading_type': i === 0 ? 'initial_load' : 'route_change',
        'view.loading_time': ns(spec.loading),
        'view.time_spent': Math.round(slice * 1_000),
        'view.first_byte': ns(spec.loading * 0.2),
        'view.dom_content_loaded': ns(spec.loading * 0.7),
        'view.load_event': ns(spec.loading * 0.95),
        'view.performance.fcp.timestamp': ns(spec.lcp * 0.6),
        'view.performance.lcp.timestamp': ns(spec.lcp),
        'view.performance.inp.duration': ns(spec.inp),
        'view.performance.cls.score': spec.cls,
        ...(chromium ? attribution(spec) : {}),
        'view.action.count': inView(own.actions),
        'view.error.count': inView(own.errors),
        'view.resource.count': inView(own.resources),
        'view.long_task.count': inView(own.long_tasks),
        'view.frustration.count': own.actions.filter((row) => row['view.id'] === view && Number(row['action.frustration.count']) > 0).length,
        ...(visit.browser
          ? {
              'view.custom_timings': { hero_visible: ns(spec.lcp * 0.8) },
              'view.scroll.max_depth': 720 + ((n + i) % 5) * 400,
              'view.scroll.max_depth_scroll_top': ((n + i) % 5) * 400,
              'view.scroll.max_scroll_height': 2_400,
              'view.scroll.max_scroll_height_time': ns(spec.loading),
            }
          : {}),
      });

      // What the page wrote while it was there.
      const logged = [
        { level: 'info', message: `opened ${path}`, logger: 'router', origin: 'logger' },
        ...own.errors
          .filter((row) => row['view.id'] === view)
          .map((row) => ({ level: 'error', message: String(row['error.message']), logger: 'console', origin: 'console' })),
      ];

      logged.forEach((log, k) => {
        own.logs.push({
          timestamp: at + 2 * SECOND + k * SECOND,
          ...common,
          ...context,
          'log.id': uuid(6, n, i * 20 + k),
          level: log.level,
          message: log.message,
          'log.logger': log.logger,
          'log.origin': log.origin,
        });
      });
    });

    const [first, last] = [visit.pages[0] as string, visit.pages[visit.pages.length - 1] as string];
    const page = (path: string) => ({ url: `https://shop.example.com${path}`, path, name: (PAGES[path] as PageSpec).name });
    const landing = page(first);
    const exit = page(last);

    own.sessions.push({
      timestamp: start,
      ...common,
      'session.revision': 4 + visit.pages.length,
      'session.last_activity': Math.round((visit.active ? now - 30 * SECOND : end) / 1_000),
      'session.duration': Math.round(length * 1_000),
      'session.view.count': own.views.length,
      'session.action.count': own.actions.length,
      'session.error.count': own.errors.length,
      'session.resource.count': own.resources.length,
      'session.long_task.count': own.long_tasks.length,
      'session.frustration.count': own.actions.filter((row) => Number(row['action.frustration.count']) > 0).length,
      'session.crash.count': 0,
      'session.landing_view.url': landing.url,
      'session.landing_view.url_path': landing.path,
      'session.landing_view.name': landing.name,
      'session.exit_view.url': exit.url,
      'session.exit_view.url_path': exit.path,
      'session.exit_view.name': exit.name,
    });

    for (const type of Object.keys(own) as RumType[]) data[type].push(...own[type]);
  });

  for (const rows of Object.values(data)) {
    rows.sort((a, b) => b.timestamp - a.timestamp);
  }

  return data;
}
