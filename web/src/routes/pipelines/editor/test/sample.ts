import type { PipelineSignalType } from '../../PipelineGraph/model';

/**
 * An event of each signal to try a pipeline on before there is a real one at
 * hand: shaped like what an agent sends, with a JSON message and a nested
 * object so the common processors have something to work on.
 */
const SAMPLES: Record<PipelineSignalType, unknown> = {
  logs: {
    _timestamp: 1_779_530_400_000_000,
    message: '{"level":"WARN","msg":"payment retry","trace_id":"4bf92f3577b34da6","user":{"id":42,"email":"ada@example.com"}}',
    level: 'WARN',
    appname: 'checkout',
    kubernetes: { namespace: 'shop', pod_name: 'checkout-7d9f8' },
  },
  metrics: {
    name: 'http_requests_total',
    value: 1,
    timestamp: '2026-05-23T10:00:00Z',
    tags: { service: 'checkout', status: '200', method: 'POST' },
  },
  traces: {
    trace_id: '4bf92f3577b34da6a3ce929d0e0e4736',
    span_id: '00f067aa0ba902b7',
    name: 'POST /orders',
    'service.name': 'checkout',
    duration_ns: 182_000_000,
    status_code: 'OK',
  },
};

export function sampleEvent(signalType: PipelineSignalType): string {
  return JSON.stringify(SAMPLES[signalType], null, 2);
}
