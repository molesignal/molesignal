import { describe, expect, it } from 'vitest';

import type { StreamField } from '@/api/streams';
import { buildSignalJumps } from '@/shell/signalReference/links';

import { requestedLogLinkFromParams } from '../queryState';
import {
  logFieldFor,
  logLinkKey,
  logLinkStatement,
  logMeaningOf,
  type LogMeaning,
  pinnedLogFilters,
} from './model';

const field = (name: string, data_type: StreamField['dataType'] = 'utf8'): StreamField => ({
  name,
  dataType: data_type,
  nullable: true,
  indexed: false,
});

/** Logs as OTLP writes them: resource attributes and the record's own under their semantic-convention names. */
const OTLP = [
  field('_timestamp', 'timestamp'),
  field('service.name'),
  field('host.name'),
  field('deployment.environment'),
  field('severity_text'),
  field('body'),
  field('trace_id'),
  field('span_id'),
  field('http.request.method'),
  field('http.route'),
  field('http.response.status_code', 'int64'),
];

/** Logs written as JSON lines, each field named as the application chose. */
const JSON_LINES = [
  field('_timestamp', 'timestamp'),
  field('level'),
  field('service'),
  field('hostname'),
  field('message'),
  field('traceId'),
  field('method'),
  field('path'),
  field('status_code', 'int64'),
  field('env'),
];

const named = (meaning: LogMeaning, fields: readonly StreamField[]) => logFieldFor(meaning, fields)?.name ?? null;

describe('logFieldFor', () => {
  it('finds the field OTLP keeps each meaning in', () => {
    expect(named('service', OTLP)).toBe('service.name');
    expect(named('host', OTLP)).toBe('host.name');
    expect(named('environment', OTLP)).toBe('deployment.environment');
    expect(named('trace_id', OTLP)).toBe('trace_id');
    expect(named('span_id', OTLP)).toBe('span_id');
    expect(named('method', OTLP)).toBe('http.request.method');
    expect(named('route', OTLP)).toBe('http.route');
    expect(named('status', OTLP)).toBe('http.response.status_code');
  });

  it('finds the field a JSON line keeps it in, however the name is spelt', () => {
    expect(named('service', JSON_LINES)).toBe('service');
    expect(named('host', JSON_LINES)).toBe('hostname');
    expect(named('trace_id', JSON_LINES)).toBe('traceId');
    expect(named('method', JSON_LINES)).toBe('method');
    expect(named('route', JSON_LINES)).toBe('path');
    expect(named('status', JSON_LINES)).toBe('status_code');
    expect(named('environment', JSON_LINES)).toBe('env');
    expect(named('service', [field('serviceName')])).toBe('serviceName');
  });

  it('prefers the semantic-convention name where a stream has both', () => {
    expect(named('service', [field('service'), field('service.name')])).toBe('service.name');
  });

  it('finds a qualified name at the end of a longer one, and no bare word there', () => {
    expect(named('route', [field('attributes.http.route')])).toBe('attributes.http.route');
    expect(named('service', [field('resource.service.name')])).toBe('resource.service.name');
    // The file a collector read the log from is no route, and the service a request went to
    // is not the one that wrote the log.
    expect(named('route', [field('log.file.path')])).toBeNull();
    expect(named('service', [field('peer.service')])).toBeNull();
  });

  it('has none for a meaning the stream keeps nowhere, or only in a structure', () => {
    expect(named('host', [field('service.name')])).toBeNull();
    expect(named('service', [field('service', 'json')])).toBeNull();
  });
});

describe('logMeaningOf', () => {
  it('knows what another signal names a thing by', () => {
    expect(logMeaningOf('service_name')).toBe('service');
    expect(logMeaningOf('service.name')).toBe('service');
    expect(logMeaningOf('resource.service.name')).toBe('service');
    expect(logMeaningOf('deployment_environment')).toBe('environment');
    expect(logMeaningOf('http.status_code')).toBe('status');
    expect(logMeaningOf('traceid')).toBe('trace_id');
  });

  it('gives no meaning to a name it does not know', () => {
    expect(logMeaningOf('alert')).toBeNull();
    expect(logMeaningOf('instance')).toBeNull();
    expect(logMeaningOf('k8s.node.name')).toBeNull();
  });

  it('knows a pod by the names Kubernetes collectors give it', () => {
    expect(logMeaningOf('k8s.pod.name')).toBe('pod');
    expect(logMeaningOf('kubernetes.pod_name')).toBe('pod');
  });
});

