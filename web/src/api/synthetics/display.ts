import type { MonitorSpec, ValueSource } from './types';

export function valueSourceText(source: ValueSource | undefined): string {
  if (!source) return '—';
  if (source.source === 'literal') return source.value;
  if (source.source === 'variable') return `{{${source.name}}}`;
  return `{{${source.reference}}}`;
}

export function monitorTarget(spec: MonitorSpec | undefined): string {
  if (!spec) return '—';
  switch (spec.kind) {
    case 'http':
      return valueSourceText(spec.configuration.steps[0]?.url);
    case 'browser': {
      const navigate = spec.configuration.steps.find((step) => step.action.action === 'navigate');
      return valueSourceText(navigate?.action.url as ValueSource | undefined);
    }
    case 'tcp':
      return `${valueSourceText(spec.configuration.host)}:${spec.configuration.port}`;
    case 'dns':
      return `${spec.configuration.name} · ${spec.configuration.record_type}`;
    case 'icmp':
      return spec.configuration.host;
    case 'tls':
      return `${spec.configuration.host}:${spec.configuration.port}`;
    case 'grpc':
      return spec.configuration.endpoint;
    case 'heartbeat':
      return 'Heartbeat';
  }
}
