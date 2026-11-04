import type { AlertStage, PipelineSignalType } from '../../PipelineGraph/model';

/**
 * An event of each signal to try a pipeline on before there is a real one at
 * hand: shaped like what an agent sends, with a JSON message and a nested
 * object so the common processors have something to work on.
 */
const SAMPLES: Record<PipelineSignalType, unknown> = {
  // A profile's metadata row: the profile itself is an object elsewhere, which no step touches.
  profiles: {
    id: 'pr-01J9Z',
    service: 'checkout',
    profile_type: 'cpu',
    object_key: 'blobs/v1/org/profiles/checkout/cpu/20261031/pr-01J9Z.pprof.zst',
    archived_bytes: 40_960,
    duration_nanos: 10_000_000_000,
    sample_count: 1_250,
    total_value: 9_800_000_000,
    unsymbolized: false,
    trace_id: '',
    span_id: '',
    labels: { pod: 'checkout-7d9f8' },
  },
  logs: {
    _timestamp: 1_779_530_400_000_000,
    message: '{"level":"WARN","msg":"payment retry","trace_id":"4bf92f3577b34da6","user":{"id":42,"email":"ada@example.com"}}',
    level: 'WARN',
    appname: 'checkout',
    kubernetes: { namespace: 'shop', pod_name: 'checkout-7d9f8' },
  },
  // A sample as a metrics stream stores it: its labels beside the metric name and the value.
  metrics: {
    metric_name: 'http_requests_total',
    value: 1,
    metric_kind: 'counter',
    service: 'checkout',
    status: '200',
    method: 'POST',
    pod: 'checkout-7d9f8',
  },
  traces: {
    trace_id: '4bf92f3577b34da6a3ce929d0e0e4736',
    span_id: '00f067aa0ba902b7',
    parent_span_id: '53995c3f42cd8ad8',
    name: 'POST /orders',
    'service.name': 'checkout',
    duration_ns: 182_000_000,
    status_code: 'OK',
    'http.url': 'https://shop.example/orders?token=abc',
  },
};

/** An alert event as an alert pipeline's steps see it, before its incident opens. */
const ALERT_SAMPLE = {
  summary: 'Checkout error rate above 5%',
  severity: 'error',
  labels: { service: 'checkout', env: 'prod', host_name: 'web-01' },
  annotations: { runbook: 'https://wiki.example.com/checkout' },
  affected_services: ['checkout'],
};

/** Where the sample alert event came from, as the steps read it (`%rule_name`, `%source`…). */
export const ALERT_SAMPLE_METADATA = {
  rule_id: 'rule-1',
  rule_name: 'Checkout error rate',
  source: 'rule',
  event: 'opened',
} as const;

/** What the steps of an alert pipeline read in a test, by when the pipeline runs. */
export function alertSampleMetadata(metadata: Readonly<Record<string, string>>, stage: AlertStage): Record<string, string> {
  return stage === 'notify'
    ? { ...metadata, event: 'triggered', policy_id: 'policy-1', policy_name: 'On-call alerts' }
    : { ...metadata };
}

export function sampleEvent(signalType: PipelineSignalType | 'alert'): string {
  return JSON.stringify(signalType === 'alert' ? ALERT_SAMPLE : SAMPLES[signalType], null, 2);
}
