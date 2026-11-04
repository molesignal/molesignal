import { NOW_MS, RUN_ID, spanId, traceId } from './shared.mjs';
import {
  CLIENTS,
  registerRumApplication,
  sendRumSession,
} from '../rum_seed_events.mjs';

const APPLICATION = { id: 'shop-web', name: 'Shop web' };
const ORIGIN = 'https://shop.example.test';

export async function seedRum(api) {
  const transport = {
    get: (path) => api.request('GET', path),
    post: (path, body, headers) => api.request('POST', path, body, headers),
  };
  await registerRumApplication(transport, APPLICATION);

  const ipAddresses = [
    '203.0.113.42',
    '198.51.100.18',
    '192.0.2.73',
    '203.0.113.91',
  ];
  const journeys = [
    ['/', '/collections/summer', '/products/running-shoes', '/checkout'],
    ['/', '/search', '/products/travel-pack', '/checkout'],
    ['/campaign/member-week', '/products/studio-headphones', '/checkout'],
    ['/', '/account/orders'],
  ];
  const failures = [
    {
      type: 'CheckoutApiError',
      message: 'Checkout request failed with an upstream error',
      fingerprint: 'checkout-api-failure',
    },
    {
      type: 'PaymentTimeout',
      message: 'Payment authorization exceeded the expected response time',
      fingerprint: 'checkout-payment-failure',
    },
  ];
  const frames = [
    { file: 'checkout.js', function: 'submitOrder', line: 184, column: 16 },
    { file: 'api-client.js', function: 'request', line: 72, column: 9 },
  ];

  const totals = { sessions: 0, events: 0, errors: 0, replays: 0 };

  for (let index = 0; index < 96; index += 1) {
    const failed = index % 17 === 0;
    const trace = traceId(index % 1_200);
    const events = await sendRumSession(transport, {
      application: APPLICATION.id,
      service: 'shop-web',
      env: 'production',
      version: index % 5 === 0 ? '2026.7.31' : '2026.7.30',
      origin: ORIGIN,
      sessionId: `web_${RUN_ID}_${String(index).padStart(3, '0')}`,
      userId: `customer-${4_000 + index}`,
      startedMs: NOW_MS - (index + 2) * 160_000,
      client: CLIENTS[index % CLIENTS.length],
      ip: ipAddresses[index % ipAddresses.length],
      journey: journeys[index % journeys.length],
      metrics: (page) => ({
        lcp: 1_320 + (index % 11) * 95 + (page === '/checkout' ? 520 : 0),
        inp: 24 + (index % 9) * 7,
        cls: Number((0.018 + (index % 8) * 0.008).toFixed(3)),
        ttfb: 105 + (index % 12) * 24,
      }),
      interaction: index % 23 === 0 ? 'rage_click' : 'click',
      request: {
        method: 'POST',
        path: '/api/orders',
        status: failed ? 502 : 201,
        durationMs: failed ? 2_350 : 185 + (index % 13) * 22,
        trace: { traceId: trace, spanId: spanId(trace, 'browser-fetch') },
      },
      errors: failed
        ? [{ ...failures[Math.floor(index / 17) % failures.length], frames }]
        : [],
      longTaskMs: index % 7 === 0 ? 140 + (index % 5) * 30 : undefined,
      vital:
        index % 5 === 0
          ? { name: 'checkout_form_ready', durationMs: 620 + (index % 9) * 40 }
          : undefined,
      replay: index < 24,
    });

    totals.sessions += 1;
    totals.events += events.length;
    totals.errors += events.filter((event) => event.type === 'error').length;
    totals.replays += index < 24 ? 1 : 0;
  }

  return { application: APPLICATION.id, ...totals };
}
