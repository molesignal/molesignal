import {
  NOW_MS,
  SERVICES,
  kv,
  ns,
  spanId,
  traceId,
  versionFor,
} from './shared.mjs';

function exceptionEvent(type, message, stack, atMs) {
  return {
    timeUnixNano: ns(atMs),
    name: 'exception',
    attributes: [
      kv('exception.type', type),
      kv('exception.message', message),
      kv('exception.stacktrace', stack),
    ],
  };
}

function otlpSpan({
  trace,
  id,
  parent = '',
  name,
  kind,
  startMs,
  durationMs,
  attributes = [],
  error,
}) {
  return {
    traceId: trace,
    spanId: id,
    ...(parent ? { parentSpanId: parent } : {}),
    name,
    kind,
    startTimeUnixNano: ns(startMs),
    endTimeUnixNano: ns(startMs + durationMs),
    attributes,
    status: error ? { code: 2, message: error.message } : { code: 1 },
    ...(error
      ? {
          events: [
            exceptionEvent(
              error.type,
              error.message,
              error.stack,
              startMs + durationMs - 1,
            ),
          ],
        }
      : {}),
  };
}

function resourceGroup(service, version, spans) {
  const config = SERVICES[service];
  return {
    resource: {
      attributes: [
        kv('service.namespace', 'shop'),
        kv('service.name', service),
        kv('service.version', version),
        kv('service.instance.id', `${service}-prod-${1 + (spans.length % config.instances)}`),
        kv('deployment.environment.name', 'production'),
        kv('telemetry.sdk.name', 'opentelemetry'),
        kv('telemetry.sdk.version', config.sdkVersion),
        kv('telemetry.sdk.language', config.language),
        kv('cloud.region', 'us-east-1'),
      ],
    },
    scopeSpans: [
      {
        scope: { name: `shop.${service}`, version: '1.0.0' },
        spans,
      },
    ],
  };
}

function addSpan(groups, service, version, span) {
  const key = `${service}@${version}`;
  const group = groups.get(key) ?? { service, version, spans: [] };
  group.spans.push(span);
  groups.set(key, group);
}

