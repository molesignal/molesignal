import { NOW_MS, RUN_ID, spanId, traceId } from './shared.mjs';
import { buildRrwebReplay } from '../rum_replay_fixture.mjs';

export async function seedRum(api) {
  const countries = ['US', 'US', 'DE', 'GB', 'BR', 'JP', 'AU'];
  const browsers = ['Chrome', 'Chrome', 'Safari', 'Edge', 'Firefox'];
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
  const sessions = Array.from({ length: 96 }, (_, index) => {
    const started = (NOW_MS - (index + 2) * 160_000) * 1_000;
    const journey = journeys[index % journeys.length];
    return {
      timestamp: started,
      session_id: `web_${RUN_ID}_${String(index).padStart(3, '0')}`,
      user_id: `customer-${4_000 + index}`,
      ip_address: ipAddresses[index % ipAddresses.length],
      country: countries[index % countries.length],
      browser: browsers[index % browsers.length],
      application: 'shop-web',
      environment: 'production',
      version: index % 5 === 0 ? '2026.7.31' : '2026.7.30',
      device: index % 4 === 0 ? 'mobile' : 'desktop',
      os: index % 4 === 0 ? 'iOS' : index % 3 === 0 ? 'Windows' : 'macOS',
      last_page: journey[journey.length - 1],
      duration_ms: 74_000 + (index % 18) * 9_500,
      error_count: index % 17 === 0 ? 1 : 0,
      started_at_micros: started,
    };
  });
  const actions = sessions.flatMap((session, index) => {
    const journey = journeys[index % journeys.length];
    const common = {
      application: session.application,
      environment: session.environment,
      version: session.version,
      country: session.country,
      browser: session.browser,
      device: session.device,
      os: session.os,
      session_id: session.session_id,
    };
    const views = journey.map((page, pageIndex) => ({
      ...common,
      timestamp:
        session.started_at_micros + (pageIndex * 13 + 1) * 1_000_000,
      ts_micros:
        session.started_at_micros + (pageIndex * 13 + 1) * 1_000_000,
      type: 'view',
      name: `View ${page}`,
      page,
      lcp_ms: 1_320 + (index % 11) * 95 + (page === '/checkout' ? 520 : 0),
      fid_ms: 24 + (index % 9) * 7,
      cls: Number((0.018 + (index % 8) * 0.008).toFixed(3)),
      ttfb_ms: 105 + (index % 12) * 24,
      payload: { path: page, release: session.version },
      service: 'api-gateway',
    }));
    const failed = session.error_count > 0;
    return [
      ...views,
      {
        ...common,
        timestamp: session.started_at_micros + 58_000_000,
        ts_micros: session.started_at_micros + 58_000_000,
        type: index % 23 === 0 ? 'rage_click' : 'resource',
        name:
          index % 23 === 0
            ? 'Repeated checkout submission'
            : 'POST /api/orders',
        page: journey[journey.length - 1],
        url: '/api/orders',
        duration_ms: failed ? 2_350 : 185 + (index % 13) * 22,
        status: failed ? 502 : 201,
        payload: { method: 'POST', path: '/checkout' },
        service: 'api-gateway',
        trace_id: traceId(index % 1_200),
        parent_span_id: spanId(
          traceId(index % 1_200),
          'gateway-server',
        ),
      },
    ];
  });
  const errors = sessions
    .filter((session) => session.error_count > 0)
    .map((session, index) => ({
      timestamp: session.started_at_micros + 58_000_000,
      session_id: session.session_id,
      user_id: session.user_id,
      application: session.application,
      environment: session.environment,
      version: session.version,
      page: '/checkout',
      error_type: index % 2 === 0 ? 'CheckoutApiError' : 'PaymentTimeout',
      fingerprint: `checkout-${index % 2 === 0 ? 'api' : 'payment'}-failure`,
      message:
        index % 2 === 0
          ? 'Checkout request failed with an upstream error'
          : 'Payment authorization exceeded the expected response time',
      error: {
        stack: [
          {
            file: 'checkout.js',
            function: 'submitOrder',
            line: 184,
            column: 16,
          },
          {
            file: 'api-client.js',
            function: 'request',
            line: 72,
            column: 9,
          },
        ],
      },
    }));
  await api.post('/rum/sessions', sessions);
  await api.post('/rum/actions', actions);
  await api.post('/rum/errors', errors);
  for (const [index, session] of sessions.slice(0, 24).entries()) {
    const journey = journeys[index % journeys.length];
    await api.post('/rum/replay', {
      session_id: session.session_id,
      seq: 1,
      events: buildRrwebReplay({
        startMs: Math.floor(session.started_at_micros / 1_000),
        origin: 'https://shop.example.test',
        journey,
        interaction: index % 23 === 0 ? 'rage_click' : 'click',
        errorLabel: session.error_count > 0 ? 'POST /api/orders · 502' : undefined,
        viewport:
          session.device === 'mobile'
            ? { width: 390, height: 844 }
            : { width: 1440, height: 900 },
      }),
    });
  }
  return {
    sessions: sessions.length,
    actions: actions.length,
    errors: errors.length,
  };
}
