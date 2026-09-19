import { describe, expect, it } from 'vitest';

import {
  datasourceLinkForStream,
  intakePathForSignal,
} from './datasourceLink';

describe('streamDatasourceLink', () => {
  it.each([
    ['logs', '/datasource/custom/curl'],
    ['metrics', '/datasource/applications/opentelemetry'],
    ['traces', '/datasource/applications/opentelemetry'],
    ['profiles', '/datasource/recommended/continuous-profiling'],
  ] as const)('routes %s streams to the matching intake guide', (streamType, pathname) => {
    const target = new URL(
      datasourceLinkForStream({ name: `app ${streamType}`, stream_type: streamType }),
      'https://molesignal.test',
    );

    expect(target.pathname).toBe(pathname);
    expect(target.searchParams.get('signal')).toBe(streamType);
    expect(target.searchParams.get('stream')).toBe(`app ${streamType}`);
  });

  it('uses the selected stream in native intake endpoints', () => {
    expect(intakePathForSignal('logs', 'app logs')).toBe(
      '/api/v1/intake/logs/app%20logs',
    );
    expect(intakePathForSignal('traces', 'checkout')).toBe(
      '/api/v1/intake/traces/checkout',
    );
  });

  it('uses the dedicated profiles endpoint', () => {
    expect(intakePathForSignal('profiles', 'default')).toBe(
      '/api/v1/profiles/intake',
    );
  });
});
