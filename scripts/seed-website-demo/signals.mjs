import {
  NOW_MS,
  SERVICES,
  SERVICE_NAMES,
  spanId,
  traceId,
  versionFor,
} from './shared.mjs';

function makeLogs() {
  const templates = {
    'api-gateway': ['request routed to order-service', 'rate limit budget checked'],
    'order-service': ['order workflow completed', 'order state persisted'],
    'inventory-service': [
      'inventory reservation confirmed',
      'stock snapshot refreshed',
    ],
    'payment-service': [
      'payment authorization completed',
      'provider response validated',
    ],
    'user-service': ['customer profile loaded', 'loyalty tier resolved'],
    'notification-service': [
      'order confirmation delivered',
      'notification preference applied',
    ],
  };
  return Array.from({ length: 720 }, (_, index) => {
    const service = SERVICE_NAMES[index % SERVICE_NAMES.length];
    const error = index % 47 === 0;
    const warning = !error && index % 19 === 0;
    const timestamp = (NOW_MS - (719 - index) * 30_000) * 1_000;
    return {
      _timestamp: timestamp,
      timestamp,
      level: error ? 'error' : warning ? 'warn' : 'info',
      message: error
        ? service === 'payment-service'
          ? 'payment provider timeout; retry scheduled'
          : `${service} request exceeded latency objective`
        : templates[service][index % 2],
      service,
      env: 'production',
      region: index % 4 === 0 ? 'eu-west-1' : 'us-east-1',
      host: `${service}-prod-${1 + (index % SERVICES[service].instances)}`,
      trace_id: traceId(index % 1_200),
      span_id: spanId(traceId(index % 1_200), `${service}-server`),
      path:
        service === 'api-gateway'
          ? '/api/orders'
          : `/${service.replace('-service', '')}`,
      method: index % 3 === 0 ? 'POST' : 'GET',
      status_code: error ? 504 : warning ? 429 : 200,
      latency_ms: error ? 1_850 + (index % 7) * 80 : 42 + (index % 21) * 9,
      user_id: `customer-${1_000 + (index % 240)}`,
      build: versionFor(service, index),
      error,
    };
  });
}

function metricSeries(service, route, status, start, step, count = 72) {
  return Array.from({ length: count }, (_, index) => ({
    _timestamp: (NOW_MS - (count - 1 - index) * 300_000) * 1_000,
    value: start + index * step + (index % 7),
    service,
    route,
    method:
      route.includes('orders') || route.includes('authorize') ? 'POST' : 'GET',
    status,
    env: 'production',
    region: index % 5 === 0 ? 'eu-west-1' : 'us-east-1',
    instance: `${service}-prod-${1 + (index % SERVICES[service].instances)}`,
  }));
}

function gaugeSeries(service, base, jitter, count = 72) {
  return Array.from({ length: count }, (_, index) => ({
    _timestamp: (NOW_MS - (count - 1 - index) * 300_000) * 1_000,
    value: Number(
      (
        base +
        Math.sin(index / 4) * jitter +
        (index % 5) * jitter * 0.08
      ).toFixed(3),
    ),
    service,
    host: `${service}-prod-${1 + (index % SERVICES[service].instances)}`,
    env: 'production',
    region: index % 5 === 0 ? 'eu-west-1' : 'us-east-1',
  }));
}

export async function seedSignals(api) {
  const logs = makeLogs();
  await api.post('/ingest/logs/app_logs', logs);

  const requestMetrics = [
    ...metricSeries('api-gateway', '/api/orders', '201', 1_240_000, 610),
    ...metricSeries('api-gateway', '/api/orders', '502', 11_200, 9),
    ...metricSeries('order-service', '/orders', '201', 1_180_000, 590),
    ...metricSeries('payment-service', '/authorize', '200', 1_060_000, 540),
    ...metricSeries('payment-service', '/authorize', '504', 34_000, 21),
    ...metricSeries(
      'inventory-service',
      '/reservations',
      '201',
      1_150_000,
      575,
    ),
  ];
  const durations = [
    ...gaugeSeries('api-gateway', 185, 48).map((row) => ({
      ...row,
      route: '/api/orders',
      quantile: 'p95',
    })),
    ...gaugeSeries('order-service', 245, 72).map((row) => ({
      ...row,
      route: '/orders',
      quantile: 'p95',
    })),
    ...gaugeSeries('payment-service', 620, 180).map((row) => ({
      ...row,
      route: '/authorize',
      quantile: 'p95',
    })),
  ];
  const cpu = SERVICE_NAMES.flatMap((service, index) =>
    gaugeSeries(service, 0.28 + index * 0.06, 0.09),
  );
  const memory = SERVICE_NAMES.flatMap((service, index) =>
    gaugeSeries(service, 420 + index * 115, 55),
  );
  await api.post('/ingest/metrics/http_requests_total', requestMetrics);
  await api.post('/ingest/metrics/http_request_duration_ms', durations);
  await api.post('/ingest/metrics/process_cpu_usage', cpu);
  await api.post('/ingest/metrics/memory_usage_mb', memory);
  return {
    logs: logs.length,
    metricSamples:
      requestMetrics.length + durations.length + cpu.length + memory.length,
  };
}