describe('logLinkStatement', () => {
  it('writes what a link asks in the stream\'s own field names', () => {
    expect(
      logLinkStatement({ service: 'checkout', status: '500', environment: "o'brien" }, OTLP),
    ).toEqual({
      statement: "service.name = 'checkout' AND http.response.status_code = 500 AND deployment.environment = 'o\\'brien'",
      gaps: [],
    });
    expect(logLinkStatement({ trace_id: 'abc', span_id: 'def' }, JSON_LINES)).toEqual({
      statement: "traceId = 'abc'",
      gaps: [{ meaning: 'span_id', value: 'def' }],
    });
  });

  it('leaves out what the stream keeps no field for, and says what', () => {
    expect(logLinkStatement({ service: 'checkout', host: 'node-3' }, [field('message')])).toEqual({
      statement: '',
      gaps: [
        { meaning: 'service', value: 'checkout' },
        { meaning: 'host', value: 'node-3' },
      ],
    });
  });

  it('writes no field the field query cannot name as it is', () => {
    expect(logLinkStatement({ service: 'checkout' }, [field('service-name')])).toEqual({
      statement: '',
      gaps: [{ meaning: 'service', value: 'checkout' }],
    });
  });

  it('says the same request the same way, whatever order it was read in', () => {
    expect(logLinkKey({ host: 'a', service: 'b' })).toBe(logLinkKey({ service: 'b', host: 'a' }));
    expect(logLinkKey({ host: 'a' })).not.toBe(logLinkKey({ service: 'a' }));
  });
});

describe('pinnedLogFilters', () => {
  it('applies each pinned filter on the stream\'s own field', () => {
    expect(
      pinnedLogFilters(
        [
          { key: 'service_name', value: 'checkout', operator: '=' },
          { key: 'severity_text', value: 'DEBUG', operator: '!=' },
          { key: 'environment', value: 'prod' },
        ],
        OTLP,
      ),
    ).toEqual({
      applied: [
        { field: 'service.name', operator: '=', value: 'checkout' },
        { field: 'severity_text', operator: '!=', value: 'DEBUG' },
        { field: 'deployment.environment', operator: '=', value: 'prod' },
      ],
      unmatched: [],
    });
  });

  it('takes a key kept in lower case for the field of that name', () => {
    expect(pinnedLogFilters([{ key: 'servicename', value: 'checkout' }], [field('serviceName')]).applied).toEqual([
      { field: 'serviceName', operator: '=', value: 'checkout' },
    ]);
  });

  it('applies none the stream keeps no field for, and keeps them apart', () => {
    const alert = { key: 'alert', value: 'A-1', operator: '=' as const };
    const host = { key: 'host', value: 'node-3', operator: '=' as const };

    expect(pinnedLogFilters([alert, host, { key: '', value: 'x' }], JSON_LINES.slice(0, 3))).toEqual({
      applied: [],
      unmatched: [alert, host],
    });
  });
});

describe('a jump from another signal to the logs', () => {
  const time = { from: '2026-07-25T10:55:00.000Z', to: '2026-07-25T11:05:00.000Z' };
  const logsOf = (jumps: ReturnType<typeof buildSignalJumps>) =>
    new URL(jumps.find((jump) => jump.id === 'logs')!.to, 'https://molesignal.local').searchParams;

  it('from a metric series names only fields the stream has', () => {
    const params = logsOf(
      buildSignalJumps(
        'service',
        'checkout',
        time,
        {
          labelName: 'service_name',
          labels: {
            __name__: 'http_requests_total',
            service_name: 'checkout',
            instance: 'node-3:9100',
            deployment_environment: 'prod',
          },
        },
        [{ key: 'service_name', value: 'checkout', operator: '=' }],
      ),
    );

    expect(logLinkStatement(requestedLogLinkFromParams(params)!, OTLP)).toEqual({
      statement: "service.name = 'checkout' AND deployment.environment = 'prod'",
      gaps: [],
    });
    expect(logLinkStatement(requestedLogLinkFromParams(params)!, JSON_LINES)).toEqual({
      statement: "service = 'checkout' AND env = 'prod'",
      gaps: [],
    });
  });

  it('from a span names the trace as the stream keeps it', () => {
    const params = logsOf(
      buildSignalJumps('span_id', 'span-1', time, {
        labels: { trace_id: 'trace-1', span_id: 'span-1', 'service.name': 'checkout' },
      }),
    );

    expect(logLinkStatement(requestedLogLinkFromParams(params)!, JSON_LINES).statement).toBe("traceId = 'trace-1'");
  });
});