function positiveIntegerEnv(name, fallback) {
  const value = Number.parseInt(process.env[name] ?? '', 10);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

function buildCommerceTrace(groups, index, startMs) {
  const trace = traceId(index);
  const candidate = index % 5 === 0;
  const paymentError = index % (candidate ? 13 : 29) === 0;
  const inventoryError = index % 97 === 0;
  const notificationError = index % 43 === 0;
  const orderError = paymentError || inventoryError;
  const gatewayError = orderError && index % 3 !== 0;
  const slow = index % 41 === 0;

  const gatewayRoot = spanId(trace, 'gateway-server');
  const gatewayClient = spanId(trace, 'gateway-order-client');
  const orderServer = spanId(trace, 'order-server');
  const inventoryClient = spanId(trace, 'inventory-client');
  const inventoryServer = spanId(trace, 'inventory-server');
  const paymentClient = spanId(trace, 'payment-client');
  const paymentServer = spanId(trace, 'payment-server');
  const stripeClient = spanId(trace, 'stripe-client');
  const databaseClient = spanId(trace, 'postgres-client');
  const producer = spanId(trace, 'orders-producer');
  const notificationConsumer = spanId(trace, 'notification-consumer');
  const userServer = spanId(trace, 'user-server');

  const orderFailure = orderError
    ? inventoryError
      ? {
          type: 'InventoryReservationError',
          message: 'Inventory reservation failed for requested item',
          stack: 'orders::inventory::reserve\norders::workflow::submit\nruntime::poll',
        }
      : {
          type: 'PaymentGatewayTimeout',
          message: 'Payment provider timed out while authorizing order',
          stack: 'orders::payments::authorize\norders::workflow::submit\nruntime::poll',
        }
    : null;
  const paymentFailure = paymentError
    ? {
        type: 'PaymentGatewayTimeout',
        message: 'Payment provider timed out after retry budget was exhausted',
        stack: 'payments::stripe::authorize\npayments::retry::execute\nruntime::poll',
      }
    : null;

  addSpan(
    groups,
    'api-gateway',
    versionFor('api-gateway', index),
    otlpSpan({
      trace,
      id: gatewayRoot,
      name: 'POST /api/orders',
      kind: 2,
      startMs,
      durationMs: slow ? 2_450 : 180 + (index % 18) * 9,
      attributes: [
        kv('http.request.method', 'POST'),
        kv('http.route', '/api/orders'),
        kv('http.response.status_code', gatewayError ? 502 : 201),
      ],
      error: gatewayError
        ? {
            type: 'UpstreamTimeout',
            message: 'Order workflow did not complete before gateway timeout',
            stack: 'gateway::orders::create\ngateway::proxy::request\nruntime::serve',
          }
        : null,
    }),
  );
  addSpan(
    groups,
    'api-gateway',
    versionFor('api-gateway', index),
    otlpSpan({
      trace,
      id: gatewayClient,
      parent: gatewayRoot,
      name: 'POST /orders',
      kind: 3,
      startMs: startMs + 8,
      durationMs: slow ? 2_310 : 145 + (index % 16) * 8,
      attributes: [kv('peer.service', 'order-service')],
      error: orderFailure,
    }),
  );
  addSpan(
    groups,
    'order-service',
    versionFor('order-service', index),
    otlpSpan({
      trace,
      id: orderServer,
      parent: gatewayClient,
      name: 'POST /orders',
      kind: 2,
      startMs: startMs + 12,
      durationMs: slow ? 2_280 : 132 + (index % 21) * 7,
      attributes: [
        kv('http.request.method', 'POST'),
        kv('http.route', '/orders'),
        kv('http.response.status_code', orderError ? 503 : 201),
      ],
      error: orderFailure,
    }),
  );
  addSpan(
    groups,
    'order-service',
    versionFor('order-service', index),
    otlpSpan({
      trace,
      id: databaseClient,
      parent: orderServer,
      name: 'SELECT orders',
      kind: 3,
      startMs: startMs + 24,
      durationMs: 18 + (index % 15),
      attributes: [kv('db.system.name', 'postgresql'), kv('db.operation.name', 'SELECT')],
    }),
  );
  addSpan(
    groups,
    'order-service',
    versionFor('order-service', index),
    otlpSpan({
      trace,
      id: inventoryClient,
      parent: orderServer,
      name: 'reserve',
      kind: 3,
      startMs: startMs + 48,
      durationMs: 42 + (index % 17) * 3,
      attributes: [kv('peer.service', 'inventory-service')],
      error: inventoryError ? orderFailure : null,
    }),
  );
  addSpan(
    groups,
    'inventory-service',
    versionFor('inventory-service', index),
    otlpSpan({
      trace,
      id: inventoryServer,
      parent: inventoryClient,
      name: 'POST /reservations',
      kind: 2,
      startMs: startMs + 52,
      durationMs: 34 + (index % 13) * 3,
      attributes: [
        kv('http.request.method', 'POST'),
        kv('http.route', '/reservations'),
        kv('http.response.status_code', inventoryError ? 409 : 201),
      ],
      error: inventoryError ? orderFailure : null,
    }),
  );
  addSpan(
    groups,
    'order-service',
    versionFor('order-service', index),
    otlpSpan({
      trace,
      id: paymentClient,
      parent: orderServer,
      name: 'authorize',
      kind: 3,
      startMs: startMs + 82,
      durationMs: paymentError ? 1_950 : 118 + (index % 25) * 11,
      attributes: [kv('peer.service', 'payment-service')],
      error: paymentFailure,
    }),
  );
  addSpan(
    groups,
    'payment-service',
    versionFor('payment-service', index),
    otlpSpan({
      trace,
      id: paymentServer,
      parent: paymentClient,
      name: 'POST /authorize',
      kind: 2,
      startMs: startMs + 87,
      durationMs: paymentError ? 1_900 : 105 + (index % 24) * 10,
      attributes: [
        kv('http.request.method', 'POST'),
        kv('http.route', '/authorize'),
        kv('http.response.status_code', paymentError ? 504 : 200),
      ],
      error: paymentFailure,
    }),
  );
  addSpan(
    groups,
    'payment-service',
    versionFor('payment-service', index),
    otlpSpan({
      trace,
      id: stripeClient,
      parent: paymentServer,
      name: 'POST',
      kind: 3,
      startMs: startMs + 95,
      durationMs: paymentError ? 1_780 : 86 + (index % 19) * 9,
      attributes: [kv('http.request.method', 'POST'), kv('server.address', 'api.stripe.test')],
      error: paymentFailure,
    }),
  );
  addSpan(
    groups,
    'order-service',
    versionFor('order-service', index),
    otlpSpan({
      trace,
      id: producer,
      parent: orderServer,
      name: 'publish orders.created',
      kind: 4,
      startMs: startMs + 126,
      durationMs: 12 + (index % 7),
      attributes: [
        kv('messaging.system', 'kafka'),
        kv('messaging.destination.name', 'orders.created'),
        kv('messaging.operation.name', 'publish'),
      ],
    }),
  );
  addSpan(
    groups,
    'notification-service',
    versionFor('notification-service', index),
    otlpSpan({
      trace,
      id: notificationConsumer,
      parent: producer,
      name: 'process orders.created',
      kind: 5,
      startMs: startMs + 142,
      durationMs: notificationError ? 820 : 72 + (index % 16) * 8,
      attributes: [
        kv('messaging.system', 'kafka'),
        kv('messaging.destination.name', 'orders.created'),
        kv('messaging.operation.name', 'process'),
      ],
      error: notificationError
        ? {
            type: 'NotificationDeliveryError',
            message: 'Transactional message provider rejected delivery',
            stack: 'notification.worker.deliver\nnotification.provider.send\nworker.run',
          }
        : null,
    }),
  );
  if (index % 2 === 0) {
    addSpan(
      groups,
      'user-service',
      versionFor('user-service', index),
      otlpSpan({
        trace,
        id: userServer,
        parent: orderServer,
        name: 'GET /users/{id}',
        kind: 2,
        startMs: startMs + 30,
        durationMs: 24 + (index % 12) * 3,
        attributes: [
          kv('http.request.method', 'GET'),
          kv('http.route', '/users/{id}'),
          kv('http.response.status_code', 200),
        ],
      }),
    );
  }
}

export async function seedApm(api) {
  const traceCount = positiveIntegerEnv('MS_SEED_TRACE_COUNT', 1_200);
  const batchSize = Math.min(
    traceCount,
    positiveIntegerEnv('MS_SEED_TRACE_BATCH_SIZE', 100),
  );
  const baseMs = NOW_MS - 240_000;
  let spanCount = 0;
  for (let offset = 0; offset < traceCount; offset += batchSize) {
    const groups = new Map();
    for (let i = offset; i < offset + batchSize; i += 1) {
      const startMs = baseMs + Math.floor((i / traceCount) * 150_000);
      buildCommerceTrace(groups, i, startMs);
    }
    const resourceSpans = [...groups.values()].map(({ service, version, spans }) => {
      spanCount += spans.length;
      return resourceGroup(service, version, spans);
    });
    await api.request(
      'POST',
      '/traces',
      { resourceSpans },
      { 'stream-name': process.env.MS_SEED_TRACE_STREAM ?? 'default' },
    );
  }
  return { traces: traceCount, spans: spanCount };
}
