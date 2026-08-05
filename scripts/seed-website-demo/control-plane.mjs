import { RUN_ID } from './shared.mjs';

function panel(id, title, x, y, query, type = 'time_series') {
  return {
    kind: 'panel',
    id,
    title,
    gridPos: { x, y, w: 12, h: 22 },
    queryOptions: {},
    queries: [
      {
        refId: 'A',
        enabled: true,
        dataSourceType: query.language === 'promql' ? 'metrics' : 'logs',
        query,
      },
    ],
    transformations: [],
    visualization: { type, schemaVersion: 1, options: {} },
    fieldConfig: {},
    overrides: [],
    links: [],
  };
}

export async function seedControlPlane(api) {
  const escalation = await api.post('/alerts/escalations', {
    name: `Commerce Critical Escalation · ${RUN_ID.slice(8, 12)}`,
    steps: [
      {
        targets: [{ kind: 'user', user_id: api.userId }],
        ack_timeout_secs: 300,
      },
    ],
    repeat: true,
    max_loops: 3,
  });
  const alert = await api.post('/alerts/rules', {
    name: `Checkout error budget burn · ${RUN_ID.slice(8, 12)}`,
    description:
      'Detects sustained checkout failures before the 99.9% availability objective is exhausted.',
    enabled: true,
    query: {
      language: 'sql',
      statement:
        "SELECT COUNT(*) FROM app_logs WHERE service = 'payment-service' AND level = 'error'",
      period_secs: 300,
      stream: { name: 'app_logs', stream_type: 'logs' },
    },
    trigger: {
      operator: 'gt',
      threshold: 1,
      for_periods: 1,
      silence_secs: 900,
    },
    severity: 'critical',
    escalation_policy_id: escalation.id,
    labels: {
      severity: 'critical',
      service: 'payment-service',
      team: 'payments-risk',
      environment: 'production',
    },
    annotations: {
      runbook: 'https://docs.example.test/runbooks/checkout-error-budget',
      summary: 'Checkout failures are consuming the production error budget.',
    },
  });

  const schedule = await api.post('/schedules', {
    name: `Commerce Primary On-call · ${RUN_ID.slice(8, 12)}`,
    description:
      'Primary production rotation for checkout, orders, inventory and payments.',
    timezone: 'UTC',
    enabled: true,
    rotations: [
      {
        id: `commerce-primary-${RUN_ID}`,
        name: 'Primary',
        members: [api.userId],
        kind: 'daily',
        start_at: Date.now() * 1_000 - 86_400_000_000,
      },
    ],
    overrides: [],
  });

  const team = await api.post('/teams', {
    name: `Commerce Platform · ${RUN_ID.slice(8, 12)}`,
    member_ids: [api.userId],
  });
  const normalizeFunction = await api.post('/functions', {
    name: `normalize_commerce_logs_${RUN_ID}`,
    language: 'vrl',
    source:
      '.level = upcase!(.level)\n.service_domain = \"commerce\"\n.environment = \"production\"',
    params_schema: {},
  });
  const pipeline = await api.post('/scheduled_pipelines', {
    name: `Commerce log enrichment · ${RUN_ID.slice(8, 12)}`,
    source_stream: 'app_logs',
    target_stream: 'app_logs_enriched',
    function_steps: [{ function_name: normalizeFunction.name }],
    cron: 'every:5m',
    lookback_secs: 900,
    enabled: true,
  });
  const connector = await api.post('/connectors', {
    name: `Production CloudWatch logs · ${RUN_ID.slice(8, 12)}`,
    kind: 'aws_cloudwatch_logs',
    enabled: false,
    config_json: {
      target_stream: 'app_logs',
      log_group: '/aws/commerce/production',
      region: 'us-east-1',
      access_key: 'example-access-key',
      secret_key: 'example-secret-key',
    },
  });

  const dashboard = await api.post('/dashboards', {
    folder_id: null,
    model: {
      engine: 'molesignal-dashboard',
      id: '',
      uid: `commerce-production-${RUN_ID}`,
      title: 'Commerce Production Overview',
      tags: ['production', 'commerce', 'slo'],
      description:
        'Golden signals for the order, inventory and payment workflow.',
      editable: true,
      defaultDashboard: false,
      timezone: 'browser',
      schemaVersion: 2,
      version: 1,
      refresh: '30s',
      time: { from: 'now-6h', to: 'now' },
      timeSettings: {
        defaultFrom: 'now-6h',
        defaultTo: 'now',
        timezone: 'browser',
      },
      refreshSettings: {
        enabled: true,
        mode: 'interval',
        defaultInterval: '30s',
        allowedIntervals: ['off', '10s', '30s', '1m', '5m'],
      },
      variables: [],
      annotations: [],
      links: [],
      layout: { type: 'grid', columns: 24, rowHeight: 8, gap: 8 },
      elements: [
        panel(
          'commerce-throughput',
          'Order throughput',
          0,
          0,
          {
            language: 'promql',
            expression:
              'sum by (service) (rate(http_requests_total[5m]))',
          },
        ),
        panel(
          'commerce-latency',
          'Checkout P95 latency',
          12,
          0,
          {
            language: 'promql',
            expression:
              'http_request_duration_ms{quantile="p95"}',
          },
        ),
        panel(
          'commerce-errors',
          'Payment failures',
          0,
          22,
          {
            language: 'sql',
            expression:
              "SELECT * FROM app_logs WHERE service = 'payment-service' AND level = 'error' ORDER BY _timestamp DESC LIMIT 100",
            streamName: 'app_logs',
            streamType: 'logs',
          },
          'logs',
        ),
        panel(
          'commerce-cpu',
          'Service CPU saturation',
          12,
          22,
          {
            language: 'promql',
            expression: 'process_cpu_usage',
          },
        ),
      ],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      createdBy: '',
      updatedBy: '',
    },
  });

  return {
    alert: alert.id,
    escalation: escalation.id,
    schedule: schedule.id,
    team: team.id,
    function: normalizeFunction.id,
    pipeline: pipeline.id,
    connector: connector.id,
    dashboard: dashboard.id,
  };
}
